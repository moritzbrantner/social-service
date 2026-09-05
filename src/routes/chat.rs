use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use uuid::Uuid;

use crate::{
    auth::RequestContext,
    error::ApiError,
    features::Feature,
    models::{
        Conversation, ConversationRow, CreateConversation, CreateMessage, LimitQuery, Message,
        MessageRow,
    },
    moderation::{RestrictionScope, TargetType, ensure_content_visible, ensure_user_can},
    routes::posts::{attach_media, load_media_ids},
    state::AppState,
};

pub async fn create_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateConversation>,
) -> Result<Json<Conversation>, ApiError> {
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    ensure_user_can(&state, context, RestrictionScope::Chat).await?;

    let mut seen = HashSet::new();
    let mut member_ids = Vec::with_capacity(input.member_ids.len() + 1);
    for user_id in std::iter::once(context.user_id.0).chain(input.member_ids) {
        if seen.insert(user_id) {
            member_ids.push(user_id);
        }
    }
    if member_ids.len() < 2 || member_ids.len() > 100 {
        return Err(ApiError::BadRequest(
            "a conversation must contain 2-100 unique members".to_owned(),
        ));
    }
    if state.features.is_enabled(Feature::Moderation) {
        let unavailable = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM moderation_account_states WHERE app_id = $1 AND user_id = ANY($2) AND state IN ('suspended', 'banned'))",
        )
        .bind(context.app_id.0)
        .bind(&member_ids)
        .fetch_one(&state.pool)
        .await?;
        if unavailable {
            return Err(ApiError::BadRequest(
                "conversation members must be available in this app".to_owned(),
            ));
        }
    }

    let mut transaction = state.pool.begin().await?;
    let row = sqlx::query_as::<_, ConversationRow>(
        "INSERT INTO conversations (id, app_id) VALUES ($1, $2) RETURNING id, created_at, updated_at, version",
    )
    .bind(Uuid::new_v4())
    .bind(context.app_id.0)
    .fetch_one(&mut *transaction)
    .await?;

    for user_id in &member_ids {
        sqlx::query(
            "INSERT INTO conversation_members (app_id, conversation_id, user_id) VALUES ($1, $2, $3)",
        )
        .bind(context.app_id.0)
        .bind(row.id)
        .bind(*user_id)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;

    Ok(Json(Conversation { row, member_ids }))
}

pub async fn list_conversations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<Conversation>>, ApiError> {
    state.features.require(Feature::Chat)?;
    let context = RequestContext::from_headers(&headers)?;
    let rows = sqlx::query_as::<_, ConversationRow>(
        "SELECT c.id, c.created_at, c.updated_at, c.version FROM conversations c JOIN conversation_members cm ON cm.app_id = c.app_id AND cm.conversation_id = c.id WHERE c.app_id = $1 AND cm.user_id = $2 AND ($3 = FALSE OR NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'conversation' AND mcs.target_id = c.id AND mcs.state <> 'active')) ORDER BY c.updated_at DESC LIMIT $4",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let mut conversations = Vec::with_capacity(rows.len());
    for row in rows {
        let member_ids = load_member_ids(&state, context.app_id.0, row.id).await?;
        conversations.push(Conversation { row, member_ids });
    }

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
    ensure_user_can(&state, context, RestrictionScope::Chat).await?;
    require_membership(&state, context.app_id.0, conversation_id, context.user_id.0).await?;
    ensure_content_visible(
        &state,
        context.app_id.0,
        TargetType::Conversation,
        conversation_id,
        "conversation",
    )
    .await?;

    let body = input
        .body
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if body.is_none() && input.media_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "a message requires body text or media".to_owned(),
        ));
    }
    if body.is_some_and(|value| value.chars().count() > 10_000) {
        return Err(ApiError::BadRequest(
            "message body must contain at most 10000 characters".to_owned(),
        ));
    }
    if input.media_ids.len() > 8 {
        return Err(ApiError::BadRequest(
            "at most 8 media attachments are allowed".to_owned(),
        ));
    }
    let unique: HashSet<_> = input.media_ids.iter().copied().collect();
    if unique.len() != input.media_ids.len() {
        return Err(ApiError::BadRequest(
            "mediaIds must not contain duplicates".to_owned(),
        ));
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

    attach_media(
        &mut transaction,
        context.app_id.0,
        context.user_id.0,
        row.id,
        &input.media_ids,
        "message_media",
        "message_id",
    )
    .await?;
    sqlx::query(
        "UPDATE conversations SET updated_at = now(), version = version + 1 WHERE app_id = $1 AND id = $2",
    )
    .bind(context.app_id.0)
    .bind(conversation_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Json(Message {
        row,
        media_ids: input.media_ids,
    }))
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
    ensure_content_visible(
        &state,
        context.app_id.0,
        TargetType::Conversation,
        conversation_id,
        "conversation",
    )
    .await?;
    let rows = sqlx::query_as::<_, MessageRow>(
        "SELECT m.id, m.conversation_id, m.author_id, m.body, m.created_at, m.updated_at, m.version FROM messages m WHERE m.app_id = $1 AND m.conversation_id = $2 AND ($3 = FALSE OR (NOT EXISTS (SELECT 1 FROM moderation_content_states mcs WHERE mcs.app_id = $1 AND mcs.target_type = 'message' AND mcs.target_id = m.id AND mcs.state <> 'active') AND NOT EXISTS (SELECT 1 FROM moderation_account_states mas WHERE mas.app_id = $1 AND mas.user_id = m.author_id AND mas.state IN ('suspended', 'banned')))) ORDER BY m.created_at DESC LIMIT $4",
    )
    .bind(context.app_id.0)
    .bind(conversation_id)
    .bind(state.features.is_enabled(Feature::Moderation))
    .bind(query.limit())
    .fetch_all(&state.pool)
    .await?;

    let mut messages = Vec::with_capacity(rows.len());
    for row in rows {
        let media_ids = load_media_ids(
            &state,
            context.app_id.0,
            "message_media",
            "message_id",
            row.id,
        )
        .await?;
        messages.push(Message { row, media_ids });
    }
    Ok(Json(messages))
}

async fn load_member_ids(
    state: &AppState,
    app_id: Uuid,
    conversation_id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM conversation_members WHERE app_id = $1 AND conversation_id = $2 ORDER BY joined_at ASC, user_id ASC",
    )
    .bind(app_id)
    .bind(conversation_id)
    .fetch_all(&state.pool)
    .await?)
}

async fn require_membership(
    state: &AppState,
    app_id: Uuid,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let is_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM conversation_members WHERE app_id = $1 AND conversation_id = $2 AND user_id = $3)",
    )
    .bind(app_id)
    .bind(conversation_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if is_member {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}
