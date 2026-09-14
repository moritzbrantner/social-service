use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    auth::RequestContext,
    error::ApiError,
    features::Feature,
    models::{FollowRequest, LimitQuery},
    moderation::{RestrictionScope, ensure_account_visible, ensure_user_can},
    relationships::{lock_user_pair, users_are_blocked_in_transaction},
    state::AppState,
};

pub async fn request_follow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Follow).await?;
    ensure_counterpart(&state, context.app_id.0, context.user_id.0, user_id).await?;

    let mut transaction = state.pool.begin().await?;
    if state.features.is_enabled(Feature::Blocks) {
        lock_user_pair(
            &mut transaction,
            context.app_id.0,
            context.user_id.0,
            user_id,
        )
        .await?;
        if users_are_blocked_in_transaction(
            &mut transaction,
            context.app_id.0,
            context.user_id.0,
            user_id,
        )
        .await?
        {
            return Err(ApiError::NotFound("profile"));
        }
    }

    sqlx::query(
        "INSERT INTO follow_requests (app_id, requester_id, target_id) SELECT $1, $2, $3 WHERE NOT EXISTS (SELECT 1 FROM follows WHERE app_id = $1 AND follower_id = $2 AND followed_id = $3) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel_follow_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    sqlx::query(
        "DELETE FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn accept_follow_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Follow).await?;
    ensure_counterpart(&state, context.app_id.0, context.user_id.0, user_id).await?;

    let mut transaction = state.pool.begin().await?;
    if state.features.is_enabled(Feature::Blocks) {
        lock_user_pair(
            &mut transaction,
            context.app_id.0,
            context.user_id.0,
            user_id,
        )
        .await?;
        if users_are_blocked_in_transaction(
            &mut transaction,
            context.app_id.0,
            context.user_id.0,
            user_id,
        )
        .await?
        {
            return Err(ApiError::NotFound("follow request"));
        }
    }

    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3 RETURNING requester_id",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .fetch_optional(&mut *transaction)
    .await?;

    if deleted.is_none() {
        let already_following = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM follows WHERE app_id = $1 AND follower_id = $2 AND followed_id = $3)",
        )
        .bind(context.app_id.0)
        .bind(user_id)
        .bind(context.user_id.0)
        .fetch_one(&mut *transaction)
        .await?;
        if !already_following {
            return Err(ApiError::NotFound("follow request"));
        }
    }

    sqlx::query(
        "INSERT INTO follows (app_id, follower_id, followed_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn decline_follow_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    sqlx::query(
        "DELETE FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn incoming_follow_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<FollowRequest>>, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    let requests = sqlx::query_as::<_, FollowRequest>(
        "SELECT requester_id, target_id, created_at FROM follow_requests WHERE app_id = $1 AND target_id = $2 ORDER BY created_at DESC, requester_id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(requests))
}

pub async fn outgoing_follow_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<FollowRequest>>, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    let requests = sqlx::query_as::<_, FollowRequest>(
        "SELECT requester_id, target_id, created_at FROM follow_requests WHERE app_id = $1 AND requester_id = $2 ORDER BY created_at DESC, target_id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(requests))
}

async fn ensure_counterpart(
    state: &AppState,
    app_id: Uuid,
    actor_id: Uuid,
    counterpart_id: Uuid,
) -> Result<(), ApiError> {
    if actor_id == counterpart_id {
        return Err(ApiError::BadRequest(
            "users cannot request or approve a follow for themselves".to_owned(),
        ));
    }
    let profile_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM profiles WHERE app_id = $1 AND user_id IN ($2, $3)",
    )
    .bind(app_id)
    .bind(actor_id)
    .bind(counterpart_id)
    .fetch_one(&state.pool)
    .await?;
    if profile_count != 2 {
        return Err(ApiError::NotFound("profile"));
    }
    ensure_account_visible(state, app_id, counterpart_id).await
}
