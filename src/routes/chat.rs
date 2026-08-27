use std::collections::HashSet;

use axum::{Json, extract::{Path, Query, State}, http::HeaderMap};
use uuid::Uuid;

use crate::{auth::RequestContext, error::ApiError, features::Feature, models::{Conversation, CreateConversation, CreateMessage, LimitQuery, Message, MessageRow}, routes::posts::{attach_media, load_media_ids}, state::AppState};

pub async fn create_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateConversation>,
) -> Result<Json<Conversation>, ApiError> {
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    let mut members: HashSet<Uuid> = input.member_ids.into_iter().collect();
    members.insert(context.user_id.0);
    if members.len() < 2 || members.len() > 100 {
        return Err(ApiError::BadRequest("a conversation must contain 2-100 unique members".to_owned()));
    }

    let mut transaction = state.pool.begin().await?;
    let conversation = sqlx::query_as::<_, Conversation>(
        "INSERT INTO conversations (id, app_id) VALUES ($1, $2) RETURNING id, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .fetch_one(&mut *transaction)
    .await?;

    for user_id in members {
        sqlx::query("INSERT INTO conversation_members (app_id, conversation_id, user_id) VALUES ($1, $2, $3)")
            .bind(context.app_id.0)
            .bind(conversation.id)
            .bind(user_id)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    Ok(Json(conversation))
}

pub async fn list_conversations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    let conversations = sqlx::query_as::<_, Conversation>(
        "SELECT c.id, c.created_at, c.updated_at, c.version FROM conversations c JOIN conversation_members cm ON cm.app_id = c.app_id AND cm.conversation_id = c.id WHERE c.app_id = $1 AND cm.user_id = $2 ORDER BY c.updated_at DESC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(conversations))
}

pub async fn create_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
    Json(input): Json<CreateMessage>,
) -> Result<Json<Message>, ApiError> {
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    require_membership(&state, context.app_id.0, conversation_id, context.user_id.0).await?;

    let body = input.body.as_deref().map(str::trim).filter(|value| !value.is_empty());
    if body.is_none() && input.media_ids.is_empty() {
        return Err(ApiError::BadRequest("a message requires body text or media".to_owned()));
    }
    if body.is_some_and(|value| value.chars().count() > 10_000) {
        return Err(ApiError::BadRequest("message body must contain at most 10000 characters".to_owned()));
    }
    if input.media_ids.len() > 8 {
        return Err(ApiError::BadRequest("at most 8 media attachments are allowed".to_owned()));
    }
    let unique: HashSet<_> = input.media_ids.iter().copied().collect();
    if unique.len() != input.media_ids.len() {
        return Err(ApiError::BadRequest("mediaIds must not contain duplicates".to_owned()));
    }
    if !input.media_ids.is_empty() {
        state.features.require(Feature::Media)?;
    }

    let mut transaction = state.pool.begin().await?;
    let row = sqlx::query_as::<_, MessageRow>(
        "INSERT INTO messages (id, app_id, conversation_id, author_id, body) VALUES ($1, $2, $3, $4, $5) RETURNING id, conversation_id, author_id, body, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .bind(conversation_id)
    .bind(context.user_id.0)
    .bind(body)
    .fetch_one(&mut *transaction)
    .await?;

    attach_media(&mut transaction, context.app_id.0, context.user_id.0, row.id, &input.media_ids, "message_media", "message_id").await?;
    sqlx::query("UPDATE conversations SET updated_at = now(), version = version + 1 WHERE app_id = $1 AND id = $2")
        .bind(context.app_id.0)
        .bind(conversation_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Json(Message { row, media_ids: input.media_ids }))
}

pub async fn list_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Message>>, ApiError> {
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    require_membership(&state, context.app_id.0, conversation_id, context.user_id.0).await?;
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT id, conversation_id, author_id, body, created_at, updated_at, version FROM messages WHERE app_id = $1 AND conversation_id = $2 ORDER BY created_at DESC LIMIT $3",
    )
    .bind(context.app_id.0)
    .bind(conversation_id)
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let mut messages = Vec::with_capacity(rows.len());
    for row in rows {
        let media_ids = load_media_ids(&state, context.app_id.0, "message_media", "message_id", row.id).await?;
        messages.push(Message { row, media_ids });
    }
    Ok(Json(messages))
}

async fn require_membership(state: &AppState, app_id: Uuid, conversation_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    let is_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE app_id = $1 AND conversation_id = $2 AND user_id = $3)",
    )
    .bind(app_id)
    .bind(conversation_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if is_member { Ok(()) } else { Err(ApiError::Forbidden) }
}
