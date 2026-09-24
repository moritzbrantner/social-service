use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::TargetType;
use crate::{
    error::ApiError,
    groups::{Group, GroupMember, GroupRow},
    models::{
        Comment, Conversation, ConversationRow, MediaAsset, Message, MessageRow, Post,
        PostAudience, PostRow, Profile,
    },
    visibility::Visibility,
};

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub(crate) enum TargetSnapshot {
    Profile(Profile),
    Post(Post),
    Comment(Comment),
    Media(MediaAsset),
    Group(Group),
    Conversation(Conversation),
    Message(Message),
}

pub(crate) async fn load_target_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    app_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<Option<TargetSnapshot>, ApiError> {
    let snapshot = match target_type {
        TargetType::Profile => {
            sqlx::query_as::<_, Profile>(
                "SELECT user_id, display_name, bio, avatar_media_id, visibility, created_at, updated_at, version FROM profiles WHERE app_id = $1 AND user_id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&mut **transaction)
            .await?
            .map(TargetSnapshot::Profile)
        }
        TargetType::Post => {
            let record = sqlx::query_as::<
                _,
                (
                    Uuid,
                    Uuid,
                    String,
                    Visibility,
                    PostAudience,
                    DateTime<Utc>,
                    DateTime<Utc>,
                    i64,
                ),
            >(
                "SELECT id, author_id, body, visibility, audience, created_at, updated_at, version FROM posts WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&mut **transaction)
            .await?;
            if let Some((
                id,
                author_id,
                body,
                visibility,
                audience,
                created_at,
                updated_at,
                version,
            )) = record
            {
                let media_ids = sqlx::query_scalar::<_, Uuid>(
                    "SELECT media_id FROM post_media WHERE app_id = $1 AND post_id = $2 ORDER BY position ASC",
                )
                .bind(app_id)
                .bind(target_id)
                .fetch_all(&mut **transaction)
                .await?;
                Some(TargetSnapshot::Post(Post {
                    row: PostRow {
                        id,
                        author_id,
                        body,
                        visibility,
                        created_at,
                        updated_at,
                        version,
                    },
                    audience,
                    media_ids,
                }))
            } else {
                None
            }
        }
        TargetType::Comment => sqlx::query_as::<_, Comment>(
            "SELECT id, post_id, parent_comment_id, author_id, body, deleted_at, created_at, updated_at, version FROM comments WHERE app_id = $1 AND id = $2",
        )
        .bind(app_id)
        .bind(target_id)
        .fetch_optional(&mut **transaction)
        .await?
        .map(TargetSnapshot::Comment),
        TargetType::Media => sqlx::query_as::<_, MediaAsset>(
            "SELECT id, owner_id, url, content_type, created_at, updated_at, version FROM media_assets WHERE app_id = $1 AND id = $2",
        )
        .bind(app_id)
        .bind(target_id)
        .fetch_optional(&mut **transaction)
        .await?
        .map(TargetSnapshot::Media),
        TargetType::Group => {
            let row = sqlx::query_as::<_, GroupRow>(
                "SELECT id, name, avatar_media_id, created_by, created_at, updated_at, version FROM groups WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&mut **transaction)
            .await?;
            if let Some(row) = row {
                let members = sqlx::query_as::<_, GroupMember>(
                    "SELECT user_id, role, joined_at, updated_at, version FROM group_members WHERE app_id = $1 AND group_id = $2 ORDER BY joined_at ASC, user_id ASC",
                )
                .bind(app_id)
                .bind(target_id)
                .fetch_all(&mut **transaction)
                .await?;
                let chat_conversation_id = sqlx::query_scalar::<_, Uuid>(
                    "SELECT conversation_id FROM group_conversations WHERE app_id = $1 AND group_id = $2",
                )
                .bind(app_id)
                .bind(target_id)
                .fetch_optional(&mut **transaction)
                .await?;
                Some(TargetSnapshot::Group(Group {
                    row,
                    members,
                    chat_conversation_id,
                }))
            } else {
                None
            }
        }
        TargetType::Conversation => {
            let row = sqlx::query_as::<_, ConversationRow>(
                "SELECT id, created_at, updated_at, version FROM conversations WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&mut **transaction)
            .await?;
            if let Some(row) = row {
                let member_ids = sqlx::query_scalar::<_, Uuid>(
                    "SELECT user_id FROM conversation_members WHERE app_id = $1 AND conversation_id = $2 ORDER BY joined_at ASC, user_id ASC",
                )
                .bind(app_id)
                .bind(target_id)
                .fetch_all(&mut **transaction)
                .await?;
                Some(TargetSnapshot::Conversation(Conversation { row, member_ids }))
            } else {
                None
            }
        }
        TargetType::Message => {
            let row = sqlx::query_as::<_, MessageRow>(
                "SELECT id, conversation_id, author_id, body, created_at, updated_at, version FROM messages WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&mut **transaction)
            .await?;
            if let Some(row) = row {
                let media_ids = sqlx::query_scalar::<_, Uuid>(
                    "SELECT media_id FROM message_media WHERE app_id = $1 AND message_id = $2 ORDER BY position ASC",
                )
                .bind(app_id)
                .bind(target_id)
                .fetch_all(&mut **transaction)
                .await?;
                Some(TargetSnapshot::Message(Message { row, media_ids }))
            } else {
                None
            }
        }
    };
    Ok(snapshot)
}

pub(crate) fn snapshot_value(snapshot: &TargetSnapshot) -> Result<Value, ApiError> {
    serde_json::to_value(snapshot).map_err(|_| ApiError::Internal)
}

pub(crate) fn snapshot_json(snapshot: &TargetSnapshot) -> Result<String, ApiError> {
    serde_json::to_string(snapshot).map_err(|_| ApiError::Internal)
}
