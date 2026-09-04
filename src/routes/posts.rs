use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    auth::{RequestContext, app_id, optional_user_id},
    error::ApiError,
    features::Feature,
    models::{Comment, CreateComment, CreatePost, FollowEdge, LimitQuery, Post, PostRow},
    state::AppState,
};

use super::profiles::ensure_profile_visible;

pub async fn create_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreatePost>,
) -> Result<Json<Post>, ApiError> {
    state.features.require(Feature::Posts)?;
    let context = RequestContext::from_headers(&headers)?;
    validate_text(&input.body, 10_000, "body")?;
    let media_ids = unique_media_ids(input.media_ids)?;
    if !media_ids.is_empty() {
        state.features.require(Feature::Media)?;
    }

    let mut transaction = state.pool.begin().await?;
    let row = sqlx::query_as::<_, PostRow>(
        "INSERT INTO posts (id, app_id, author_id, body, visibility) VALUES ($1, $2, $3, $4, COALESCE($5, 'public'::social_visibility)) RETURNING id, author_id, body, visibility, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(input.body.trim())
    .bind(input.visibility)
    .fetch_one(&mut *transaction)
    .await?;

    attach_media(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        row.id,
        &media_ids,
        "post_media",
        "post_id",
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(Post { row, media_ids }))
}

pub async fn get_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
) -> Result<Json<Post>, ApiError> {
    state.features.require(Feature::Posts)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    Ok(Json(load_post(&state, app_id, post_id, viewer_id).await?))
}

pub async fn delete_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Posts)?;
    let context = RequestContext::from_headers(&headers)?;
    let result = sqlx::query("DELETE FROM posts WHERE app_id = $1 AND id = $2 AND author_id = $3")
        .bind(context.app_id.0)
        .bind(post_id)
        .bind(context.user_id.0)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound("post"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn create_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
    Json(input): Json<CreateComment>,
) -> Result<Json<Comment>, ApiError> {
    state.features.require(Feature::Comments)?;
    validate_text(&input.body, 5_000, "body")?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_post_visible(&state, context.app_id.0, post_id, Some(context.user_id.0)).await?;

    let comment = sqlx::query_as::<_, Comment>(
        "INSERT INTO comments (id, app_id, post_id, author_id, body) VALUES ($1, $2, $3, $4, $5) RETURNING id, post_id, author_id, body, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .bind(post_id)
    .bind(context.user_id.0)
    .bind(input.body.trim())
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(comment))
}

pub async fn list_comments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(post_id): Path<Uuid>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Comment>>, ApiError> {
    state.features.require(Feature::Comments)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    ensure_post_visible(&state, app_id, post_id, viewer_id).await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, post_id, author_id, body, created_at, updated_at, version FROM comments WHERE app_id = $1 AND post_id = $2 ORDER BY created_at ASC, id ASC LIMIT $3",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(comments))
}

pub async fn follow_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Follows)?;
    let context = RequestContext::from_headers(&headers)?;
    if user_id == context.user_id.0 {
        return Err(ApiError::BadRequest(
            "users cannot follow themselves".to_owned(),
        ));
    }
    ensure_profile_visible(&state, context.app_id.0, user_id, Some(context.user_id.0)).await?;
    sqlx::query("INSERT INTO follows (app_id, follower_id, followed_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(context.app_id.0)
        .bind(context.user_id.0)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unfollow_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Follows)?;
    let context = RequestContext::from_headers(&headers)?;
    sqlx::query("DELETE FROM follows WHERE app_id = $1 AND follower_id = $2 AND followed_id = $3")
        .bind(context.app_id.0)
        .bind(context.user_id.0)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn followers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<FollowEdge>>, ApiError> {
    state.features.require(Feature::Follows)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    ensure_profile_visible(&state, app_id, user_id, viewer_id).await?;
    let follows = sqlx::query_as::<_, FollowEdge>(
        "SELECT follower_id, followed_id, created_at FROM follows WHERE app_id = $1 AND followed_id = $2 ORDER BY created_at DESC, follower_id ASC LIMIT $3",
    )
    .bind(app_id)
    .bind(user_id)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(follows))
}

pub async fn following(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<FollowEdge>>, ApiError> {
    state.features.require(Feature::Follows)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    ensure_profile_visible(&state, app_id, user_id, viewer_id).await?;
    let follows = sqlx::query_as::<_, FollowEdge>(
        "SELECT follower_id, followed_id, created_at FROM follows WHERE app_id = $1 AND follower_id = $2 ORDER BY created_at DESC, followed_id ASC LIMIT $3",
    )
    .bind(app_id)
    .bind(user_id)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(follows))
}

pub async fn timeline(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Post>>, ApiError> {
    state.features.require(Feature::Posts)?;
    state.features.require(Feature::Follows)?;
    let context = RequestContext::from_headers(&headers)?;
    let rows = sqlx::query_as::<_, PostRow>(
        "SELECT p.id, p.author_id, p.body, p.visibility, p.created_at, p.updated_at, p.version FROM posts p WHERE p.app_id = $1 AND (p.author_id = $2 OR EXISTS (SELECT 1 FROM follows f WHERE f.app_id = $1 AND f.follower_id = $2 AND f.followed_id = p.author_id)) AND (p.visibility = 'public' OR p.author_id = $2) ORDER BY p.created_at DESC, p.id ASC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let mut posts = Vec::with_capacity(rows.len());
    for row in rows {
        let media_ids =
            load_media_ids(&state, context.app_id.0, "post_media", "post_id", row.id).await?;
        posts.push(Post { row, media_ids });
    }
    Ok(Json(posts))
}

async fn load_post(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Post, ApiError> {
    let row = sqlx::query_as::<_, PostRow>(
        "SELECT id, author_id, body, visibility, created_at, updated_at, version FROM posts WHERE app_id = $1 AND id = $2 AND (visibility = 'public' OR author_id = $3)",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(viewer_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("post"))?;
    let media_ids = load_media_ids(state, app_id, "post_media", "post_id", post_id).await?;
    Ok(Post { row, media_ids })
}

async fn ensure_post_visible(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let visible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM posts WHERE app_id = $1 AND id = $2 AND (visibility = 'public' OR author_id = $3))",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(viewer_id)
    .fetch_one(&state.pool)
    .await?;
    if visible {
        Ok(())
    } else {
        Err(ApiError::NotFound("post"))
    }
}

pub(crate) async fn load_media_ids(
    state: &AppState,
    app_id: Uuid,
    table: &'static str,
    owner_column: &'static str,
    owner_id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    let sql = format!(
        "SELECT media_id FROM {table} WHERE app_id = $1 AND {owner_column} = $2 ORDER BY position ASC"
    );
    Ok(sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(app_id)
        .bind(owner_id)
        .fetch_all(&state.pool)
        .await?)
}

pub(crate) async fn attach_media(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app_id: Uuid,
    user_id: Uuid,
    owner_id: Uuid,
    media_ids: &[Uuid],
    table: &'static str,
    owner_column: &'static str,
) -> Result<(), ApiError> {
    for (position, media_id) in media_ids.iter().enumerate() {
        let position = i16::try_from(position)
            .map_err(|_| ApiError::BadRequest("too many media attachments".to_owned()))?;
        let sql = format!(
            "INSERT INTO {table} (app_id, {owner_column}, media_id, position) SELECT $1, $2, m.id, $3 FROM media_assets m WHERE m.app_id = $1 AND m.id = $4 AND m.owner_id = $5"
        );
        let result = sqlx::query(&sql)
            .bind(app_id)
            .bind(owner_id)
            .bind(position)
            .bind(media_id)
            .bind(user_id)
            .execute(&mut **transaction)
            .await?;
        if result.rows_affected() != 1 {
            return Err(ApiError::BadRequest(
                "media attachments must belong to the current user and app".to_owned(),
            ));
        }
    }
    Ok(())
}

fn unique_media_ids(media_ids: Vec<Uuid>) -> Result<Vec<Uuid>, ApiError> {
    if media_ids.len() > 8 {
        return Err(ApiError::BadRequest(
            "at most 8 media attachments are allowed".to_owned(),
        ));
    }
    let mut seen = HashSet::with_capacity(media_ids.len());
    if media_ids.iter().any(|id| !seen.insert(*id)) {
        return Err(ApiError::BadRequest(
            "mediaIds must not contain duplicates".to_owned(),
        ));
    }
    Ok(media_ids)
}

fn validate_text(value: &str, max: usize, field: &str) -> Result<(), ApiError> {
    let length = value.trim().chars().count();
    if !(1..=max).contains(&length) {
        return Err(ApiError::BadRequest(format!(
            "{field} must contain 1-{max} characters"
        )));
    }
    Ok(())
}
