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
async fn group_moderation_composes_with_local_roles_and_chat() {
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
        FeatureSet::from_csv("groups,chat,moderation").expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let escapee_id = Uuid::new_v4();
    let moderator_id = Uuid::new_v4();
    let outsider_id = Uuid::new_v4();

    let response = send(
        &state,
        Method::POST,
        "/v1/groups",
        app_id,
        owner_id,
        None,
        Some(json!({
            "name": "Moderated Team",
            "memberIds": [member_id, escapee_id]
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let group = json_body(response).await;
    let group_id = Uuid::parse_str(group["id"].as_str().expect("group id")).expect("UUID group id");

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/groups/{group_id}/chat"),
        app_id,
        owner_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let conversation = json_body(response).await;
    let conversation_id = Uuid::parse_str(conversation["id"].as_str().expect("conversation id"))
        .expect("UUID conversation id");

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        member_id,
        None,
        Some(json!({ "body": "message before moderation" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let historical_message_id = json_body(response).await["id"].clone();

    let report_input = json!({
        "targetType": "group",
        "targetId": group_id,
        "category": "abuse",
        "context": "group-level report",
        "idempotencyKey": "group-report-1"
    });
    let response = send(
        &state,
        Method::POST,
        "/v1/reports",
        app_id,
        outsider_id,
        None,
        Some(report_input.clone()),
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "outsiders must not learn whether a private group exists through reporting"
    );

    let first_report = send(
        &state,
        Method::POST,
        "/v1/reports",
        app_id,
        member_id,
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
        member_id,
        None,
        Some(report_input),
    )
    .await;
    assert_eq!(second_report.status(), StatusCode::OK);
    let second_report = json_body(second_report).await;
    assert_eq!(first_report["id"], second_report["id"]);
    assert_eq!(first_report["caseId"], second_report["caseId"]);
    let case_id =
        Uuid::parse_str(first_report["caseId"].as_str().expect("case id")).expect("UUID case id");

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/moderation/content/group/{group_id}"),
        app_id,
        moderator_id,
        Some("reports.read"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let snapshot = json_body(response).await;
    assert_eq!(snapshot["type"], "group");
    assert_eq!(snapshot["data"]["id"], group_id.to_string());

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/content/group/{group_id}"),
        app_id,
        moderator_id,
        Some("content.moderate"),
        Some(json!({
            "state": "hidden",
            "reason": "review in progress",
            "caseId": case_id
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/groups/{group_id}"),
        app_id,
        owner_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        "/v1/groups",
        app_id,
        owner_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("groups")
            .is_empty()
    );

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}"),
        app_id,
        owner_id,
        None,
        Some(json!({ "name": "Must Not Change", "avatarMediaId": null })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{escapee_id}/restrictions/group"),
        app_id,
        moderator_id,
        Some("users.restrict"),
        Some(json!({ "reason": "group actions restricted" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/groups/{group_id}/leave"),
        app_id,
        escapee_id,
        None,
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::NO_CONTENT,
        "a restriction or hidden group must not prevent a member from leaving"
    );

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/content/group/{group_id}"),
        app_id,
        moderator_id,
        Some("content.moderate"),
        Some(json!({
            "state": "active",
            "reason": "review complete",
            "caseId": case_id
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{owner_id}/restrictions/group"),
        app_id,
        moderator_id,
        Some("users.restrict"),
        Some(json!({ "reason": "group management cooldown" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}"),
        app_id,
        owner_id,
        None,
        Some(json!({ "name": "Blocked Rename", "avatarMediaId": null })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/moderation/users/{owner_id}/restrictions/group"),
        app_id,
        moderator_id,
        Some("users.restrict"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}"),
        app_id,
        owner_id,
        None,
        Some(json!({ "name": "Allowed Rename", "avatarMediaId": null })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let removal_input = json!({
        "reason": "confirmed abuse",
        "caseId": case_id
    });
    for _ in 0..2 {
        let response = send(
            &state,
            Method::POST,
            &format!("/v1/moderation/groups/{group_id}/members/{member_id}/remove"),
            app_id,
            moderator_id,
            Some("users.restrict"),
            Some(removal_input.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/groups/{group_id}"),
        app_id,
        member_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        member_id,
        None,
        Some(json!({ "body": "must not send after removal" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        owner_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let messages = json_body(response).await;
    assert!(
        messages
            .as_array()
            .expect("messages")
            .iter()
            .any(|message| message["id"] == historical_message_id),
        "privileged removal must not rewrite message history"
    );

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/moderation/groups/{group_id}/members/{owner_id}/remove"),
        app_id,
        moderator_id,
        Some("users.restrict"),
        Some(json!({ "reason": "invalid owner removal" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/moderation/groups/{group_id}/members/{owner_id}/remove"),
        other_app_id,
        moderator_id,
        Some("users.restrict"),
        Some(json!({ "reason": "wrong app" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let moderator_leave_events = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_membership_events WHERE app_id = $1 AND group_id = $2 AND user_id = $3 AND event_type = 'left' AND actor_id = $4",
    )
    .bind(app_id)
    .bind(group_id)
    .bind(member_id)
    .bind(moderator_id)
    .fetch_one(&state.pool)
    .await
    .expect("membership history should be queryable");
    assert_eq!(moderator_leave_events, 1);

    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/audit?limit=100",
        app_id,
        moderator_id,
        Some("audit.read"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let audit = json_body(response).await;
    assert_eq!(
        audit
            .as_array()
            .expect("audit")
            .iter()
            .filter(|event| event["action"] == "group.member.remove")
            .count(),
        1,
        "idempotent force removal must emit one privileged audit event"
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
