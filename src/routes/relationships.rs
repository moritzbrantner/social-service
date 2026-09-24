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
    models::{LimitQuery, UserSafetyRelationship},
    relationships::{ensure_relationship_target, lock_user_pair, lock_users},
    state::AppState,
};

pub async fn block_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Blocks)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_relationship_target(&state, context.app_id.0, context.user_id.0, user_id).await?;

    let mut transaction = state.pool.begin().await?;
    lock_users(
        &mut transaction,
        context.app_id.0,
        &[context.user_id.0, user_id],
    )
    .await?;
    lock_user_pair(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
    )
    .await?;
    sqlx::query(
        "INSERT INTO user_blocks (app_id, blocker_id, blocked_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM follows WHERE app_id = $1 AND ((follower_id = $2 AND followed_id = $3) OR (follower_id = $3 AND followed_id = $2))",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM follow_requests WHERE app_id = $1 AND ((requester_id = $2 AND target_id = $3) OR (requester_id = $3 AND target_id = $2))",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM reactions r USING posts p WHERE r.app_id = $1 AND p.app_id = r.app_id AND p.id = r.post_id AND r.target_type = 'post' AND ((r.user_id = $2 AND p.author_id = $3) OR (r.user_id = $3 AND p.author_id = $2))",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM reactions r USING comments c WHERE r.app_id = $1 AND c.app_id = r.app_id AND c.id = r.comment_id AND r.target_type = 'comment' AND ((r.user_id = $2 AND c.author_id = $3) OR (r.user_id = $3 AND c.author_id = $2))",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM votes v USING posts p WHERE v.app_id = $1 AND p.app_id = v.app_id AND p.id = v.post_id AND v.target_type = 'post' AND ((v.user_id = $2 AND p.author_id = $3) OR (v.user_id = $3 AND p.author_id = $2))",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "DELETE FROM votes v USING comments c WHERE v.app_id = $1 AND c.app_id = v.app_id AND c.id = v.comment_id AND v.target_type = 'comment' AND ((v.user_id = $2 AND c.author_id = $3) OR (v.user_id = $3 AND c.author_id = $2))",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn unblock_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Blocks)?;
    let context = RequestContext::from_headers(&headers)?;
    let mut transaction = state.pool.begin().await?;
    lock_users(
        &mut transaction,
        context.app_id.0,
        &[context.user_id.0, user_id],
    )
    .await?;
    lock_user_pair(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        user_id,
    )
    .await?;
    sqlx::query(
        "DELETE FROM user_blocks WHERE app_id = $1 AND blocker_id = $2 AND blocked_id = $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_blocks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<UserSafetyRelationship>>, ApiError> {
    state.features.require(Feature::Blocks)?;
    let context = RequestContext::from_headers(&headers)?;
    let blocks = sqlx::query_as::<_, UserSafetyRelationship>(
        "SELECT blocked_id AS user_id, created_at FROM user_blocks WHERE app_id = $1 AND blocker_id = $2 ORDER BY created_at DESC, blocked_id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(blocks))
}

pub async fn mute_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Mutes)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_relationship_target(&state, context.app_id.0, context.user_id.0, user_id).await?;
    sqlx::query(
        "INSERT INTO user_mutes (app_id, muter_id, muted_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unmute_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Mutes)?;
    let context = RequestContext::from_headers(&headers)?;
    sqlx::query("DELETE FROM user_mutes WHERE app_id = $1 AND muter_id = $2 AND muted_id = $3")
        .bind(context.app_id.0)
        .bind(context.user_id.0)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_mutes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<UserSafetyRelationship>>, ApiError> {
    state.features.require(Feature::Mutes)?;
    let context = RequestContext::from_headers(&headers)?;
    let mutes = sqlx::query_as::<_, UserSafetyRelationship>(
        "SELECT muted_id AS user_id, created_at FROM user_mutes WHERE app_id = $1 AND muter_id = $2 ORDER BY created_at DESC, muted_id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(mutes))
}
