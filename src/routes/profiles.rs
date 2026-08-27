use axum::{Json, extract::{Path, State}, http::HeaderMap};
use uuid::Uuid;

use crate::{auth::{RequestContext, app_id}, error::ApiError, features::Feature, models::{Profile, UpsertProfile}, state::AppState};

pub async fn get_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Profile>, ApiError> {
    state.features.require(Feature::Profiles)?;
    let app_id = app_id(&headers)?.0;
    let profile = sqlx::query_as::<_, Profile>(
        "SELECT user_id, display_name, bio, avatar_media_id, created_at, updated_at, version FROM profiles WHERE app_id = $1 AND user_id = $2",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("profile"))?;
    Ok(Json(profile))
}

pub async fn upsert_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpsertProfile>,
) -> Result<Json<Profile>, ApiError> {
    state.features.require(Feature::Profiles)?;
    validate_profile(&input)?;
    let context = RequestContext::from_headers(&headers)?;

    if let Some(media_id) = input.avatar_media_id {
        state.features.require(Feature::Media)?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM media_assets WHERE app_id = $1 AND id = $2 AND owner_id = $3)",
        )
        .bind(context.app_id.0)
        .bind(media_id)
        .bind(context.user_id.0)
        .fetch_one(&state.pool)
        .await?;
        if !exists {
            return Err(ApiError::BadRequest("avatar media must belong to the current user and app".to_owned()));
        }
    }

    let profile = sqlx::query_as::<_, Profile>(
        "INSERT INTO profiles (app_id, user_id, display_name, bio, avatar_media_id) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (app_id, user_id) DO UPDATE SET display_name = EXCLUDED.display_name, bio = EXCLUDED.bio, avatar_media_id = EXCLUDED.avatar_media_id, updated_at = now(), version = profiles.version + 1 RETURNING user_id, display_name, bio, avatar_media_id, created_at, updated_at, version",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(input.display_name.trim())
    .bind(input.bio.as_deref().map(str::trim).filter(|value| !value.is_empty()))
    .bind(input.avatar_media_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(profile))
}

fn validate_profile(input: &UpsertProfile) -> Result<(), ApiError> {
    let name_len = input.display_name.trim().chars().count();
    if !(1..=120).contains(&name_len) {
        return Err(ApiError::BadRequest("displayName must contain 1-120 characters".to_owned()));
    }
    if input.bio.as_ref().is_some_and(|bio| bio.chars().count() > 2000) {
        return Err(ApiError::BadRequest("bio must contain at most 2000 characters".to_owned()));
    }
    Ok(())
}
