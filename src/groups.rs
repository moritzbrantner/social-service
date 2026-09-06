use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Type};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "group_role", rename_all = "lowercase")]
pub enum GroupRole {
    Owner,
    Admin,
    Member,
}

impl GroupRole {
    pub const fn can_manage_members(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    pub const fn can_manage_group(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GroupRow {
    pub id: Uuid,
    pub name: String,
    pub avatar_media_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub user_id: Uuid,
    pub role: GroupRole,
    pub joined_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    #[serde(flatten)]
    pub row: GroupRow,
    pub members: Vec<GroupMember>,
    pub chat_conversation_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroup {
    pub name: String,
    pub avatar_media_id: Option<Uuid>,
    #[serde(default)]
    pub member_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroup {
    pub name: String,
    pub avatar_media_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct SetGroupRole {
    pub role: GroupRole,
}
