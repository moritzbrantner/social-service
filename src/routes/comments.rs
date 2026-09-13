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
    models::{Comment, CreateReply, LimitQuery},
    moderation::{
        RestrictionScope, TargetType, ensure_account_visible, ensure_content_visible,
        ensure_user_can,
    },
    relationships::ensure_not_blocked,
    state::AppState,
};

pub async fn list_root_comments(
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
        "SELECT c.id, c.post_id, c.parent_comment_id, c.author_id, c.body, c.deleted_at, c.created_at, c.updated_at, c.version FROM comments c WHERE c.app_id = $1 AND c.post_id = $2 AND c.parent_comment_id IS NULL AND ($3 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'comment' AND mcs.target_id = c.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = c.author_id AND mas.state IN ('suspended', 'banned')))) AND ($5 = FALSE OR $4 IS NULL OR NOT social_users_blocked($1, $4, c.author_id)) AND ($6 = FALSE OR $4 IS NULL OR NOT social_user_muted($1, $4, c.author_id)) ORDER BY c.created_at ASC, c.id ASC LIMIT $7",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(viewer_id)
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(state.features.is_enabled(Feature::Mutes))
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(comments))
}

pub async fn list_replies(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((post_id, comment_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Comment>>, ApiError> {
    state.features.require(Feature::Comments)?;
    let app_id = app_id(&headers)?.0;
    let viewer_id = optional_user_id(&headers)?.map(|user_id| user_id.0);
    ensure_post_visible(&state, app_id, post_id, viewer_id).await?;
    ensure_comment_visible(&state, app_id, post_id, comment_id, viewer_id).await?;

    let comments = sqlx::query_as::<_, Comment>(
        "SELECT c.id, c.post_id, c.parent_comment_id, c.author_id, c.body, c.deleted_at, c.created_at, c.updated_at, c.version FROM comments c WHERE c.app_id = $1 AND c.post_id = $2 AND c.parent_comment_id = $3 AND ($4 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'comment' AND mcs.target_id = c.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = c.author_id AND mas.state IN ('suspended', 'banned')))) AND ($6 = FALSE OR $5 IS NULL OR NOT social_users_blocked($1, $5, c.author_id)) AND ($7 = FALSE OR $5 IS NULL OR NOT social_user_muted($1, $5, c.author_id)) ORDER BY c.created_at ASC, c.id ASC LIMIT $8",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(comment_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(viewer_id)
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(state.features.is_enabled(Feature::Mutes))
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(comments))
}

pub async fn create_reply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((post_id, comment_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<CreateReply>,
) -> Result<Json<Comment>, ApiError> {
    state.features.require(Feature::Comments)?;
    validate_text(&input.body, 5_000, "body")?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Comment).await?;
    ensure_post_visible(&state, context.app_id.0, post_id, Some(context.user_id.0)).await?;

    let mut transaction = state.pool.begin().await?;
    let parent = sqlx::query_scalar::<_, Uuid>(
        "SELECT c.id FROM comments c WHERE c.app_id = $1 AND c.post_id = $2 AND c.id = $3 AND c.deleted_at IS NULL AND ($4 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'comment' AND mcs.target_id = c.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = c.author_id AND mas.state IN ('suspended', 'banned')))) AND ($6 = FALSE OR NOT social_users_blocked($1, $5, c.author_id)) AND ($7 = FALSE OR NOT social_user_muted($1, $5, c.author_id)) FOR UPDATE",
    )
    .bind(context.app_id.0)
    .bind(post_id)
    .bind(comment_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(context.user_id.0)
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(state.features.is_enabled(Feature::Mutes))
    .fetch_optional(&mut *transaction)
    .await?;
    if parent.is_none() {
        return Err(ApiError::NotFound("comment"));
    }

    let comment = sqlx::query_as::<_, Comment>(
        "INSERT INTO comments (id, app_id, post_id, parent_comment_id, author_id, body) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, post_id, parent_comment_id, author_id, body, deleted_at, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .bind(post_id)
    .bind(comment_id)
    .bind(context.user_id.0)
    .bind(input.body.trim())
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(comment))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((post_id, comment_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    state.features.require(Feature::Comments)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Comment).await?;

    let mut transaction = state.pool.begin().await?;
    let deleted = sqlx::query_scalar::<_, bool>(
        "SELECT deleted_at IS NOT NULL FROM comments WHERE app_id = $1 AND post_id = $2 AND id = $3 AND author_id = $4 FOR UPDATE",
    )
    .bind(context.app_id.0)
    .bind(post_id)
    .bind(comment_id)
    .bind(context.user_id.0)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(ApiError::NotFound("comment"))?;

    if deleted {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let has_children = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM comments WHERE app_id = $1 AND post_id = $2 AND parent_comment_id = $3)",
    )
    .bind(context.app_id.0)
    .bind(post_id)
    .bind(comment_id)
    .fetch_one(&mut *transaction)
    .await?;

    if has_children {
        sqlx::query(
            "UPDATE comments SET body = '', deleted_at = now(), updated_at = now(), version = version + 1 WHERE app_id = $1 AND post_id = $2 AND id = $3",
        )
        .bind(context.app_id.0)
        .bind(post_id)
        .bind(comment_id)
        .execute(&mut *transaction)
        .await?;
    } else {
        sqlx::query("DELETE FROM comments WHERE app_id = $1 AND post_id = $2 AND id = $3")
            .bind(context.app_id.0)
            .bind(post_id)
            .bind(comment_id)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_post_visible(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let author_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT author_id FROM posts WHERE app_id = $1 AND id = $2 AND (visibility = 'public' OR author_id = $3)",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(viewer_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("post"))?;
    ensure_not_blocked(state, app_id, viewer_id, author_id, "post").await?;
    ensure_account_visible(state, app_id, author_id).await?;
    ensure_content_visible(state, app_id, TargetType::Post, post_id, "post").await
}

async fn ensure_comment_visible(
    state: &AppState,
    app_id: Uuid,
    post_id: Uuid,
    comment_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<(), ApiError> {
    let visible = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM comments c WHERE c.app_id = $1 AND c.post_id = $2 AND c.id = $3 AND ($4 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'comment' AND mcs.target_id = c.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = c.author_id AND mas.state IN ('suspended', 'banned')))) AND ($6 = FALSE OR $5 IS NULL OR NOT social_users_blocked($1, $5, c.author_id)) AND ($7 = FALSE OR $5 IS NULL OR NOT social_user_muted($1, $5, c.author_id)))",
    )
    .bind(app_id)
    .bind(post_id)
    .bind(comment_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(viewer_id)
    .bind(state.features.is_enabled(Feature::Blocks))
    .bind(state.features.is_enabled(Feature::Mutes))
    .fetch_one(&state.pool)
    .await?;

    if visible {
        Ok(())
    } else {
        Err(ApiError::NotFound("comment"))
    }
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
