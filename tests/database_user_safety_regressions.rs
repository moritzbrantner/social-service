use std::time::Duration;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use social_service::{
    app,
    features::FeatureSet,
    relationships::lock_user_pair,
    state::AppState,
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn block_policy_closes_saved_post_and_direct_pin_write_gaps() {
    let state = test_state().await;
    let app_id = Uuid::new_v4();
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();

    create_profile(&state, app_id, alice, "Alice").await;
    create_profile(&state, app_id, bob, "Bob").await;

    let bob_post = create_post(&state, app_id, bob, "Bob post").await;
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/posts/{bob_post}/save"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    let conversation = json_body(
        send(
            &state,
            Method::POST,
            "/v1/conversations",
            app_id,
            alice,
            Some(json!({ "memberIds": [bob] })),
        )
        .await,
    )
    .await;
    let conversation_id = Uuid::parse_str(
        conversation["id"]
            .as_str()
            .expect("conversation id should exist"),
    )
    .expect("conversation id should be UUID");

    let alice_message = create_message(&state, app_id, alice, conversation_id, "Alice message").await;
    let bob_message = create_message(&state, app_id, bob, conversation_id, "Bob message").await;
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/conversations/{conversation_id}/pins/{bob_message}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/blocks/{bob}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    let saved = json_body(
        send(
            &state,
            Method::GET,
            "/v1/saved-posts",
            app_id,
            alice,
            None,
        )
        .await,
    )
    .await;
    assert!(
        saved.as_array().expect("saved-post list").is_empty(),
        "saved posts must reapply the current block boundary"
    );
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/posts/{bob_post}/save"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND,
        "known post ids must not bypass block visibility through saves"
    );

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/conversations/{conversation_id}/pins/{alice_message}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::FORBIDDEN,
        "all writes to a blocked direct conversation must stop, even for the caller's own message"
    );
    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/conversations/{conversation_id}/pins/{bob_message}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::FORBIDDEN,
        "unpin is also a direct-conversation write"
    );
    let pin_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM conversation_message_pins WHERE app_id = $1 AND conversation_id = $2 AND message_id = $3",
    )
    .bind(app_id)
    .bind(conversation_id)
    .bind(bob_message)
    .fetch_one(&state.pool)
    .await
    .expect("pin count should be readable");
    assert_eq!(pin_count, 1, "blocked unpin must not mutate persisted pin state");
}

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn user_pair_lock_is_symmetric_and_app_scoped() {
    let state = test_state().await;
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();

    let mut held = state.pool.begin().await.expect("first transaction");
    lock_user_pair(&mut held, app_id, alice, bob)
        .await
        .expect("first pair lock");

    let mut other_app = state.pool.begin().await.expect("other-app transaction");
    tokio::time::timeout(
        Duration::from_millis(250),
        lock_user_pair(&mut other_app, other_app_id, bob, alice),
    )
    .await
    .expect("same user ids in another app must not contend")
    .expect("other-app pair lock");
    other_app.commit().await.expect("other-app commit");

    let mut same_app = state.pool.begin().await.expect("same-app transaction");
    assert!(
        tokio::time::timeout(
            Duration::from_millis(250),
            lock_user_pair(&mut same_app, app_id, bob, alice),
        )
        .await
        .is_err(),
        "reversed user order must contend on the same app-scoped pair lock"
    );
    drop(same_app);
    held.commit().await.expect("release pair lock");
}

async fn test_state() -> AppState {
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
    AppState::new(
        pool,
        FeatureSet::from_csv("posts,saves,follows,blocks,chat")
            .expect("test capabilities should resolve"),
    )
}

async fn create_profile(state: &AppState, app_id: Uuid, user_id: Uuid, name: &str) {
    let response = send(
        state,
        Method::PUT,
        "/v1/profiles/me",
        app_id,
        user_id,
        Some(json!({ "displayName": name })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

async fn create_post(state: &AppState, app_id: Uuid, user_id: Uuid, body: &str) -> Uuid {
    let response = send(
        state,
        Method::POST,
        "/v1/posts",
        app_id,
        user_id,
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    Uuid::parse_str(body["id"].as_str().expect("post id")).expect("post id should be UUID")
}

async fn create_message(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    conversation_id: Uuid,
    body: &str,
) -> Uuid {
    let response = send(
        state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        user_id,
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    Uuid::parse_str(body["id"].as_str().expect("message id")).expect("message id should be UUID")
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
