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
async fn saved_posts_are_private_and_message_pins_are_conversation_shared() {
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
        FeatureSet::from_csv("saves,chat").expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let author_id = Uuid::new_v4();
    let member_id = Uuid::new_v4();
    let outsider_id = Uuid::new_v4();

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        author_id,
        Some(json!({ "body": "worth finding again" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let post = json_body(response).await;
    let post_id = post["id"].as_str().expect("post should have an id");

    for _ in 0..2 {
        let response = send(
            &state,
            Method::PUT,
            &format!("/v1/posts/{post_id}/save"),
            app_id,
            member_id,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let response = send(
        &state,
        Method::GET,
        "/v1/saved-posts",
        app_id,
        member_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await;
    let saved = saved.as_array().expect("saved posts should be an array");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["id"], post_id);
    assert!(saved[0]["savedAt"].is_string());

    let response = send(
        &state,
        Method::GET,
        "/v1/saved-posts",
        app_id,
        outsider_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("saved posts should be an array")
            .is_empty(),
        "a user's saved posts must remain private"
    );

    for _ in 0..2 {
        let response = send(
            &state,
            Method::DELETE,
            &format!("/v1/posts/{post_id}/save"),
            app_id,
            member_id,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        author_id,
        Some(json!({ "body": "private", "visibility": "private" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let private_post = json_body(response).await;
    let private_post_id = private_post["id"]
        .as_str()
        .expect("private post should have an id");
    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/posts/{private_post_id}/save"),
        app_id,
        member_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::POST,
        "/v1/conversations",
        app_id,
        author_id,
        Some(json!({ "memberIds": [member_id] })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let conversation = json_body(response).await;
    let conversation_id = conversation["id"]
        .as_str()
        .expect("conversation should have an id");

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        author_id,
        Some(json!({ "body": "pin this" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let message = json_body(response).await;
    let message_id = message["id"].as_str().expect("message should have an id");

    for _ in 0..2 {
        let response = send(
            &state,
            Method::PUT,
            &format!("/v1/conversations/{conversation_id}/pins/{message_id}"),
            app_id,
            member_id,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/conversations/{conversation_id}/pins"),
        app_id,
        author_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let pins = json_body(response).await;
    let pins = pins.as_array().expect("pins should be an array");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0]["id"], message_id);
    assert_eq!(pins[0]["pinnedBy"], member_id.to_string());
    assert!(pins[0]["pinnedAt"].is_string());

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/conversations/{conversation_id}/pins"),
        app_id,
        outsider_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = send(
        &state,
        Method::POST,
        "/v1/conversations",
        app_id,
        author_id,
        Some(json!({ "memberIds": [outsider_id] })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let other_conversation = json_body(response).await;
    let other_conversation_id = other_conversation["id"]
        .as_str()
        .expect("conversation should have an id");
    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/conversations/{other_conversation_id}/pins/{message_id}"),
        app_id,
        author_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    for _ in 0..2 {
        let response = send(
            &state,
            Method::DELETE,
            &format!("/v1/conversations/{conversation_id}/pins/{message_id}"),
            app_id,
            author_id,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/conversations/{conversation_id}/pins"),
        app_id,
        member_id,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("pins should be an array")
            .is_empty()
    );
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
