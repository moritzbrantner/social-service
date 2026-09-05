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

const ADMIN_CAPABILITIES: &str =
    "reports.read,content.moderate,users.restrict,roles.manage,audit.read";

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn moderation_is_app_scoped_idempotent_and_enforced() {
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
        FeatureSet::from_csv("comments,follows,chat,moderation")
            .expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let admin_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let moderator_id = Uuid::new_v4();

    for (id, name) in [
        (admin_id, "Admin"),
        (user_id, "Reported user"),
        (moderator_id, "Moderator"),
    ] {
        let response = send(
            &state,
            Method::PUT,
            "/v1/profiles/me",
            app_id,
            id,
            None,
            Some(json!({ "displayName": name })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        user_id,
        None,
        Some(json!({ "body": "reported post" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let post = json_body(response).await;
    let post_id = Uuid::parse_str(post["id"].as_str().expect("post id")).expect("UUID post id");

    let report_input = json!({
        "targetType": "post",
        "targetId": post_id,
        "category": "spam",
        "context": "duplicate-looking content",
        "idempotencyKey": "report-1"
    });
    let first_report = send(
        &state,
        Method::POST,
        "/v1/reports",
        app_id,
        moderator_id,
        None,
        Some(report_input.clone()),
    )
    .await;
    assert_eq!(first_report.status(), StatusCode::OK);
    let first_report = json_body(first_report).await;
    let second_report = send(
        &state,
        Method::POST,
        "/v1/reports",
        app_id,
        moderator_id,
        None,
        Some(report_input),
    )
    .await;
    assert_eq!(second_report.status(), StatusCode::OK);
    let second_report = json_body(second_report).await;
    assert_eq!(first_report["id"], second_report["id"]);
    assert_eq!(first_report["caseId"], second_report["caseId"]);
    let case_id = first_report["caseId"].as_str().expect("case id");

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/content/post/{post_id}"),
        app_id,
        admin_id,
        None,
        Some(json!({ "state": "hidden", "reason": "confirmed spam", "caseId": case_id })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    for _ in 0..2 {
        let response = send(
            &state,
            Method::PUT,
            &format!("/v1/moderation/content/post/{post_id}"),
            app_id,
            admin_id,
            Some(ADMIN_CAPABILITIES),
            Some(json!({ "state": "hidden", "reason": "confirmed spam", "caseId": case_id })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}"),
        app_id,
        user_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/moderation/content/post/{post_id}"),
        app_id,
        admin_id,
        Some("reports.read"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let snapshot = json_body(response).await;
    assert_eq!(snapshot["type"], "post");
    assert_eq!(snapshot["data"]["id"], post_id.to_string());

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{user_id}/restrictions/post"),
        app_id,
        admin_id,
        Some(ADMIN_CAPABILITIES),
        Some(json!({ "reason": "cooldown", "caseId": null })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        user_id,
        None,
        Some(json!({ "body": "blocked post" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/moderation/users/{user_id}/restrictions/post"),
        app_id,
        admin_id,
        Some(ADMIN_CAPABILITIES),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        user_id,
        None,
        Some(json!({ "body": "allowed again" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{user_id}"),
        app_id,
        admin_id,
        Some(ADMIN_CAPABILITIES),
        Some(json!({ "state": "banned", "reason": "repeated abuse" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/profiles/{user_id}"),
        app_id,
        admin_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        user_id,
        None,
        Some(json!({ "body": "banned user post" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{user_id}"),
        app_id,
        admin_id,
        Some(ADMIN_CAPABILITIES),
        Some(json!({ "state": "active", "reason": "appeal accepted" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/profiles/{user_id}"),
        app_id,
        admin_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/roles/{moderator_id}"),
        app_id,
        admin_id,
        Some("roles.manage"),
        Some(json!({ "role": "moderator", "reason": "on-call rotation" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/me",
        app_id,
        moderator_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let me = json_body(response).await;
    assert_eq!(me["role"], "moderator");
    assert!(
        me["effectiveCapabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "reports.read")
    );

    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/cases",
        app_id,
        moderator_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await.as_array().expect("cases").len(),
        1
    );

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/content/post/{post_id}"),
        other_app_id,
        admin_id,
        Some(ADMIN_CAPABILITIES),
        Some(json!({ "state": "removed", "reason": "wrong app" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/audit?limit=100",
        app_id,
        admin_id,
        Some("audit.read"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let audit = json_body(response).await;
    let audit = audit.as_array().expect("audit array");
    assert_eq!(
        audit
            .iter()
            .filter(|event| event["action"] == "content.state")
            .count(),
        1,
        "repeating the same enforcement state must be idempotent"
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
