use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    auth::{RequestContext, app_id, optional_user_id},
    error::ApiError,
    features::Feature,
    models::{
        Comment, CreateComment, CreatePost, FollowEdge, LimitQuery, Post, PostAudience, PostRow,
    },
    moderation::{
        RestrictionScope, TargetType, ensure_account_visible, ensure_content_visible,
        ensure_user_can,
    },
    relationships::{ensure_not_blocked, users_are_blocked_in_transaction},
    state::AppState,
    visibility::Visibility,
};

use super::profiles::ensure_profile_visible;

#[derive(FromRow)]
struct PostRecord {
    id: Uuid,
    author_id: Uuid,
    body: String,
    visibility: Visibility,
    audience: PostAudience,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

pub async fn create_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreatePost>,
) -> Result<Json<Post>, ApiError> {
    state.features.require(Feature::Posts)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Post).await?;
    validate_text(&input.body, 10_000, "body")?;
    let media_ids = unique_media_ids(input.media_ids)?;
    if !media_ids.is_empty() {
        state.features.require(Feature::Media)?;
    }
    let (audience, visibility) = resolve_post_audience(&state, input.visibility, input.audience)?;

    let mut transaction = state.pool.begin().await?;
    let record = sqlx::query_as::<_, PostRecord>(
        "INSERT INTO posts (id, app_id, author_id, body, visibility, audience) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, author_id, body, visibility, audience, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(input.body.trim())
    .bind(visibility)
    .bind(audience)
    .fetch_one(&mut *transaction)
    .await?;

    attach_media(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        record.id,
        &media_ids,
        "post_media",
        "post_id",
    )
    .await?;
    transaction.commit().await?;
    Ok(Json(post_from_record(record, media_ids)))
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
    ensure_user_can(&state, context, RestrictionScope::Post).await?;
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
    ensure_user_can(&state, context, RestrictionScope::Comment).await?;
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

pub async fn follow_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Follows)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Follow).await?;
    if user_id == context.user_id.0 {
        return Err(ApiError::BadRequest(
            "users cannot follow themselves".to_owned(),
        ));
    }
    ensure_profile_visible(&state, context.app_id.0, user_id, Some(context.user_id.0)).await?;

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
    sqlx::query("INSERT INTO follows (app_id, follower_id, followed_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(context.app_id.0)
        .bind(context.user_id.0)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unfollow_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Follows)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Follow).await?;
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
        "SELECT f.follower_id, f.followed_id, f.created_at FROM follows f JOIN profiles p ON p.app_id = f.app_id AND p.user_id = f.follower_id WHERE f.app_id = $1 AND f.followed_id = $2 AND (p.visibility = 'public' OR p.user_id = $3 OR $3 = $2) AND ($4 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = p.user_id AND mas.state IN ('suspended', 'banned')) AND NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'profile' AND mcs.target_id = p.user_id AND mcs.state <> 'active'))) AND ($5 = FALSE OR $3 IS NULL OR NOT social_users_blocked($1, $3, p.user_id)) ORDER BY f.created_at DESC, f.follower_id ASC LIMIT $6",
    )
    .bind(app_id)
    .bind(user_id)
    .bind(viewer_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(state.features.is_enabled(Feature::Blocks))
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
        "SELECT f.follower_id, f.followed_id, f.created_at FROM follows f JOIN profiles p ON p.app_id = f.app_id AND p.user_id = f.followed_id WHERE f.app_id = $1 AND f.follower_id = $2 AND (p.visibility = 'public' OR p.user_id = $3 OR $3 = $2) AND ($4 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = p.user_id AND mas.state IN ('suspended', 'banned')) AND NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'profile' AND mcs.target_id = p.user_id AND mcs.state <> 'active'))) AND ($5 = FALSE OR $3 IS NULL OR NOT social_users_blocked($1, $3, p.user_id)) ORDER BY f.created_at DESC, f.followed_id ASC LIMIT $6",
    )
    .bind(app_id)
    .bind(user_id)
    .bind(viewer_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(state.features.is_enabled(Feature::Blocks))
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
    let records = sqlx::query_as::<_, PostRecord>(
        "SELECT p.id, p.author_id, p.body, p.visibility, p.audience, p.created_at, p.updated_at, p.version FROM posts p WHERE p.app_id = $1 AND (p.author_id = $2 OR EXISTS (SELECT 1 FROM follows f WHERE f.app_id = $1 AND f.follower_id = $2 AND f.followed_id = p.author_id)) AND (p.author_id = $2 OR p.audience = 'public' OR ($3 = TRUE AND p.audience = 'approved_followers' AND EXISTS (SELECT 1 FROM follow_approvals fa WHERE fa.app_id = $1 AND fa.requester_id = $2 AND fa.target_id = p.author_id))) AND ($4 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'post' AND mcs.target_id = p.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = p.author_id AND mas.state IN ('suspended', 'banned')))) AND ($5 = FALSE OR NOT social_users_blocked($1, $2, p.author_id)) AND ($6 = FALSE OR NOT social_user_muted($1, $2, p.author_id)) ORDER BY p.created_at DESC, p.id ASC LIMIT $7",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(state.features.is_enabled(Feature::FollowRequests))
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(state.features.is_enabled(Feature::Mutes))
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let post_ids = records.iter().map(|record| record.id).collect::<Vec<_>>();
    let mut media_by_post =
        load_media_ids_batch(&state, context.app_id.0, "post_media", "post_id", &post_ids).await?;

    let mut posts = Vec::with_capacity(records.len());
    for record in records {
        let media_ids = media_by_post.remove(&record.id).unwrap_or_default();
        posts.push(post_from_record(record, media_ids));
    }
    Ok(Json(posts))
}

async fn load_post(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Post, ApiError> {
    let record = sqlx::query_as::<_, PostRecord>(
        "SELECT id, author_id, body, visibility, audience, created_at, updated_at, version FROM posts WHERE app_id = $1 AND id = $2",
    )
    .bind(app_id)
    .bind(post_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("post"))?;
    ensure_post_record_visible(state, app_id, post_id, &record, viewer_id).await?;
    let media_ids = load_media_ids(state, app_id, "post_media", "post_id", post_id).await?;
    Ok(post_from_record(record, media_ids))
}

pub(crate) async fn ensure_post_visible(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Uuid, ApiError> {
    let target = sqlx::query_as::<_, (Uuid, PostAudience)>(
        "SELECT author_id, audience FROM posts WHERE app_id = $1 AND id = $2",
    )
    .bind(app_id)
    .bind(post_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("post"))?;
    ensure_post_audience(state, app_id, target.0, target.1, viewer_id).await?;
    ensure_not_blocked(state, app_id, viewer_id, target.0, "post").await?;
    ensure_account_visible(state, app_id, target.0).await?;
    ensure_content_visible(state, app_id, TargetType::Post, post_id, "post").await?;
    Ok(target.0)
}

async fn ensure_post_record_visible(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    record: &PostRecord,
    viewer_id: Option<Uuid>,
) -> Result<(), ApiError> {
    ensure_post_audience(state, app_id, record.author_id, record.audience, viewer_id).await?;
    ensure_not_blocked(state, app_id, viewer_id, record.author_id, "post").await?;
    ensure_account_visible(state, app_id, record.author_id).await?;
    ensure_content_visible(state, app_id, TargetType::Post, post_id, "post").await
}

async fn ensure_post_audience(
    state: &AppState,
    app_id: Uuid,
    author_id: Uuid,
    audience: PostAudience,
    viewer_id: Option<Uuid>,
) -> Result<(), ApiError> {
    if viewer_id == Some(author_id) || audience == PostAudience::Public {
        return Ok(());
    }
    if audience == PostAudience::OwnerOnly || !state.features.is_enabled(Feature::FollowRequests) {
        return Err(ApiError::NotFound("post"));
    }
    let Some(viewer_id) = viewer_id else {
        return Err(ApiError::NotFound("post"));
    };
    let approved = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM follow_approvals WHERE app_id = $1 AND requester_id = $2 AND target_id = $3)",
    )
    .bind(app_id)
    .bind(viewer_id)
    .bind(author_id)
    .fetch_one(&state.pool)
    .await?;
    if approved {
        Ok(())
    } else {
        Err(ApiError::NotFound("post"))
    }
}

fn resolve_post_audience(
    state: &AppState,
    visibility: Option<Visibility>,
    audience: Option<PostAudience>,
) -> Result<(PostAudience, Visibility), ApiError> {
    let legacy_audience = visibility.map(|visibility| match visibility {
        Visibility::Public => PostAudience::Public,
        Visibility::Private => PostAudience::OwnerOnly,
    });
    let audience = audience.or(legacy_audience).unwrap_or_default();
    if audience == PostAudience::ApprovedFollowers {
        state.features.require(Feature::FollowRequests)?;
    }
    let projected_visibility = match audience {
        PostAudience::Public => Visibility::Public,
        PostAudience::OwnerOnly | PostAudience::ApprovedFollowers => Visibility::Private,
    };
    if visibility.is_some_and(|visibility| visibility != projected_visibility) {
        return Err(ApiError::BadRequest(
            "visibility conflicts with the requested post audience".to_owned(),
        ));
    }
    Ok((audience, projected_visibility))
}

fn post_from_record(record: PostRecord, media_ids: Vec<Uuid>) -> Post {
    Post {
        audience: record.audience,
        media_ids,
        row: PostRow {
            id: record.id,
            author_id: record.author_id,
            body: record.body,
            visibility: record.visibility,
            created_at: record.created_at,
            updated_at: record.updated_at,
            version: record.version,
        },
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
        "SELECT relation.media_id FROM {table} relation WHERE relation.app_id = $1 AND relation.{owner_column} = $2 AND ($3 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'media' AND mcs.target_id = relation.media_id AND mcs.state <> 'active')) ORDER BY relation.position ASC"
    );
    Ok(sqlx::query_scalar::<_, Uuid>(&sql)
        .bind(app_id)
        .bind(owner_id)
        .bind(state.features.is_enabled(Feature::Moderation))
        .fetch_all(&state.pool)
        .await?)
}

pub(crate) async fn load_media_ids_batch(
    state: &AppState,
    app_id: Uuid,
    table: &'static str,
    owner_column: &'static str,
    owner_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Uuid>>, ApiError> {
    if owner_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let sql = format!(
        "SELECT relation.{owner_column}, relation.media_id FROM {table} relation WHERE relation.app_id = $1 AND relation.{owner_column} = ANY($2) AND ($3 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'media' AND mcs.target_id = relation.media_id AND mcs.state <> 'active')) ORDER BY relation.{owner_column} ASC, relation.position ASC"
    );
    let rows = sqlx::query_as::<_, (Uuid, Uuid)>(&sql)
        .bind(app_id)
        .bind(owner_ids)
        .bind(state.features.is_enabled(Feature::Moderation))
        .fetch_all(&state.pool)
        .await?;

    let mut media_by_owner = HashMap::<Uuid, Vec<Uuid>>::with_capacity(owner_ids.len());
    for (owner_id, media_id) in rows {
        media_by_owner.entry(owner_id).or_default().push(media_id);
    }
    Ok(media_by_owner)
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
