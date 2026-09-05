use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    auth::RequestContext,
    error::ApiError,
    features::Feature,
    groups::{CreateGroup, Group, GroupRole, SetGroupRole, UpdateGroup},
    models::{Conversation, ConversationRow},
    state::AppState,
};

use super::support::{
    append_membership_event, ensure_group_actor_available, ensure_members_available,
    ensure_members_available_in_transaction, group_member_ids, linked_chat_id, load_group,
    lock_actor_role, normalize_members, sync_conversation_members, sync_linked_chat, touch_group,
    validate_avatar, validate_name,
};

pub async fn create_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateGroup>,
) -> Result<Json<Group>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;
    let name = validate_name(&input.name)?;
    if input.avatar_media_id.is_some() {
        state.features.require(Feature::Media)?;
    }

    let member_ids = normalize_members(context.user_id.0, input.member_ids)?;
    ensure_members_available(&state, context.app_id.0, &member_ids).await?;

    let group_id = Uuid::new_v4();
    let mut transaction = state.pool.begin().await?;
    validate_avatar(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        input.avatar_media_id,
    )
    .await?;

    sqlx::query(
        "INSERT INTO groups (id, app_id, name, avatar_media_id, created_by) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(group_id)
    .bind(context.app_id.0)
    .bind(name)
    .bind(input.avatar_media_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;

    for user_id in member_ids {
        let role = if user_id == context.user_id.0 {
            GroupRole::Owner
        } else {
            GroupRole::Member
        };
        sqlx::query(
            "INSERT INTO group_members (app_id, group_id, user_id, role) VALUES ($1, $2, $3, $4)",
        )
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(user_id)
        .bind(role)
        .execute(&mut *transaction)
        .await?;
        append_membership_event(
            &mut transaction,
            context.app_id.0,
            group_id,
            user_id,
            "joined",
            context.user_id.0,
            None,
            Some(role),
        )
        .await?;
    }

    transaction.commit().await?;
    Ok(Json(
        load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
    ))
}

pub async fn update_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
    Json(input): Json<UpdateGroup>,
) -> Result<Json<Group>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;
    let name = validate_name(&input.name)?;
    if input.avatar_media_id.is_some() {
        state.features.require(Feature::Media)?;
    }

    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if !actor_role.can_manage_group() {
        return Err(ApiError::Forbidden);
    }
    validate_avatar(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        input.avatar_media_id,
    )
    .await?;
    sqlx::query(
        "UPDATE groups SET name = $1, avatar_media_id = $2, updated_at = now(), version = version + 1 WHERE app_id = $3 AND id = $4",
    )
    .bind(name)
    .bind(input.avatar_media_id)
    .bind(context.app_id.0)
    .bind(group_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Json(
        load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
    ))
}

pub async fn add_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Group>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;

    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if !actor_role.can_manage_members() {
        return Err(ApiError::Forbidden);
    }
    ensure_members_available_in_transaction(
        &state,
        &mut transaction,
        context.app_id.0,
        &[user_id],
    )
    .await?;

    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM group_members WHERE app_id = $1 AND group_id = $2 AND user_id = $3)",
    )
    .bind(context.app_id.0)
    .bind(group_id)
    .bind(user_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !exists {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM group_members WHERE app_id = $1 AND group_id = $2",
        )
        .bind(context.app_id.0)
        .bind(group_id)
        .fetch_one(&mut *transaction)
        .await?;
        if count >= 100 {
            return Err(ApiError::BadRequest(
                "a group can contain at most 100 members".to_owned(),
            ));
        }

        sqlx::query(
            "INSERT INTO group_members (app_id, group_id, user_id, role) VALUES ($1, $2, $3, 'member')",
        )
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
        append_membership_event(
            &mut transaction,
            context.app_id.0,
            group_id,
            user_id,
            "joined",
            context.user_id.0,
            None,
            Some(GroupRole::Member),
        )
        .await?;
        sync_linked_chat(&mut transaction, context.app_id.0, group_id).await?;
        touch_group(&mut transaction, context.app_id.0, group_id).await?;
    }
    transaction.commit().await?;

    Ok(Json(
        load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
    ))
}

pub async fn remove_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Group>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;

    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if !actor_role.can_manage_members() {
        return Err(ApiError::Forbidden);
    }

    let target_role = sqlx::query_scalar::<_, GroupRole>(
        "SELECT role FROM group_members WHERE app_id = $1 AND group_id = $2 AND user_id = $3",
    )
    .bind(context.app_id.0)
    .bind(group_id)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(target_role) = target_role else {
        transaction.commit().await?;
        return Ok(Json(
            load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
        ));
    };

    if target_role == GroupRole::Owner
        || (actor_role == GroupRole::Admin && target_role != GroupRole::Member)
    {
        return Err(ApiError::Forbidden);
    }

    append_membership_event(
        &mut transaction,
        context.app_id.0,
        group_id,
        user_id,
        "left",
        context.user_id.0,
        Some(target_role),
        None,
    )
    .await?;
    sqlx::query(
        "DELETE FROM group_members WHERE app_id = $1 AND group_id = $2 AND user_id = $3",
    )
    .bind(context.app_id.0)
    .bind(group_id)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sync_linked_chat(&mut transaction, context.app_id.0, group_id).await?;
    touch_group(&mut transaction, context.app_id.0, group_id).await?;
    transaction.commit().await?;

    Ok(Json(
        load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
    ))
}

pub async fn set_member_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((group_id, user_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<SetGroupRole>,
) -> Result<Json<Group>, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;

    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if actor_role != GroupRole::Owner {
        return Err(ApiError::Forbidden);
    }

    let target_role = sqlx::query_scalar::<_, GroupRole>(
        "SELECT role FROM group_members WHERE app_id = $1 AND group_id = $2 AND user_id = $3",
    )
    .bind(context.app_id.0)
    .bind(group_id)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| ApiError::BadRequest("target user is not a group member".to_owned()))?;

    if target_role == input.role {
        transaction.commit().await?;
        return Ok(Json(
            load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
        ));
    }

    if input.role == GroupRole::Owner {
        if user_id == context.user_id.0 {
            transaction.commit().await?;
            return Ok(Json(
                load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
            ));
        }
        sqlx::query(
            "UPDATE group_members SET role = 'admin', updated_at = now(), version = version + 1 WHERE app_id = $1 AND group_id = $2 AND user_id = $3",
        )
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(context.user_id.0)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE group_members SET role = 'owner', updated_at = now(), version = version + 1 WHERE app_id = $1 AND group_id = $2 AND user_id = $3",
        )
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
        append_membership_event(
            &mut transaction,
            context.app_id.0,
            group_id,
            context.user_id.0,
            "role_changed",
            context.user_id.0,
            Some(GroupRole::Owner),
            Some(GroupRole::Admin),
        )
        .await?;
        append_membership_event(
            &mut transaction,
            context.app_id.0,
            group_id,
            user_id,
            "role_changed",
            context.user_id.0,
            Some(target_role),
            Some(GroupRole::Owner),
        )
        .await?;
    } else {
        if user_id == context.user_id.0 {
            return Err(ApiError::BadRequest(
                "transfer ownership before changing the owner's role".to_owned(),
            ));
        }
        sqlx::query(
            "UPDATE group_members SET role = $1, updated_at = now(), version = version + 1 WHERE app_id = $2 AND group_id = $3 AND user_id = $4",
        )
        .bind(input.role)
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
        append_membership_event(
            &mut transaction,
            context.app_id.0,
            group_id,
            user_id,
            "role_changed",
            context.user_id.0,
            Some(target_role),
            Some(input.role),
        )
        .await?;
    }
    touch_group(&mut transaction, context.app_id.0, group_id).await?;
    transaction.commit().await?;

    Ok(Json(
        load_group(&state, context.app_id.0, group_id, context.user_id.0).await?,
    ))
}

pub async fn leave_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Groups)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;

    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if actor_role == GroupRole::Owner {
        return Err(ApiError::BadRequest(
            "transfer group ownership before leaving".to_owned(),
        ));
    }

    append_membership_event(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
        "left",
        context.user_id.0,
        Some(actor_role),
        None,
    )
    .await?;
    sqlx::query(
        "DELETE FROM group_members WHERE app_id = $1 AND group_id = $2 AND user_id = $3",
    )
    .bind(context.app_id.0)
    .bind(group_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    sync_linked_chat(&mut transaction, context.app_id.0, group_id).await?;
    touch_group(&mut transaction, context.app_id.0, group_id).await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ensure_group_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
) -> Result<Json<Conversation>, ApiError> {
    state.features.require(Feature::Groups)?;
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_group_actor_available(&state, context.app_id.0, context.user_id.0).await?;

    let mut transaction = state.pool.begin().await?;
    let actor_role = lock_actor_role(
        &mut transaction,
        context.app_id.0,
        group_id,
        context.user_id.0,
    )
    .await?;
    if !actor_role.can_manage_group() {
        return Err(ApiError::Forbidden);
    }

    let existing_id = linked_chat_id(&mut transaction, context.app_id.0, group_id).await?;
    let row = if let Some(conversation_id) = existing_id {
        sync_conversation_members(
            &mut transaction,
            context.app_id.0,
            group_id,
            conversation_id,
        )
        .await?;
        sqlx::query_as::<_, ConversationRow>(
            "SELECT id, created_at, updated_at, version FROM conversations WHERE app_id = $1 AND id = $2",
        )
        .bind(context.app_id.0)
        .bind(conversation_id)
        .fetch_one(&mut *transaction)
        .await?
    } else {
        let member_ids = group_member_ids(&mut transaction, context.app_id.0, group_id).await?;
        if member_ids.len() < 2 {
            return Err(ApiError::BadRequest(
                "a group chat requires at least 2 members".to_owned(),
            ));
        }
        ensure_members_available_in_transaction(
            &state,
            &mut transaction,
            context.app_id.0,
            &member_ids,
        )
        .await?;

        let row = sqlx::query_as::<_, ConversationRow>(
            "INSERT INTO conversations (id, app_id) VALUES ($1, $2) RETURNING id, created_at, updated_at, version",
        )
        .bind(Uuid::new_v4())
        .bind(context.app_id.0)
        .fetch_one(&mut *transaction)
        .await?;
        for user_id in &member_ids {
            sqlx::query(
                "INSERT INTO conversation_members (app_id, conversation_id, user_id) VALUES ($1, $2, $3)",
            )
            .bind(context.app_id.0)
            .bind(row.id)
            .bind(*user_id)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT INTO group_conversations (app_id, group_id, conversation_id) VALUES ($1, $2, $3)",
        )
        .bind(context.app_id.0)
        .bind(group_id)
        .bind(row.id)
        .execute(&mut *transaction)
        .await?;
        touch_group(&mut transaction, context.app_id.0, group_id).await?;
        row
    };

    let member_ids = group_member_ids(&mut transaction, context.app_id.0, group_id).await?;
    transaction.commit().await?;
    Ok(Json(Conversation { row, member_ids }))
}
