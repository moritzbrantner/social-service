use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{FromRow, Type};
use uuid::Uuid;

use crate::{
    error::ApiError,
    moderation::{Capability, TargetType, actor, correlation_id, target_exists},
    state::AppState,
};

const MAX_EVIDENCE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "moderation_signal_severity", rename_all = "lowercase")]
pub enum SignalSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSignal {
    target_type: TargetType,
    target_id: Uuid,
    case_id: Option<Uuid>,
    source: String,
    kind: String,
    severity: SignalSeverity,
    confidence: Option<f64>,
    model: Option<String>,
    model_version: Option<String>,
    evidence: Option<Value>,
    idempotency_key: String,
    observed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ModerationSignal {
    id: Uuid,
    case_id: Option<Uuid>,
    target_type: TargetType,
    target_id: Uuid,
    source: String,
    kind: String,
    severity: SignalSeverity,
    confidence: Option<f64>,
    model: Option<String>,
    model_version: Option<String>,
    evidence: Value,
    idempotency_key: String,
    observed_at: DateTime<Utc>,
    ingested_by: Uuid,
    correlation_id: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalQuery {
    target_type: Option<TargetType>,
    target_id: Option<Uuid>,
    case_id: Option<Uuid>,
    minimum_severity: Option<SignalSeverity>,
    source: Option<String>,
    limit: Option<i64>,
}

pub async fn create_signal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateSignal>,
) -> Result<Json<ModerationSignal>, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::SignalsWrite)?;

    let source = required_text(&input.source, "source", 120)?;
    let kind = required_text(&input.kind, "kind", 120)?;
    let idempotency_key = required_text(&input.idempotency_key, "idempotencyKey", 128)?;
    let model = optional_text(input.model.as_deref(), "model", 200)?;
    let model_version = optional_text(input.model_version.as_deref(), "modelVersion", 120)?;
    if input
        .confidence
        .is_some_and(|confidence| !(0.0..=1.0).contains(&confidence))
    {
        return Err(ApiError::BadRequest(
            "confidence must be between 0 and 1".to_owned(),
        ));
    }

    let evidence = input.evidence.unwrap_or_else(|| Value::Object(Map::new()));
    if !evidence.is_object() {
        return Err(ApiError::BadRequest(
            "evidence must be a JSON object".to_owned(),
        ));
    }
    if evidence.to_string().len() > MAX_EVIDENCE_BYTES {
        return Err(ApiError::BadRequest(format!(
            "evidence must be at most {MAX_EVIDENCE_BYTES} bytes"
        )));
    }

    let app_id = actor.context.app_id.0;
    if !target_exists(&state, app_id, input.target_type, input.target_id).await? {
        return Err(ApiError::NotFound("moderation target"));
    }
    ensure_case_matches(
        &state,
        app_id,
        input.case_id,
        input.target_type,
        input.target_id,
    )
    .await?;

    let observed_at_was_supplied = input.observed_at.is_some();
    let observed_at = input.observed_at.unwrap_or_else(Utc::now);
    let correlation = correlation_id(&headers)?;
    let signal_id = Uuid::new_v4();
    let inserted = sqlx::query_as::<_, ModerationSignal>(
        "INSERT INTO moderation_signals (id, app_id, case_id, target_type, target_id, source, kind, severity, confidence, model, model_version, evidence, idempotency_key, observed_at, ingested_by, correlation_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) ON CONFLICT (app_id, source, idempotency_key) DO NOTHING RETURNING id, case_id, target_type, target_id, source, kind, severity, confidence, model, model_version, evidence, idempotency_key, observed_at, ingested_by, correlation_id, created_at",
    )
    .bind(signal_id)
    .bind(app_id)
    .bind(input.case_id)
    .bind(input.target_type)
    .bind(input.target_id)
    .bind(source)
    .bind(kind)
    .bind(input.severity)
    .bind(input.confidence)
    .bind(model)
    .bind(model_version)
    .bind(&evidence)
    .bind(idempotency_key)
    .bind(observed_at)
    .bind(actor.context.user_id.0)
    .bind(correlation.as_deref())
    .fetch_optional(&state.pool)
    .await?;

    let signal = if let Some(signal) = inserted {
        signal
    } else {
        let existing = sqlx::query_as::<_, ModerationSignal>(
            "SELECT id, case_id, target_type, target_id, source, kind, severity, confidence, model, model_version, evidence, idempotency_key, observed_at, ingested_by, correlation_id, created_at FROM moderation_signals WHERE app_id = $1 AND source = $2 AND idempotency_key = $3",
        )
        .bind(app_id)
        .bind(source)
        .bind(idempotency_key)
        .fetch_one(&state.pool)
        .await?;

        if existing.case_id != input.case_id
            || existing.target_type != input.target_type
            || existing.target_id != input.target_id
            || existing.kind != kind
            || existing.severity != input.severity
            || existing.confidence != input.confidence
            || existing.model.as_deref() != model
            || existing.model_version.as_deref() != model_version
            || existing.evidence != evidence
            || (observed_at_was_supplied && existing.observed_at != observed_at)
        {
            return Err(ApiError::BadRequest(
                "idempotencyKey was already used for a different moderation signal".to_owned(),
            ));
        }
        existing
    };

    Ok(Json(signal))
}

pub async fn list_signals(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SignalQuery>,
) -> Result<Json<Vec<ModerationSignal>>, ApiError> {
    let actor = actor(&state, &headers).await?;
    actor.require(Capability::SignalsRead)?;

    if query.target_type.is_some() != query.target_id.is_some() {
        return Err(ApiError::BadRequest(
            "targetType and targetId must be provided together".to_owned(),
        ));
    }
    let source = optional_text(query.source.as_deref(), "source", 120)?;
    let signals = sqlx::query_as::<_, ModerationSignal>(
        "SELECT id, case_id, target_type, target_id, source, kind, severity, confidence, model, model_version, evidence, idempotency_key, observed_at, ingested_by, correlation_id, created_at FROM moderation_signals WHERE app_id = $1 AND ($2::moderation_target_type IS NULL OR target_type = $2) AND ($3::uuid IS NULL OR target_id = $3) AND ($4::uuid IS NULL OR case_id = $4) AND ($5::moderation_signal_severity IS NULL OR severity >= $5) AND ($6::text IS NULL OR source = $6) ORDER BY severity DESC, observed_at DESC, id DESC LIMIT $7",
    )
    .bind(actor.context.app_id.0)
    .bind(query.target_type)
    .bind(query.target_id)
    .bind(query.case_id)
    .bind(query.minimum_severity)
    .bind(source)
    .bind(query.limit.unwrap_or(50).clamp(1, 100))
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(signals))
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
    let case_target = sqlx::query_as::<_, (TargetType, Uuid)>(
        "SELECT target_type, target_id FROM moderation_cases WHERE app_id = $1 AND id = $2",
    )
    .bind(app_id)
    .bind(case_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound("moderation case"))?;
    if case_target != (target_type, target_id) {
        return Err(ApiError::BadRequest(
            "caseId does not match moderation signal target".to_owned(),
        ));
    }
    Ok(())
}

fn required_text<'a>(value: &'a str, field: &str, max: usize) -> Result<&'a str, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(ApiError::BadRequest(format!(
            "{field} must contain 1-{max} characters"
        )));
    }
    Ok(value)
}

fn optional_text<'a>(
    value: Option<&'a str>,
    field: &str,
    max: usize,
) -> Result<Option<&'a str>, ApiError> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    if value.is_some_and(|value| value.chars().count() > max) {
        return Err(ApiError::BadRequest(format!(
            "{field} must contain at most {max} characters"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{optional_text, required_text};

    #[test]
    fn trims_and_bounds_signal_identifiers() {
        assert_eq!(required_text("  spam  ", "kind", 10).expect("valid"), "spam");
        assert!(required_text("   ", "kind", 10).is_err());
        assert_eq!(optional_text(Some(" model "), "model", 10).expect("valid"), Some("model"));
        assert_eq!(optional_text(Some("  "), "model", 10).expect("valid"), None);
    }
}
