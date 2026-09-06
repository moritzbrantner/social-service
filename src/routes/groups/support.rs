use std::collections::HashSet;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::ApiError,
    features::Feature,
    groups::{Group, GroupMember, GroupRole, GroupRow},
    state::AppState,
};

type RoleTransition = (Option<GroupRole>, Option<GroupRole>);

pub(super) async fn load_group(
    state: &AppState,
    app_id: Uuid,
    group_id: Uuid,
    viewer_id: Uuid,
) -> Result<Group, ApiError> {
    let row = sqlx::query_as::<_, GroupRow>(
        "SELECT g.id, g.name, g.avatar_media_id, g.created_by, g.created_at, g.updated_at, g.version FROM groups g JOIN group_members viewer ON viewer.app_id = g.app_id AND viewer.group_id = g.id WHERE g.app_id = $1 AND g.id = $2 AND viewer.user_id = $3",
    )
    .bind(app_id)
    .bind(group_id)
    .bind(viewer_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("group"))?;
    let members = sqlx::query_as::<_, GroupMember>(
        "SELECT user_id, role, joined_at, updated_at, version FROM group_members WHERE app_id = $1 AND group_id = $2 ORDER BY joined_at ASC, user_id ASC",
    )
    .bind(app_id)
    .bind(group_id)
    .fetch_all(&state.pool)
    .await?;
    let chat_conversation_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT conversation_id FROM group_conversations WHERE app_id = $1 AND group_id = $2",
    )
    .bind(app_id)
    .bind(group_id)
    .fetch_optional(&state.pool)
    .await?;

    Ok(Group {
        row,
        members,
        chat_conversation_id,
    })
}

pub(super) async fn lock_actor_role(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
    actor_id: Uuid,
) -> Result<GroupRole, ApiError> {
    sqlx::query_scalar::<_, GroupRole>(
        "SELECT gm.role FROM groups g JOIN group_members gm ON gm.app_id = g.app_id AND gm.group_id = g.id WHERE g.app_id = $1 AND g.id = $2 AND gm.user_id = $3 FOR UPDATE OF g",
    )
    .bind(app_id)
    .bind(group_id)
    .bind(actor_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(ApiError::NotFound("group"))
}

pub(super) async fn group_member_ids(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM group_members WHERE app_id = $1 AND group_id = $2 ORDER BY joined_at ASC, user_id ASC",
    )
    .bind(app_id)
    .bind(group_id)
    .fetch_all(&mut **transaction)
    .await?)
}

pub(super) async fn linked_chat_id(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
) -> Result<Option<Uuid>, ApiError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT conversation_id FROM group_conversations WHERE app_id = $1 AND group_id = $2",
    )
    .bind(app_id)
    .bind(group_id)
    .fetch_optional(&mut **transaction)
    .await?)
}

pub(super) async fn sync_linked_chat(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
) -> Result<(), ApiError> {
    if let Some(conversation_id) = linked_chat_id(transaction, app_id, group_id).await? {
        sync_conversation_members(transaction, app_id, group_id, conversation_id).await?;
    }
    Ok(())
}

pub(super) async fn sync_conversation_members(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO conversation_members (app_id, conversation_id, user_id) SELECT app_id, $3, user_id FROM group_members WHERE app_id = $1 AND group_id = $2 ON CONFLICT (conversation_id, user_id) DO NOTHING",
    )
    .bind(app_id)
    .bind(group_id)
    .bind(conversation_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "DELETE FROM conversation_members cm WHERE cm.app_id = $1 AND cm.conversation_id = $3 AND NOT EXISTS (SELECT 1 FROM group_members gm WHERE gm.app_id = $1 AND gm.group_id = $2 AND gm.user_id = cm.user_id)",
    )
    .bind(app_id)
    .bind(group_id)
    .bind(conversation_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn append_membership_event(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
    user_id: Uuid,
    event_type: &'static str,
    actor_id: Uuid,
    role_transition: RoleTransition,
) -> Result<(), ApiError> {
    let (previous_role, new_role) = role_transition;
    sqlx::query(
        "INSERT INTO group_membership_events (id, app_id, group_id, user_id, event_type, actor_id, previous_role, new_role) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(Uuid::new_v4())
    .bind(app_id)
    .bind(group_id)
    .bind(user_id)
    .bind(event_type)
    .bind(actor_id)
    .bind(previous_role)
    .bind(new_role)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn touch_group(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    group_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE groups SET updated_at = now(), version = version + 1 WHERE app_id = $1 AND id = $2",
    )
    .bind(app_id)
    .bind(group_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(super) async fn validate_avatar(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    actor_id: Uuid,
    avatar_media_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let Some(media_id) = avatar_media_id else {
        return Ok(());
    };
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM media_assets WHERE app_id = $1 AND id = $2 AND owner_id = $3)",
    )
    .bind(app_id)
    .bind(media_id)
    .bind(actor_id)
    .fetch_one(&mut **transaction)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "group avatar media must belong to the current user and app".to_owned(),
        ))
    }
}

pub(super) async fn ensure_group_actor_available(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }
    let unavailable = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_account_states WHERE app_id = $1 AND user_id = $2 AND state IN ('suspended', 'banned'))",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if unavailable {
        Err(ApiError::Forbidden)
    } else {
        Ok(())
    }
}

pub(super) async fn ensure_members_available(
    state: &AppState,
    app_id: Uuid,
    member_ids: &[Uuid],
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }
    let unavailable = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_account_states WHERE app_id = $1 AND user_id = ANY($2) AND state IN ('suspended', 'banned'))",
    )
    .bind(app_id)
    .bind(member_ids.to_vec())
    .fetch_one(&state.pool)
    .await?;
    if unavailable {
        Err(ApiError::BadRequest(
            "group members must be available in this app".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub(super) async fn ensure_members_available_in_transaction(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    member_ids: &[Uuid],
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }
    let unavailable = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_account_states WHERE app_id = $1 AND user_id = ANY($2) AND state IN ('suspended', 'banned'))",
    )
    .bind(app_id)
    .bind(member_ids.to_vec())
    .fetch_one(&mut **transaction)
    .await?;
    if unavailable {
        Err(ApiError::BadRequest(
            "group members must be available in this app".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn normalize_members(
    owner_id: Uuid,
    member_ids: Vec<Uuid>,
) -> Result<Vec<Uuid>, ApiError> {
    let mut seen = HashSet::new();
    let mut members = Vec::with_capacity(member_ids.len() + 1);
    for user_id in std::iter::once(owner_id).chain(member_ids) {
        if seen.insert(user_id) {
            members.push(user_id);
        }
    }
    if members.len() > 100 {
        return Err(ApiError::BadRequest(
            "a group can contain at most 100 unique members".to_owned(),
        ));
    }
    Ok(members)
}

pub(super) fn validate_name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();
    if !(1..=120).contains(&name.chars().count()) {
        return Err(ApiError::BadRequest(
            "group name must contain 1-120 characters".to_owned(),
        ));
    }
    Ok(name)
}
