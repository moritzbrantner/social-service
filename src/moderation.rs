use std::collections::HashSet;

use axum::http::{HeaderMap, HeaderName};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction, Type};
use uuid::Uuid;

use crate::{auth::RequestContext, error::ApiError, features::Feature, state::AppState};

const CAPABILITIES: HeaderName = HeaderName::from_static("x-social-moderation-capabilities");
const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_target_type", rename_all = "lowercase")]
pub enum TargetType {
    Profile,
    Post,
    Comment,
    Media,
    Conversation,
    Message,
}

impl TargetType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Post => "post",
            Self::Comment => "comment",
            Self::Media => "media",
            Self::Conversation => "conversation",
            Self::Message => "message",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_content_state", rename_all = "lowercase")]
pub enum ContentState {
    Active,
    Hidden,
    Removed,
}

impl ContentState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Hidden => "hidden",
            Self::Removed => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_account_state", rename_all = "lowercase")]
pub enum AccountState {
    Active,
    Suspended,
    Banned,
}

impl AccountState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Banned => "banned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_case_state", rename_all = "lowercase")]
pub enum CaseState {
    Open,
    Investigating,
    Resolved,
    Dismissed,
}

impl CaseState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Investigating => "investigating",
            Self::Resolved => "resolved",
            Self::Dismissed => "dismissed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_role", rename_all = "lowercase")]
pub enum Role {
    Moderator,
    Admin,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Moderator => "moderator",
            Self::Admin => "admin",
        }
    }

    pub const fn capabilities(self) -> &'static [Capability] {
        match self {
            Self::Moderator => &[
                Capability::ReportsRead,
                Capability::ContentModerate,
                Capability::UsersRestrict,
                Capability::AuditRead,
            ],
            Self::Admin => &Capability::ALL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_restriction_scope", rename_all = "lowercase")]
pub enum RestrictionScope {
    Profile,
    Media,
    Post,
    Comment,
    Follow,
    Chat,
}

impl RestrictionScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Media => "media",
            Self::Post => "post",
            Self::Comment => "comment",
            Self::Follow => "follow",
            Self::Chat => "chat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    ReportsRead,
    ContentModerate,
    UsersRestrict,
    RolesManage,
    AuditRead,
}

impl Capability {
    pub const ALL: [Self; 5] = [
        Self::ReportsRead,
        Self::ContentModerate,
        Self::UsersRestrict,
        Self::RolesManage,
        Self::AuditRead,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReportsRead => "reports.read",
            Self::ContentModerate => "content.moderate",
            Self::UsersRestrict => "users.restrict",
            Self::RolesManage => "roles.manage",
            Self::AuditRead => "audit.read",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "reports.read" => Some(Self::ReportsRead),
            "content.moderate" => Some(Self::ContentModerate),
            "users.restrict" => Some(Self::UsersRestrict),
            "roles.manage" => Some(Self::RolesManage),
            "audit.read" => Some(Self::AuditRead),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct ModerationActor {
    pub context: RequestContext,
    pub role: Option<Role>,
    capabilities: HashSet<Capability>,
}

impl ModerationActor {
    pub fn require(&self, capability: Capability) -> Result<(), ApiError> {
        if self.capabilities.contains(&capability) {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }

    pub fn capability_names(&self) -> Vec<&'static str> {
        Capability::ALL
            .into_iter()
            .filter(|capability| self.capabilities.contains(capability))
            .map(Capability::as_str)
            .collect()
    }
}

pub async fn actor(state: &AppState, headers: &HeaderMap) -> Result<ModerationActor, ApiError> {
    state.features.require(Feature::Moderation)?;
    let context = RequestContext::from_headers(headers)?;
    let role = sqlx::query_scalar::<_, Role>(
        "SELECT role FROM moderation_role_bindings WHERE app_id = $1 AND user_id = $2",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .fetch_optional(&state.pool)
    .await?;

    let mut capabilities = trusted_capabilities(headers)?;
    if let Some(role) = role {
        capabilities.extend(role.capabilities());
    }

    Ok(ModerationActor {
        context,
        role,
        capabilities,
    })
}

pub async fn ensure_user_can(
    state: &AppState,
    context: RequestContext,
    scope: RestrictionScope,
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }

    let account_state = sqlx::query_scalar::<_, AccountState>(
        "SELECT state FROM moderation_account_states WHERE app_id = $1 AND user_id = $2",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .fetch_optional(&state.pool)
    .await?;
    if matches!(
        account_state,
        Some(AccountState::Suspended | AccountState::Banned)
    ) {
        return Err(ApiError::Forbidden);
    }

    let restricted = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_restrictions WHERE app_id = $1 AND user_id = $2 AND scope = $3)",
    )
    .bind(context.app_id.0)
    .bind(context.user_id.0)
    .bind(scope)
    .fetch_one(&state.pool)
    .await?;
    if restricted {
        Err(ApiError::Forbidden)
    } else {
        Ok(())
    }
}

pub async fn ensure_account_visible(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }

    let hidden = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_account_states WHERE app_id = $1 AND user_id = $2 AND state IN ('suspended', 'banned'))",
    )
    .bind(app_id)
    .bind(user_id)
    .fetch_one(&state.pool)
    .await?;
    if hidden {
        Err(ApiError::NotFound("profile"))
    } else {
        Ok(())
    }
}

pub async fn ensure_content_visible(
    state: &AppState,
    app_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
    resource_name: &'static str,
) -> Result<(), ApiError> {
    if !state.features.is_enabled(Feature::Moderation) {
        return Ok(());
    }

    let hidden = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_content_states WHERE app_id = $1 AND target_type = $2 AND target_id = $3 AND state <> 'active')",
    )
    .bind(app_id)
    .bind(target_type)
    .bind(target_id)
    .fetch_one(&state.pool)
    .await?;
    if hidden {
        Err(ApiError::NotFound(resource_name))
    } else {
        Ok(())
    }
}

pub async fn target_exists(
    state: &AppState,
    app_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<bool, ApiError> {
    let exists = match target_type {
        TargetType::Profile => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM profiles WHERE app_id = $1 AND user_id = $2)",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?
        }
        TargetType::Post => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM posts WHERE app_id = $1 AND id = $2)",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?
        }
        TargetType::Comment => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM comments WHERE app_id = $1 AND id = $2)",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?
        }
        TargetType::Media => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM media_assets WHERE app_id = $1 AND id = $2)",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?
        }
        TargetType::Conversation => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE app_id = $1 AND id = $2)",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?
        }
        TargetType::Message => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM messages WHERE app_id = $1 AND id = $2)",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_one(&state.pool)
            .await?
        }
    };
    Ok(exists)
}

pub fn correlation_id(headers: &HeaderMap) -> Result<Option<String>, ApiError> {
    let Some(value) = headers.get(&REQUEST_ID) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::BadRequest("invalid `x-request-id` header".to_owned()))?;
    if value.is_empty() || value.chars().count() > 128 {
        return Err(ApiError::BadRequest(
            "x-request-id must contain 1-128 characters".to_owned(),
        ));
    }
    Ok(Some(value.to_owned()))
}

#[allow(clippy::too_many_arguments)]
pub async fn append_audit(
    transaction: &mut Transaction<'_, Postgres>,
    actor: &ModerationActor,
    action: &'static str,
    target_kind: &'static str,
    target_id: Option<Uuid>,
    reason: Option<&str>,
    previous_state: Option<&str>,
    new_state: Option<&str>,
    case_id: Option<Uuid>,
    correlation_id: Option<&str>,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO moderation_audit_events (id, app_id, actor_id, action, target_kind, target_id, reason, previous_state, new_state, case_id, correlation_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(Uuid::new_v4())
    .bind(actor.context.app_id.0)
    .bind(actor.context.user_id.0)
    .bind(action)
    .bind(target_kind)
    .bind(target_id)
    .bind(reason)
    .bind(previous_state)
    .bind(new_state)
    .bind(case_id)
    .bind(correlation_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub fn validate_reason(reason: Option<&str>) -> Result<Option<&str>, ApiError> {
    let reason = reason.map(str::trim).filter(|value| !value.is_empty());
    if reason.is_some_and(|value| value.chars().count() > 2000) {
        return Err(ApiError::BadRequest(
            "reason must contain at most 2000 characters".to_owned(),
        ));
    }
    Ok(reason)
}

fn trusted_capabilities(headers: &HeaderMap) -> Result<HashSet<Capability>, ApiError> {
    let Some(value) = headers.get(&CAPABILITIES) else {
        return Ok(HashSet::new());
    };
    let value = value.to_str().map_err(|_| {
        ApiError::BadRequest("invalid `x-social-moderation-capabilities` header".to_owned())
    })?;

    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            Capability::parse(value).ok_or_else(|| {
                ApiError::BadRequest(format!("unknown moderation capability `{value}`"))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{Capability, Role, trusted_capabilities};

    #[test]
    fn moderator_role_has_no_role_management_capability() {
        assert!(
            Role::Moderator
                .capabilities()
                .contains(&Capability::ReportsRead)
        );
        assert!(
            !Role::Moderator
                .capabilities()
                .contains(&Capability::RolesManage)
        );
        assert!(
            Role::Admin
                .capabilities()
                .contains(&Capability::RolesManage)
        );
    }

    #[test]
    fn trusted_capability_claims_are_fail_closed() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-social-moderation-capabilities",
            HeaderValue::from_static("reports.read,unknown"),
        );
        let error = trusted_capabilities(&headers).expect_err("unknown claim must fail");
        assert_eq!(
            error.to_string(),
            "bad request: unknown moderation capability `unknown`"
        );
    }
}
