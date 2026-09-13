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
    moderation::{TargetType, ensure_account_visible, ensure_content_visible},
    relationships::ensure_not_blocked,
    state::AppState,
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
        "SELECT r.reaction_type, count(*)::BIGINT AS count FROM reactions r WHERE r.app_id = $1 AND r.target_type = $2 AND r.target_id = $3 AND ($4 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = r.user_id AND mas.state IN ('suspended', 'banned'))) GROUP BY r.reaction_type ORDER BY r.reaction_type ASC",
    )
    .bind(app_id)
    .bind(target_type)
    .bind(target_id)
    .bind(state.features.is_enabled(Feature::Moderation))
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
    ensure_reaction_actor_active(&state, context.app_id.0, context.user_id.0).await?;
    ensure_target_visible(
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
    .execute(&state.pool)
    .await?;
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

async fn ensure_reaction_actor_active(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
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

async fn ensure_target_visible(
    state: &AppState,
    app_id: Uuid,
    target_type: ReactionTargetType,
    target_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<(), ApiError> {
    match target_type {
        ReactionTargetType::Post => {
            let author_id = sqlx::query_scalar::<_, Uuid>(
                "SELECT author_id FROM posts WHERE app_id = $1 AND id = $2 AND (visibility = 'public' OR author_id = $3)",
            )
            .bind(app_id)
            .bind(target_id)
            .bind(viewer_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("post"))?;
            ensure_not_blocked(state, app_id, viewer_id, author_id, "post").await?;
            ensure_account_visible(state, app_id, author_id).await?;
            ensure_content_visible(state, app_id, TargetType::Post, target_id, "post").await
        }
        ReactionTargetType::Comment => {
            state.features.require(Feature::Comments)?;
            let target = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
                "SELECT c.author_id, p.author_id, c.post_id FROM comments c JOIN posts p ON p.app_id = c.app_id AND p.id = c.post_id WHERE c.app_id = $1 AND c.id = $2 AND c.deleted_at IS NULL AND (p.visibility = 'public' OR p.author_id = $3)",
            )
            .bind(app_id)
            .bind(target_id)
            .bind(viewer_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("comment"))?;
            ensure_not_blocked(state, app_id, viewer_id, target.1, "comment").await?;
            ensure_not_blocked(state, app_id, viewer_id, target.0, "comment").await?;
            ensure_account_visible(state, app_id, target.1).await?;
            ensure_account_visible(state, app_id, target.0).await?;
            ensure_content_visible(state, app_id, TargetType::Post, target.2, "comment").await?;
            ensure_content_visible(state, app_id, TargetType::Comment, target_id, "comment").await
        }
    }
}
