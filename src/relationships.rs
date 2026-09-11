use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{error::ApiError, features::Feature, state::AppState};

pub async fn ensure_not_blocked(
    state: &AppState,
    app_id: Uuid,
    viewer_id: Option<Uuid>,
    subject_id: Uuid,
    resource_name: &'static str,
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Blocks) {
        return Ok(());
    }
    let Some(viewer_id) = viewer_id else {
        return Ok(());
    };
    if viewer_id == subject_id {
        return Ok(());
    }
    if users_are_blocked(state, app_id, viewer_id, subject_id).await? {
        Err(ApiError::NotFound(resource_name))
    } else {
        Ok(())
    }
}

pub async fn users_are_blocked(
    state: &AppState,
    app_id: Uuid,
    left_id: Uuid,
    right_id: Uuid,
) -> Result<bool, ApiError> {
    if !state.features.is_enabled(Feature::Blocks) || left_id == right_id {
        return Ok(false);
    }
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT social_users_blocked($1, $2, $3)")
            .bind(app_id)
            .bind(left_id)
            .bind(right_id)
            .fetch_one(&state.pool)
            .await?,
    )
}

pub async fn users_are_blocked_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    left_id: Uuid,
    right_id: Uuid,
) -> Result<bool, ApiError> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT social_users_blocked($1, $2, $3)")
            .bind(app_id)
            .bind(left_id)
            .bind(right_id)
            .fetch_one(&mut **transaction)
            .await?,
    )
}

pub async fn lock_user_pair(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    left_id: Uuid,
    right_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query("SELECT social_lock_user_pair($1, $2, $3)")
        .bind(app_id)
        .bind(left_id)
        .bind(right_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub async fn members_have_block(
    state: &AppState,
    app_id: Uuid,
    member_ids: &[Uuid],
) -> Result<bool, ApiError> {
    if !state.features.is_enabled(Feature::Blocks) || member_ids.len() < 2 {
        return Ok(false);
    }
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM user_blocks b WHERE b.app_id = $1 AND b.blocker_id = ANY($2) AND b.blocked_id = ANY($2))",
    )
    .bind(app_id)
    .bind(member_ids)
    .fetch_one(&state.pool)
    .await?)
}

pub async fn ensure_direct_conversation_unblocked(
    state: &AppState,
    app_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Blocks) {
        return Ok(());
    }
    let is_group_conversation = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM group_conversations WHERE app_id = $1 AND conversation_id = $2)",
    )
    .bind(app_id)
    .bind(conversation_id)
    .fetch_one(&state.pool)
    .await?;
    if is_group_conversation {
        return Ok(());
    }
    let member_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM conversation_members WHERE app_id = $1 AND conversation_id = $2 ORDER BY user_id ASC",
    )
    .bind(app_id)
    .bind(conversation_id)
    .fetch_all(&state.pool)
    .await?;
    if member_ids.len() == 2 && members_have_block(state, app_id, &member_ids).await? {
        Err(ApiError::Forbidden)
    } else {
        Ok(())
    }
}

pub async fn ensure_relationship_target(
    state: &AppState,
    app_id: Uuid,
    actor_id: Uuid,
    target_id: Uuid,
) -> Result<(), ApiError> {
    if actor_id == target_id {
        return Err(ApiError::BadRequest(
            "a user safety relationship cannot target the current user".to_owned(),
        ));
    }
    let profile_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM profiles WHERE app_id = $1 AND user_id IN ($2, $3)",
    )
    .bind(app_id)
    .bind(actor_id)
    .bind(target_id)
    .fetch_one(&state.pool)
    .await?;
    if profile_count == 2 {
        Ok(())
    } else {
        Err(ApiError::NotFound("profile"))
    }
}
