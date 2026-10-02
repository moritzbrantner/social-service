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
    locking::lock_user_pair,
    models::{VoteSummary, VoteTargetType, VoteValue},
    relationships::users_are_blocked_in_transaction,
    state::AppState,
};

use super::{
    feedback_policy::{ensure_actor_active, ensure_comment_visible},
    posts::ensure_post_visible,
};

pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id)): Path<(VoteTargetType, Uuid)>,
) -> Result<Json<VoteSummary>, ApiError> {
    state.features.require(Feature::Votes)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    ensure_target_visible(&state, app_id, target_type, target_id, viewer_id).await?;

    let (upvotes, downvotes, score) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT count(*) FILTER (WHERE v.vote_value = 'up')::BIGINT, count(*) FILTER (WHERE v.vote_value = 'down')::BIGINT, COALESCE(sum(CASE v.vote_value WHEN 'up' THEN 1 ELSE -1 END), 0)::BIGINT FROM votes v WHERE v.app_id = $1 AND v.target_type = $2 AND v.target_id = $3 AND ($4 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = v.user_id AND mas.state IN ('suspended', 'banned'))) AND ($5 = FALSE OR $6 IS NULL OR NOT social_users_blocked($1, $6, v.user_id))",
    )
    .bind(app_id)
    .bind(target_type)
    .bind(target_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(viewer_id)
    .fetch_one(&state.pool)
    .await?;

    let current_user_vote = if let Some(viewer_id) = viewer_id {
        sqlx::query_scalar::<_, VoteValue>(
            "SELECT v.vote_value FROM votes v WHERE v.app_id = $1 AND v.target_type = $2 AND v.target_id = $3 AND v.user_id = $4 AND ($5 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = v.user_id AND mas.state IN ('suspended', 'banned')))",
        )
        .bind(app_id)
        .bind(target_type)
        .bind(target_id)
        .bind(viewer_id)
        .bind(state.features.is_enabled(Feature::Moderation))
        .fetch_optional(&state.pool)
        .await?
    } else {
        None
    };

    Ok(Json(VoteSummary {
        upvotes,
        downvotes,
        score,
        current_user_vote,
    }))
}

pub async fn put_vote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id, vote_value)): Path<(VoteTargetType, Uuid, VoteValue)>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Votes)?;
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
        VoteTargetType::Post => (Some(target_id), None),
        VoteTargetType::Comment => (None, Some(target_id)),
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
                VoteTargetType::Post => "post",
                VoteTargetType::Comment => "comment",
            };
            return Err(ApiError::NotFound(resource));
        }
    }
    sqlx::query(
        "INSERT INTO votes (app_id, target_type, target_id, post_id, comment_id, user_id, vote_value) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (app_id, target_type, target_id, user_id) DO UPDATE SET vote_value = EXCLUDED.vote_value, updated_at = now() WHERE votes.vote_value IS DISTINCT FROM EXCLUDED.vote_value",
    )
    .bind(context.app_id.0)
    .bind(target_type)
    .bind(target_id)
    .bind(post_id)
    .bind(comment_id)
    .bind(context.user_id.0)
    .bind(vote_value)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_vote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id)): Path<(VoteTargetType, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Votes)?;
    let context = RequestContext::from_headers(&headers)?;
    sqlx::query(
        "DELETE FROM votes WHERE app_id = $1 AND target_type = $2 AND target_id = $3 AND user_id = $4",
    )
    .bind(context.app_id.0)
    .bind(target_type)
    .bind(target_id)
    .bind(context.user_id.0)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_target_visible(
    state: &AppState,
    app_id: Uuid,
    target_type: VoteTargetType,
    target_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    match target_type {
        VoteTargetType::Post => ensure_post_visible(state, app_id, target_id, viewer_id).await,
        VoteTargetType::Comment => {
            ensure_comment_visible(state, app_id, target_id, viewer_id).await
        }
    }
}
