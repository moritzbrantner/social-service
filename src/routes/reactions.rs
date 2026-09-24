use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    auth::{RequestContext, app_id, optional_user_id},
    error::ApiError,
    features::Feature,
    models::{ReactionCount, ReactionSummary, ReactionTargetType, ReactionType},
    relationships::{lock_user_pair, users_are_blocked_in_transaction},
    state::AppState,
};

use super::{
    feedback_policy::{ensure_actor_active, ensure_comment_visible},
    posts::ensure_post_visible,
};

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id)): Path<(ReactionTargetType, Uuid)>,
) -> Result<Json<ReactionSummary>, ApiError> {
    state.features.require(Feature::Reactions)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    ensure_target_visible(&state, app_id, target_type, target_id, viewer_id).await?;

    let counts = sqlx::query_as::<_, ReactionCount>(
        "SELECT r.reaction_type, count(*)::BIGINT AS count FROM reactions r WHERE r.app_id = $1 AND r.target_type = $2 AND r.target_id = $3 AND ($4 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = r.user_id AND mas.state IN ('suspended', 'banned'))) AND ($5 = FALSE OR $6 IS NULL OR NOT social_users_blocked($1, $6, r.user_id)) GROUP BY r.reaction_type ORDER BY r.reaction_type ASC",
    )
    .bind(app_id)
    .bind(target_type)
    .bind(target_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(viewer_id)
    .fetch_all(&state.pool)
    .await?;

    let current_user_reactions = if let Some(viewer_id) = viewer_id {
        sqlx::query_scalar::<_, ReactionType>(
            "SELECT r.reaction_type FROM reactions r WHERE r.app_id = $1 AND r.target_type = $2 AND r.target_id = $3 AND r.user_id = $4 AND ($5 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = r.user_id AND mas.state IN ('suspended', 'banned'))) ORDER BY r.reaction_type ASC",
        )
        .bind(app_id)
        .bind(target_type)
        .bind(target_id)
        .bind(viewer_id)
        .bind(state.features.is_enabled(Feature::Moderation))
        .fetch_all(&state.pool)
        .await?
    } else {
        Vec::new()
    };

    Ok(Json(ReactionSummary {
        counts,
        current_user_reactions,
    }))
}

pub async fn put_reaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id, reaction_type)): Path<(ReactionTargetType, Uuid, ReactionType)>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Reactions)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_actor_active(&state, context.app_id.0, context.user_id.0).await?;
    let target_author_id = ensure_target_visible(
        &state,
        context.app_id.0,
        target_type,
        target_id,
        Some(context.user_id.0),
    )
    .await?;

    let (post_id, comment_id) = match target_type {
        ReactionTargetType::Post => (Some(target_id), None),
        ReactionTargetType::Comment => (None, Some(target_id)),
    };
    let mut transaction = state.pool.begin().await?;
    if state.features.is_enabled(Feature::Blocks) && target_author_id != context.user_id.0 {
        lock_user_pair(
            &mut transaction,
            context.app_id.0,
            context.user_id.0,
            target_author_id,
        )
        .await?;
        if users_are_blocked_in_transaction(
            &mut transaction,
            context.app_id.0,
            context.user_id.0,
            target_author_id,
        )
        .await?
        {
            let resource = match target_type {
                ReactionTargetType::Post => "post",
                ReactionTargetType::Comment => "comment",
            };
            return Err(ApiError::NotFound(resource));
        }
    }
    sqlx::query(
        "INSERT INTO reactions (app_id, target_type, target_id, post_id, comment_id, user_id, reaction_type) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(target_type)
    .bind(target_id)
    .bind(post_id)
    .bind(comment_id)
    .bind(context.user_id.0)
    .bind(reaction_type)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_reaction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id, reaction_type)): Path<(ReactionTargetType, Uuid, ReactionType)>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Reactions)?;
    let context = RequestContext::from_headers(&headers)?;
    sqlx::query(
        "DELETE FROM reactions WHERE app_id = $1 AND target_type = $2 AND target_id = $3 AND user_id = $4 AND reaction_type = $5",
    )
    .bind(context.app_id.0)
    .bind(target_type)
    .bind(target_id)
    .bind(context.user_id.0)
    .bind(reaction_type)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_target_visible(
    state: &AppState,
    app_id: Uuid,
    target_type: ReactionTargetType,
    target_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    match target_type {
        ReactionTargetType::Post => ensure_post_visible(state, app_id, target_id, viewer_id).await,
        ReactionTargetType::Comment => {
            ensure_comment_visible(state, app_id, target_id, viewer_id).await
        }
    }
}
