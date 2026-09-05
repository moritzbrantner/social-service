use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    auth::RequestContext,
    error::ApiError,
    features::Feature,
    models::{Comment, ConversationRow, MediaAsset, MessageRow, PostRow, Profile},
    moderation::{
        AccountState, Capability, CaseState, ContentState, RestrictionScope, Role, TargetType,
        actor, append_audit, correlation_id, target_exists, validate_reason,
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReport {
    target_type: TargetType,
    target_id: Uuid,
    category: String,
    context: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ModerationReport {
    id: Uuid,
    case_id: Uuid,
    reporter_id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
    category: String,
    context: Option<String>,
    idempotency_key: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ModerationCase {
    id: Uuid,
    target_type: TargetType,
    target_id: Uuid,
    state: CaseState,
    opened_by: Uuid,
    resolution_note: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

#[derive(Debug, Deserialize)]
pub struct CaseQuery {
    state: Option<CaseState>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetCaseState {
    state: CaseState,
    resolution_note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetContentState {
    state: ContentState,
    reason: Option<String>,
    case_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountState {
    state: AccountState,
    reason: Option<String>,
    case_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRestriction {
    reason: Option<String>,
    case_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct SetRole {
    role: Role,
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModerationMe {
    user_id: Uuid,
    role: Option<Role>,
    effective_capabilities: Vec<&'static str>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RestrictionRecord {
    scope: RestrictionScope,
    reason: Option<String>,
    updated_at: DateTime<Utc>,
    version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserModeration {
    user_id: Uuid,
    state: AccountState,
    restrictions: Vec<RestrictionRecord>,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    id: Uuid,
    actor_id: Uuid,
    action: String,
    target_kind: String,
    target_id: Option<Uuid>,
    reason: Option<String>,
    previous_state: Option<String>,
    new_state: Option<String>,
    case_id: Option<Uuid>,
    correlation_id: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "lowercase")]
pub enum TargetSnapshot {
    Profile(Profile),
    Post(PostRow),
    Comment(Comment),
    Media(MediaAsset),
    Conversation(ConversationRow),
    Message(MessageRow),
}

pub async fn create_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateReport>,
) -> Result<Json<ModerationReport>, ApiError> {
    state.features.require(Feature::Moderation)?;
    let context = RequestContext::from_headers(&headers)?;
    let category = input.category.trim();
    if category.is_empty() || category.chars().count() > 80 {
        return Err(ApiError::BadRequest(
            "category must contain 1-80 characters".to_owned(),
        ));
    }
    let report_context = input
        .context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if report_context.is_some_and(|value| value.chars().count() > 2000) {
        return Err(ApiError::BadRequest(
            "context must contain at most 2000 characters".to_owned(),
        ));
    }
    let idempotency_key = input
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if idempotency_key.is_some_and(|value| value.chars().count() > 128) {
        return Err(ApiError::BadRequest(
            "idempotencyKey must contain at most 128 characters".to_owned(),
        ));
    }
    if !target_exists(&state, context.app_id.0, input.target_type, input.target_id).await? {
        return Err(ApiError::NotFound("moderation target"));
    }

    let case_id = Uuid::new_v4();
    let report_id = Uuid::new_v4();
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO moderation_cases (id, app_id, target_type, target_id, opened_by) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(case_id)
    .bind(context.app_id.0)
    .bind(input.target_type)
    .bind(input.target_id)
    .bind(context.user_id.0)
    .execute(&mut *transaction)
    .await?;

    let inserted = sqlx::query_as::<_, ModerationReport>(
        "INSERT INTO moderation_reports (id, app_id, case_id, reporter_id, target_type, target_id, category, context, idempotency_key) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (app_id, reporter_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING RETURNING id, case_id, reporter_id, target_type, target_id, category, context, idempotency_key, created_at",
    )
    .bind(report_id)
    .bind(context.app_id.0)
    .bind(case_id)
    .bind(context.user_id.0)
    .bind(input.target_type)
    .bind(input.target_id)
    .bind(category)
    .bind(report_context)
    .bind(idempotency_key)
    .fetch_optional(&mut *transaction)
    .await?;

    let report = if let Some(report) = inserted {
        report
    } else {
        let key = idempotency_key.ok_or_else(|| {
            ApiError::BadRequest("report idempotency conflict without a key".to_owned())
        })?;
        let existing = sqlx::query_as::<_, ModerationReport>(
            "SELECT id, case_id, reporter_id, target_type, target_id, category, context, idempotency_key, created_at FROM moderation_reports WHERE app_id = $1 AND reporter_id = $2 AND idempotency_key = $3",
        )
        .bind(context.app_id.0)
        .bind(context.user_id.0)
        .bind(key)
        .fetch_one(&mut *transaction)
        .await?;
        if existing.target_type != input.target_type
            || existing.target_id != input.target_id
            || existing.category != category
            || existing.context.as_deref() != report_context
        {
            return Err(ApiError::BadRequest(
                "idempotencyKey was already used for a different report".to_owned(),
            ));
        }
        sqlx::query("DELETE FROM moderation_cases WHERE app_id = $1 AND id = $2")
            .bind(context.app_id.0)
            .bind(case_id)
            .execute(&mut *transaction)
            .await?;
        existing
    };

    transaction.commit().await?;
    Ok(Json(report))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModerationMe>, ApiError> {
    let actor = actor(&state, &headers).await?;
    Ok(Json(ModerationMe {
        user_id: actor.context.user_id.0,
        role: actor.role,
        effective_capabilities: actor.capability_names(),
    }))
}

pub async fn list_cases(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CaseQuery>,
) -> Result<Json<Vec<ModerationCase>>, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::ReportsRead)?;
    let cases = sqlx::query_as::<_, ModerationCase>(
        "SELECT id, target_type, target_id, state, opened_by, resolution_note, created_at, updated_at, version FROM moderation_cases WHERE app_id = $1 AND ($2::moderation_case_state IS NULL OR state = $2) ORDER BY created_at ASC, id ASC LIMIT $3",
    )
    .bind(actor.context.app_id.0)
    .bind(query.state)
    .bind(query.limit.unwrap_or(50).clamp(1, 100))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(cases))
}

pub async fn review_target(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id)): Path<(TargetType, Uuid)>,
) -> Result<Json<TargetSnapshot>, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor
        .require(Capability::ReportsRead)
        .or_else(|_| actor.require(Capability::ContentModerate))?;
    let app_id = actor.context.app_id.0;
    let snapshot = match target_type {
        TargetType::Profile => TargetSnapshot::Profile(
            sqlx::query_as::<_, Profile>(
                "SELECT user_id, display_name, bio, avatar_media_id, visibility, created_at, updated_at, version FROM profiles WHERE app_id = $1 AND user_id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("moderation target"))?,
        ),
        TargetType::Post => TargetSnapshot::Post(
            sqlx::query_as::<_, PostRow>(
                "SELECT id, author_id, body, visibility, created_at, updated_at, version FROM posts WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("moderation target"))?,
        ),
        TargetType::Comment => TargetSnapshot::Comment(
            sqlx::query_as::<_, Comment>(
                "SELECT id, post_id, author_id, body, created_at, updated_at, version FROM comments WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("moderation target"))?,
        ),
        TargetType::Media => TargetSnapshot::Media(
            sqlx::query_as::<_, MediaAsset>(
                "SELECT id, owner_id, url, content_type, created_at, updated_at, version FROM media_assets WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("moderation target"))?,
        ),
        TargetType::Conversation => TargetSnapshot::Conversation(
            sqlx::query_as::<_, ConversationRow>(
                "SELECT id, created_at, updated_at, version FROM conversations WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("moderation target"))?,
        ),
        TargetType::Message => TargetSnapshot::Message(
            sqlx::query_as::<_, MessageRow>(
                "SELECT id, conversation_id, author_id, body, created_at, updated_at, version FROM messages WHERE app_id = $1 AND id = $2",
            )
            .bind(app_id)
            .bind(target_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound("moderation target"))?,
        ),
    };
    Ok(Json(snapshot))
}

pub async fn set_case_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(case_id): Path<Uuid>,
    Json(input): Json<SetCaseState>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::ContentModerate)?;
    let note = input
        .resolution_note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if note.is_some_and(|value| value.chars().count() > 4000) {
        return Err(ApiError::BadRequest(
            "resolutionNote must contain at most 4000 characters".to_owned(),
        ));
    }
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let current = sqlx::query_as::<_, (CaseState, Option<String>)>(
        "SELECT state, resolution_note FROM moderation_cases WHERE app_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(case_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(ApiError::NotFound("moderation case"))?;
    if current.0 == input.state && current.1.as_deref() == note {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query(
        "UPDATE moderation_cases SET state = $3, resolution_note = $4, updated_at = now(), version = version + 1 WHERE app_id = $1 AND id = $2",
    )
    .bind(actor.context.app_id.0)
    .bind(case_id)
    .bind(input.state)
    .bind(note)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        &actor,
        "case.state",
        "case",
        Some(case_id),
        note,
        Some(current.0.as_str()),
        Some(input.state.as_str()),
        Some(case_id),
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_content_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((target_type, target_id)): Path<(TargetType, Uuid)>,
    Json(input): Json<SetContentState>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::ContentModerate)?;
    let reason = validate_reason(input.reason.as_deref())?;
    if !target_exists(&state, actor.context.app_id.0, target_type, target_id).await? {
        return Err(ApiError::NotFound("moderation target"));
    }
    ensure_case_matches(
        &state,
        actor.context.app_id.0,
        input.case_id,
        target_type,
        target_id,
    )
    .await?;
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query_scalar::<_, ContentState>(
        "SELECT state FROM moderation_content_states WHERE app_id = $1 AND target_type = $2 AND target_id = $3 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(target_type)
    .bind(target_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let effective_previous = previous.unwrap_or(ContentState::Active);
    if effective_previous == input.state {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query(
        "INSERT INTO moderation_content_states (app_id, target_type, target_id, state, reason, updated_by) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (app_id, target_type, target_id) DO UPDATE SET state = EXCLUDED.state, reason = EXCLUDED.reason, updated_by = EXCLUDED.updated_by, updated_at = now(), version = moderation_content_states.version + 1",
    )
    .bind(actor.context.app_id.0)
    .bind(target_type)
    .bind(target_id)
    .bind(input.state)
    .bind(reason)
    .bind(actor.context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        &actor,
        "content.state",
        target_type.as_str(),
        Some(target_id),
        reason,
        Some(effective_previous.as_str()),
        Some(input.state.as_str()),
        input.case_id,
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_user_moderation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<Json<UserModeration>, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::UsersRestrict)?;
    let state_value = sqlx::query_scalar::<_, AccountState>(
        "SELECT state FROM moderation_account_states WHERE app_id = $1 AND user_id = $2",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .unwrap_or(AccountState::Active);
    let restrictions = sqlx::query_as::<_, RestrictionRecord>(
        "SELECT scope, reason, updated_at, version FROM moderation_restrictions WHERE app_id = $1 AND user_id = $2 ORDER BY scope::text ASC",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(UserModeration {
        user_id,
        state: state_value,
        restrictions,
    }))
}

pub async fn set_account_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(input): Json<SetAccountState>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::UsersRestrict)?;
    let reason = validate_reason(input.reason.as_deref())?;
    ensure_case_matches(
        &state,
        actor.context.app_id.0,
        input.case_id,
        TargetType::Profile,
        user_id,
    )
    .await?;
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query_scalar::<_, AccountState>(
        "SELECT state FROM moderation_account_states WHERE app_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let effective_previous = previous.unwrap_or(AccountState::Active);
    if effective_previous == input.state {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query(
        "INSERT INTO moderation_account_states (app_id, user_id, state, reason, updated_by) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (app_id, user_id) DO UPDATE SET state = EXCLUDED.state, reason = EXCLUDED.reason, updated_by = EXCLUDED.updated_by, updated_at = now(), version = moderation_account_states.version + 1",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .bind(input.state)
    .bind(reason)
    .bind(actor.context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        &actor,
        "account.state",
        "user",
        Some(user_id),
        reason,
        Some(effective_previous.as_str()),
        Some(input.state.as_str()),
        input.case_id,
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_restriction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((user_id, scope)): Path<(Uuid, RestrictionScope)>,
    Json(input): Json<SetRestriction>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::UsersRestrict)?;
    let reason = validate_reason(input.reason.as_deref())?;
    ensure_case_matches(
        &state,
        actor.context.app_id.0,
        input.case_id,
        TargetType::Profile,
        user_id,
    )
    .await?;
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query_scalar::<_, Option<String>>(
        "SELECT reason FROM moderation_restrictions WHERE app_id = $1 AND user_id = $2 AND scope = $3 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .bind(scope)
    .fetch_optional(&mut *transaction)
    .await?;
    if previous.as_ref().is_some_and(|value| value.as_deref() == reason) {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query(
        "INSERT INTO moderation_restrictions (app_id, user_id, scope, reason, updated_by) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (app_id, user_id, scope) DO UPDATE SET reason = EXCLUDED.reason, updated_by = EXCLUDED.updated_by, updated_at = now(), version = moderation_restrictions.version + 1",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .bind(scope)
    .bind(reason)
    .bind(actor.context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        &actor,
        "restriction.set",
        "user",
        Some(user_id),
        reason,
        previous.as_ref().map(|_| "restricted"),
        Some(scope.as_str()),
        input.case_id,
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_restriction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((user_id, scope)): Path<(Uuid, RestrictionScope)>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::UsersRestrict)?;
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query_scalar::<_, Option<String>>(
        "SELECT reason FROM moderation_restrictions WHERE app_id = $1 AND user_id = $2 AND scope = $3 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .bind(scope)
    .fetch_optional(&mut *transaction)
    .await?;
    if previous.is_none() {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query(
        "DELETE FROM moderation_restrictions WHERE app_id = $1 AND user_id = $2 AND scope = $3",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .bind(scope)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        &actor,
        "restriction.clear",
        "user",
        Some(user_id),
        None,
        Some(scope.as_str()),
        None,
        None,
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
    Json(input): Json<SetRole>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::RolesManage)?;
    let reason = validate_reason(input.reason.as_deref())?;
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query_scalar::<_, Role>(
        "SELECT role FROM moderation_role_bindings WHERE app_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if previous == Some(input.role) {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    sqlx::query(
        "INSERT INTO moderation_role_bindings (app_id, user_id, role, granted_by) VALUES ($1, $2, $3, $4) ON CONFLICT (app_id, user_id) DO UPDATE SET role = EXCLUDED.role, granted_by = EXCLUDED.granted_by, updated_at = now(), version = moderation_role_bindings.version + 1",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .bind(input.role)
    .bind(actor.context.user_id.0)
    .execute(&mut *transaction)
    .await?;
    append_audit(
        &mut transaction,
        &actor,
        "role.set",
        "user",
        Some(user_id),
        reason,
        previous.map(Role::as_str),
        Some(input.role.as_str()),
        None,
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::RolesManage)?;
    let correlation = correlation_id(&headers)?;
    let mut transaction = state.pool.begin().await?;
    let previous = sqlx::query_scalar::<_, Role>(
        "SELECT role FROM moderation_role_bindings WHERE app_id = $1 AND user_id = $2 FOR UPDATE",
    )
    .bind(actor.context.app_id.0)
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(previous) = previous else {
        transaction.commit().await?;
        return Ok(StatusCode::NO_CONTENT);
    };
    sqlx::query("DELETE FROM moderation_role_bindings WHERE app_id = $1 AND user_id = $2")
        .bind(actor.context.app_id.0)
        .bind(user_id)
        .execute(&mut *transaction)
        .await?;
    append_audit(
        &mut transaction,
        &actor,
        "role.clear",
        "user",
        Some(user_id),
        None,
        Some(previous.as_str()),
        None,
        None,
        correlation.as_deref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEvent>>, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::AuditRead)?;
    let events = sqlx::query_as::<_, AuditEvent>(
        "SELECT id, actor_id, action, target_kind, target_id, reason, previous_state, new_state, case_id, correlation_id, created_at FROM moderation_audit_events WHERE app_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(actor.context.app_id.0)
    .bind(query.limit.unwrap_or(100).clamp(1, 250))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(events))
}

async fn ensure_case_matches(
    state: &AppState,
    app_id: Uuid,
    case_id: Option<Uuid>,
    target_type: TargetType,
    target_id: Uuid,
) -> Result<(), ApiError> {
    let Some(case_id) = case_id else {
        return Ok(());
    };
    let matches = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM moderation_cases WHERE app_id = $1 AND id = $2 AND target_type = $3 AND target_id = $4)",
    )
    .bind(app_id)
    .bind(case_id)
    .bind(target_type)
    .bind(target_id)
    .fetch_one(&state.pool)
    .await?;
    if matches {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "caseId must reference a case for the same app and target".to_owned(),
        ))
    }
}
