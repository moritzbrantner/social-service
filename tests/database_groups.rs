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
async fn groups_keep_membership_roles_and_chat_in_sync() {
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
        FeatureSet::from_csv("groups,chat").expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let admin_id = Uuid::new_v4();
    let newcomer_id = Uuid::new_v4();
    let outsider_id = Uuid::new_v4();

    let response = send(
        &state,
        Method::POST,
        "/v1/groups",
        app_id,
        owner_id,
        Some(json!({
            "name": "Project Team",
            "memberIds": [member_id]
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let group = json_body(response).await;
    let group_id = Uuid::parse_str(group["id"].as_str().expect("group id")).expect("UUID group id");
    assert_eq!(group["name"], "Project Team");
    assert_eq!(group["chatConversationId"], Value::Null);
    assert_eq!(group["members"].as_array().expect("members").len(), 2);
    assert_role(&group, owner_id, "owner");
    assert_role(&group, member_id, "member");

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/groups/{group_id}"),
        app_id,
        outsider_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}/members/{newcomer_id}"),
        app_id,
        member_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}/members/{admin_id}"),
        app_id,
        owner_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}/members/{admin_id}/role"),
        app_id,
        owner_id,
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let group = json_body(response).await;
    assert_role(&group, admin_id, "admin");

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}/members/{newcomer_id}"),
        app_id,
        admin_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}"),
        app_id,
        admin_id,
        Some(json!({
            "name": "Renamed Team",
            "avatarMediaId": null
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["name"], "Renamed Team");

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/groups/{group_id}/chat"),
        app_id,
        admin_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let conversation = json_body(response).await;
    let conversation_id =
        Uuid::parse_str(conversation["id"].as_str().expect("group conversation id"))
            .expect("UUID conversation id");
    assert_eq!(
        conversation["memberIds"]
            .as_array()
            .expect("conversation members")
            .len(),
        4
    );

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        newcomer_id,
        Some(json!({ "body": "Message before leaving" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let message = json_body(response).await;
    let message_id = message["id"].clone();

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/groups/{group_id}/members/{member_id}"),
        app_id,
        owner_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let group = json_body(response).await;
    assert!(!has_member(&group, member_id));

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        member_id,
        Some(json!({ "body": "Should not send" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/groups/{group_id}/leave"),
        app_id,
        newcomer_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        owner_id,
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
            .any(|message| message["id"] == message_id),
        "leaving a group must not rewrite or delete existing messages"
    );

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/groups/{group_id}/members/{admin_id}/role"),
        app_id,
        owner_id,
        Some(json!({ "role": "owner" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let group = json_body(response).await;
    assert_role(&group, admin_id, "owner");
    assert_role(&group, owner_id, "admin");

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/groups/{group_id}/leave"),
        app_id,
        owner_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let left_events = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM group_membership_events WHERE app_id = $1 AND group_id = $2 AND event_type = 'left'",
    )
    .bind(app_id)
    .bind(group_id)
    .fetch_one(&state.pool)
    .await
    .expect("membership history should be queryable");
    assert_eq!(
        left_events, 3,
        "removals and voluntary leaves must remain in append-only membership history"
    );

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/groups/{group_id}"),
        other_app_id,
        admin_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/groups/{group_id}/chat"),
        app_id,
        admin_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let conversation_again = json_body(response).await;
    assert_eq!(conversation_again["id"], conversation_id.to_string());
    assert_eq!(
        conversation_again["memberIds"]
            .as_array()
            .expect("conversation members")
            .len(),
        1,
        "idempotent chat resolution must heal membership to the current group"
    );
}

fn assert_role(group: &Value, user_id: Uuid, role: &str) {
    let member = group["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["userId"] == user_id.to_string())
        .expect("member should exist");
    assert_eq!(member["role"], role);
}

fn has_member(group: &Value, user_id: Uuid) -> bool {
    group["members"]
        .as_array()
        .expect("members")
        .iter()
        .any(|member| member["userId"] == user_id.to_string())
}

async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    app_id: Uuid,
    user_id: Uuid,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-app-id", app_id.to_string())
        .header("x-user-id", user_id.to_string());
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
