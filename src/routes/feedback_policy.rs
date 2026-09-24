use uuid::Uuid;

use crate::{
    error::ApiError,
    features::Feature,
    moderation::{TargetType, ensure_account_visible, ensure_content_visible},
    relationships::ensure_not_blocked,
    state::AppState,
};

pub(super) async fn ensure_actor_active(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let profile_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM profiles WHERE app_id = $1 AND user_id = $2)",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if !profile_exists {
        return Err(ApiError::NotFound("profile"));
    }

    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }
    let restricted = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_account_states WHERE app_id = $1 AND user_id = $2 AND state IN ('suspended', 'banned'))",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if restricted {
        Err(ApiError::Forbidden)
    } else {
        Ok(())
    }
}

pub(super) async fn ensure_comment_visible(
    state: &AppState,
    app_id: Uuid,
    comment_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    state.features.require(Feature::Comments)?;
    let target = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT c.author_id, c.post_id FROM comments c WHERE c.app_id = $1 AND c.id = $2 AND c.deleted_at IS NULL",
    )
    .bind(app_id)
    .bind(comment_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("comment"))?;

    super::posts::ensure_post_visible(state, app_id, target.1, viewer_id).await?;
    ensure_not_blocked(state, app_id, viewer_id, target.0, "comment").await?;
    ensure_account_visible(state, app_id, target.0).await?;
    ensure_content_visible(state, app_id, TargetType::Comment, comment_id, "comment").await?;
    Ok(target.0)
}
