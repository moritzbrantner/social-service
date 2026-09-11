use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use social_service::{app, features::FeatureSet, state::AppState};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn moderation_signals_are_scoped_idempotent_explainable_and_non_authoritative() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("PostgreSQL should be reachable");
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("migrations should apply");

    let state = AppState::new(
        pool,
        FeatureSet::from_csv("posts,moderation").expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let author_id = Uuid::new_v4();
    let adapter_id = Uuid::new_v4();

    let response = send(
        &state,
        Method::PUT,
        "/v1/profiles/me",
        app_id,
        author_id,
        None,
        Some(json!({ "displayName": "Author" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        author_id,
        None,
        Some(json!({ "body": "classifier target" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let post = json_body(response).await;
    let post_id = Uuid::parse_str(post["id"].as_str().expect("post id")).expect("UUID post id");

    let signal_input = json!({
        "targetType": "post",
        "targetId": post_id,
        "source": "text-safety",
        "kind": "spam",
        "severity": "critical",
        "confidence": 0.97,
        "model": "classifier",
        "modelVersion": "2026-09",
        "evidence": {
            "labels": ["spam"],
            "explanation": "repeated campaign pattern"
        },
        "idempotencyKey": "scan-1"
    });

    let response = send(
        &state,
        Method::POST,
        "/v1/moderation/signals",
        app_id,
        adapter_id,
        None,
        Some(signal_input.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let first = send(
        &state,
        Method::POST,
        "/v1/moderation/signals",
        app_id,
        adapter_id,
        Some("signals.write"),
        Some(signal_input.clone()),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first = json_body(first).await;
    assert_eq!(first["severity"], "critical");
    assert_eq!(first["source"], "text-safety");
    assert_eq!(first["evidence"]["labels"][0], "spam");

    let repeated = send(
        &state,
        Method::POST,
        "/v1/moderation/signals",
        app_id,
        adapter_id,
        Some("signals.write"),
        Some(signal_input.clone()),
    )
    .await;
    assert_eq!(repeated.status(), StatusCode::OK);
    let repeated = json_body(repeated).await;
    assert_eq!(repeated["id"], first["id"]);
    assert_eq!(repeated["observedAt"], first["observedAt"]);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}"),
        app_id,
        author_id,
        None,
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "even a critical signal must not silently become moderation state"
    );

    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/signals?minimumSeverity=high",
        app_id,
        adapter_id,
        Some("signals.write"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/moderation/signals?targetType=post&targetId={post_id}&minimumSeverity=high"),
        app_id,
        adapter_id,
        Some("signals.read"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let signals = json_body(response).await;
    let signals = signals.as_array().expect("signal array");
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0]["id"], first["id"]);

    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/signals?minimumSeverity=high",
        other_app_id,
        adapter_id,
        Some("signals.read"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await.as_array().expect("signals").len(),
        0
    );

    let conflicting = json!({
        "targetType": "post",
        "targetId": post_id,
        "source": "text-safety",
        "kind": "harassment",
        "severity": "critical",
        "confidence": 0.97,
        "model": "classifier",
        "modelVersion": "2026-09",
        "evidence": {
            "labels": ["spam"],
            "explanation": "repeated campaign pattern"
        },
        "idempotencyKey": "scan-1"
    });
    let response = send(
        &state,
        Method::POST,
        "/v1/moderation/signals",
        app_id,
        adapter_id,
        Some("signals.write"),
        Some(conflicting),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let invalid_confidence = json!({
        "targetType": "post",
        "targetId": post_id,
        "source": "text-safety",
        "kind": "spam",
        "severity": "high",
        "confidence": 1.1,
        "idempotencyKey": "scan-2"
    });
    let response = send(
        &state,
        Method::POST,
        "/v1/moderation/signals",
        app_id,
        adapter_id,
        Some("signals.write"),
        Some(invalid_confidence),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = send(
        &state,
        Method::POST,
        "/v1/reports",
        app_id,
        author_id,
        None,
        Some(json!({
            "targetType": "post",
            "targetId": post_id,
            "category": "spam",
            "idempotencyKey": "report-for-signal"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let report = json_body(response).await;
    let case_id = report["caseId"].as_str().expect("case id");

    let response = send(
        &state,
        Method::POST,
        "/v1/moderation/signals",
        app_id,
        adapter_id,
        Some("signals.write"),
        Some(json!({
            "targetType": "post",
            "targetId": post_id,
            "caseId": case_id,
            "source": "rate-abuse",
            "kind": "burst",
            "severity": "high",
            "confidence": 0.82,
            "evidence": { "windowSeconds": 60, "events": 48 },
            "idempotencyKey": "rate-1"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let signal_id =
        Uuid::parse_str(first["id"].as_str().expect("signal id")).expect("UUID signal id");
    let mutation_error =
        sqlx::query("UPDATE moderation_signals SET kind = 'changed' WHERE app_id = $1 AND id = $2")
            .bind(app_id)
            .bind(signal_id)
            .execute(&state.pool)
            .await
            .expect_err("signal evidence must be immutable");
    assert!(
        mutation_error
            .to_string()
            .contains("moderation signals are immutable")
    );
}

async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    app_id: Uuid,
    user_id: Uuid,
    capabilities: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-app-id", app_id.to_string())
        .header("x-user-id", user_id.to_string())
        .header("x-request-id", Uuid::new_v4().to_string());
    if let Some(capabilities) = capabilities {
        builder = builder.header("x-social-moderation-capabilities", capabilities);
    }
    let body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };

    app(state.clone())
        .oneshot(builder.body(body).expect("request should build"))
        .await
        .expect("router should respond")
}

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response should contain JSON")
}
