use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    auth::RequestContext,
    error::ApiError,
    features::Feature,
    models::{LimitQuery, Post, PostRow, SavedPost},
    moderation::{TargetType, ensure_account_visible, ensure_content_visible},
    state::AppState,
    visibility::Visibility,
};

use super::posts::load_media_ids;

#[derive(FromRow)]
struct SavedPostRecord {
    id: Uuid,
    author_id: Uuid,
    body: String,
    visibility: Visibility,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
    saved_at: DateTime<Utc>,
}

pub async fn save_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Saves)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_saveable_post(&state, context.app_id.0, post_id, context.user_id.0).await?;

    sqlx::query(
        "INSERT INTO post_saves (app_id, user_id, post_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(post_id)
    .execute(&state.pool)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn unsave_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Saves)?;
    let context = RequestContext::from_headers(&headers)?;

    sqlx::query("DELETE FROM post_saves WHERE app_id = $1 AND user_id = $2 AND post_id = $3")
        .bind(context.app_id.0)
        .bind(context.user_id.0)
        .bind(post_id)
        .execute(&state.pool)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_saved_posts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<SavedPost>>, ApiError> {
    state.features.require(Feature::Saves)?;
    let context = RequestContext::from_headers(&headers)?;
    let rows = sqlx::query_as::<_, SavedPostRecord>(
        "SELECT p.id, p.author_id, p.body, p.visibility, p.created_at, p.updated_at, p.version, s.created_at AS saved_at FROM post_saves s JOIN posts p ON p.app_id = s.app_id AND p.id = s.post_id WHERE s.app_id = $1 AND s.user_id = $2 AND (p.visibility = 'public' OR p.author_id = $2) AND ($3 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'post' AND mcs.target_id = p.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = p.author_id AND mas.state IN ('suspended', 'banned')))) ORDER BY s.created_at DESC, p.id ASC LIMIT $4",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let mut saved_posts = Vec::with_capacity(rows.len());
    for row in rows {
        let media_ids =
            load_media_ids(&state, context.app_id.0, "post_media", "post_id", row.id).await?;
        saved_posts.push(SavedPost {
            post: Post {
                row: PostRow {
                    id: row.id,
                    author_id: row.author_id,
                    body: row.body,
                    visibility: row.visibility,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    version: row.version,
                },
                media_ids,
            },
            saved_at: row.saved_at,
        });
    }

    Ok(Json(saved_posts))
}

async fn ensure_saveable_post(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let author_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT author_id FROM posts WHERE app_id = $1 AND id = $2 AND (visibility = 'public' OR author_id = $3)",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("post"))?;

    ensure_account_visible(state, app_id, author_id).await?;
    ensure_content_visible(state, app_id, TargetType::Post, post_id, "post").await
}
