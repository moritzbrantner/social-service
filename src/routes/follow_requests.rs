use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    auth::{RequestContext, UserId},
    error::ApiError,
    features::Feature,
    models::{FollowApproval, FollowRequest, LimitQuery},
    moderation::{
        RestrictionScope, TargetType, ensure_account_visible, ensure_content_visible,
        ensure_user_can,
    },
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
    lock_pair_and_ensure_unblocked(
        &state,
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
        "profile",
    )
    .await?;
    sqlx::query(
        "INSERT INTO follow_requests (app_id, requester_id, target_id) SELECT $1, $2, $3 WHERE NOT EXISTS (SELECT 1 FROM follow_approvals WHERE app_id = $1 AND requester_id = $2 AND target_id = $3) ON CONFLICT DO NOTHING",
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
    let mut transaction = state.pool.begin().await?;
    lock_user_pair(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
    )
    .await?;
    sqlx::query(
        "DELETE FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
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
    ensure_user_can(
        &state,
        RequestContext {
            app_id: context.app_id,
            user_id: UserId(user_id),
        },
        RestrictionScope::Follow,
    )
    .await?;

    let mut transaction = state.pool.begin().await?;
    lock_pair_and_ensure_unblocked(
        &state,
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
        "follow request",
    )
    .await?;
    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3 RETURNING requester_id",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .fetch_optional(&mut *transaction)
    .await?;

    if deleted.is_none() {
        let already_approved = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM follow_approvals WHERE app_id = $1 AND requester_id = $2 AND target_id = $3)",
        )
        .bind(context.app_id.0)
        .bind(user_id)
        .bind(context.user_id.0)
        .fetch_one(&mut *transaction)
        .await?;
        if !already_approved {
            return Err(ApiError::NotFound("follow request"));
        }
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    sqlx::query(
        "INSERT INTO follows (app_id, follower_id, followed_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO follow_approvals (app_id, requester_id, target_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
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
    let mut transaction = state.pool.begin().await?;
    lock_user_pair(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
    )
    .await?;
    sqlx::query(
        "DELETE FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
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

pub async fn approved_followers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<FollowApproval>>, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    let approvals = sqlx::query_as::<_, FollowApproval>(
        "SELECT requester_id, target_id, approved_at FROM follow_approvals WHERE app_id = $1 AND target_id = $2 ORDER BY approved_at DESC, requester_id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(approvals))
}

pub async fn revoke_follow_approval(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::FollowRequests)?;
    let context = RequestContext::from_headers(&headers)?;
    let mut transaction = state.pool.begin().await?;
    lock_user_pair(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
    )
    .await?;
    sqlx::query(
        "DELETE FROM follows f WHERE f.app_id = $1 AND f.follower_id = $2 AND f.followed_id = $3 AND EXISTS (SELECT 1 FROM follow_approvals a WHERE a.app_id = f.app_id AND a.requester_id = f.follower_id AND a.target_id = f.followed_id)",
    )
    .bind(context.app_id.0)
    .bind(user_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
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
    ensure_account_visible(state, app_id, counterpart_id).await?;
    ensure_content_visible(
        state,
        app_id,
        TargetType::Profile,
        counterpart_id,
        "profile",
    )
    .await
}

async fn lock_pair_and_ensure_unblocked(
    state: &AppState,
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    left_id: Uuid,
    right_id: Uuid,
    resource_name: &'static str,
) -> Result<(), ApiError> {
    lock_user_pair(transaction, app_id, left_id, right_id).await?;
    if state.features.is_enabled(Feature::Blocks)
        && users_are_blocked_in_transaction(transaction, app_id, left_id, right_id).await?
    {
        Err(ApiError::NotFound(resource_name))
    } else {
        Ok(())
    }
}
