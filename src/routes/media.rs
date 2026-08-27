use axum::{Json, extract::State, http::HeaderMap};
use uuid::Uuid;

use crate::{auth::RequestContext, error::ApiError, features::Feature, models::{MediaAsset, RegisterMedia}, state::AppState};

pub async fn register_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RegisterMedia>,
) -> Result<Json<MediaAsset>, ApiError> {
    state.features.require(Feature::Media)?;
    let context = RequestContext::from_headers(&headers)?;
    let url = input.url.trim();
    let content_type = input.content_type.trim();
    if url.is_empty() || url.chars().count() > 4096 {
        return Err(ApiError::BadRequest("url must contain 1-4096 characters".to_owned()));
    }
    if content_type.is_empty() || content_type.chars().count() > 255 {
        return Err(ApiError::BadRequest("contentType must contain 1-255 characters".to_owned()));
    }

    let asset = sqlx::query_as::<_, MediaAsset>(
        "INSERT INTO media_assets (id, app_id, owner_id, url, content_type) VALUES ($1, $2, $3, $4, $5) RETURNING id, owner_id, url, content_type, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(url)
    .bind(content_type)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(asset))
}
