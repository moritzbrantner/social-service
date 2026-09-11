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
async fn blocks_and_mutes_share_one_cross_surface_safety_policy() {
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
        FeatureSet::from_csv("posts,comments,follows,blocks,mutes,chat")
            .expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let other_app_id = Uuid::new_v4();
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let carol = Uuid::new_v4();

    for (user_id, name) in [(alice, "Alice"), (bob, "Bob"), (carol, "Carol")] {
        assert_eq!(
            send(
                &state,
                Method::PUT,
                "/v1/profiles/me",
                app_id,
                user_id,
                Some(json!({ "displayName": name })),
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    for (user_id, name) in [(alice, "Alice elsewhere"), (bob, "Bob elsewhere")] {
        assert_eq!(
            send(
                &state,
                Method::PUT,
                "/v1/profiles/me",
                other_app_id,
                user_id,
                Some(json!({ "displayName": name })),
            )
            .await
            .status(),
            StatusCode::OK
        );
    }

    let alice_post = create_post(&state, app_id, alice, "Alice post").await;
    let bob_post = create_post(&state, app_id, bob, "Bob post").await;

    assert_eq!(follow(&state, app_id, alice, bob).await.status(), StatusCode::NO_CONTENT);
    assert_eq!(follow(&state, app_id, bob, alice).await.status(), StatusCode::NO_CONTENT);

    let direct = create_conversation(&state, app_id, alice, &[bob]).await;
    let direct_id = Uuid::parse_str(direct["id"].as_str().expect("conversation id"))
        .expect("conversation id should be UUID");
    let bob_direct_message = send_message(&state, app_id, bob, direct_id, "before block").await;
    let bob_direct_message_id = Uuid::parse_str(
        bob_direct_message["id"]
            .as_str()
            .expect("direct message id"),
    )
    .expect("direct message id should be UUID");
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/conversations/{direct_id}/pins/{bob_direct_message_id}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    let shared = create_conversation(&state, app_id, alice, &[bob, carol]).await;
    let shared_id = Uuid::parse_str(shared["id"].as_str().expect("shared conversation id"))
        .expect("shared conversation id should be UUID");
    let bob_shared_message = send_message(&state, app_id, bob, shared_id, "Bob shared").await;
    let bob_shared_message_id = Uuid::parse_str(
        bob_shared_message["id"]
            .as_str()
            .expect("shared message id"),
    )
    .expect("shared message id should be UUID");
    let _carol_shared_message = send_message(&state, app_id, carol, shared_id, "Carol shared").await;

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/blocks/{bob}"),
        app_id,
        alice,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM follows WHERE app_id = $1 AND ((follower_id = $2 AND followed_id = $3) OR (follower_id = $3 AND followed_id = $2))",
        )
        .bind(app_id)
        .bind(alice)
        .bind(bob)
        .fetch_one(&state.pool)
        .await
        .expect("follow count"),
        0,
        "blocking must sever both follow directions"
    );

    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/profiles/{bob}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/profiles/{alice}"),
            app_id,
            bob,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND,
        "a directional block must become a bilateral visibility boundary"
    );
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/profiles/{bob}"),
            other_app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::OK,
        "block policy must remain app-scoped"
    );
    assert_eq!(follow(&state, app_id, alice, bob).await.status(), StatusCode::NOT_FOUND);
    assert_eq!(follow(&state, app_id, bob, alice).await.status(), StatusCode::NOT_FOUND);

    let blocks = json_body(
        send(&state, Method::GET, "/v1/blocks", app_id, alice, None).await,
    )
    .await;
    assert_eq!(blocks.as_array().expect("blocks").len(), 1);
    assert_eq!(blocks[0]["userId"], bob.to_string());
    let bob_blocks = json_body(
        send(&state, Method::GET, "/v1/blocks", app_id, bob, None).await,
    )
    .await;
    assert!(bob_blocks.as_array().expect("Bob blocks").is_empty());

    assert_eq!(
        send(
            &state,
            Method::POST,
            &format!("/v1/conversations/{direct_id}/messages"),
            app_id,
            alice,
            Some(json!({ "body": "must not send" })),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &state,
            Method::POST,
            "/v1/conversations",
            app_id,
            alice,
            Some(json!({ "memberIds": [bob] })),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    let direct_messages = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/conversations/{direct_id}/messages"),
            app_id,
            alice,
            None,
        )
        .await,
    )
    .await;
    assert!(
        direct_messages.as_array().expect("direct messages").is_empty(),
        "blocked authors must disappear from message reads"
    );
    let direct_pins = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/conversations/{direct_id}/pins"),
            app_id,
            alice,
            None,
        )
        .await,
    )
    .await;
    assert!(direct_pins.as_array().expect("pins").is_empty());
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/conversations/{shared_id}/pins/{bob_shared_message_id}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        send_message_status(&state, app_id, alice, shared_id, "Alice can still use shared chat").await,
        StatusCode::OK,
        "a block must not silently rewrite shared-group membership"
    );
    let shared_messages = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/conversations/{shared_id}/messages"),
            app_id,
            alice,
            None,
        )
        .await,
    )
    .await;
    let shared_messages = shared_messages.as_array().expect("shared messages");
    assert!(shared_messages.iter().any(|message| message["authorId"] == carol.to_string()));
    assert!(shared_messages.iter().any(|message| message["authorId"] == alice.to_string()));
    assert!(!shared_messages.iter().any(|message| message["authorId"] == bob.to_string()));

    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/blocks/{bob}"),
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
            Method::GET,
            &format!("/v1/profiles/{bob}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM follows WHERE app_id = $1 AND ((follower_id = $2 AND followed_id = $3) OR (follower_id = $3 AND followed_id = $2))",
        )
        .bind(app_id)
        .bind(alice)
        .bind(bob)
        .fetch_one(&state.pool)
        .await
        .expect("follow count"),
        0,
        "unblocking must not recreate old follows"
    );
    assert_eq!(follow(&state, app_id, alice, bob).await.status(), StatusCode::NO_CONTENT);

    let bob_comment = create_comment(&state, app_id, bob, alice_post, "Bob comment").await;
    let carol_comment = create_comment(&state, app_id, carol, alice_post, "Carol comment").await;
    assert!(bob_comment["id"].is_string());
    assert!(carol_comment["id"].is_string());

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/mutes/{bob}"),
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
            Method::GET,
            &format!("/v1/posts/{bob_post}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::OK,
        "muting must not become a direct visibility boundary"
    );
    assert_eq!(
        send_message_status(&state, app_id, bob, direct_id, "mute does not block chat").await,
        StatusCode::OK
    );

    let timeline = json_body(
        send(&state, Method::GET, "/v1/timeline", app_id, alice, None).await,
    )
    .await;
    assert!(!timeline
        .as_array()
        .expect("timeline")
        .iter()
        .any(|post| post["authorId"] == bob.to_string()));

    let comments = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/posts/{alice_post}/comments"),
            app_id,
            alice,
            None,
        )
        .await,
    )
    .await;
    let comments = comments.as_array().expect("comments");
    assert!(!comments.iter().any(|comment| comment["authorId"] == bob.to_string()));
    assert!(comments.iter().any(|comment| comment["authorId"] == carol.to_string()));

    let mutes = json_body(
        send(&state, Method::GET, "/v1/mutes", app_id, alice, None).await,
    )
    .await;
    assert_eq!(mutes.as_array().expect("mutes").len(), 1);
    assert_eq!(mutes[0]["userId"], bob.to_string());

    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/mutes/{bob}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let timeline = json_body(
        send(&state, Method::GET, "/v1/timeline", app_id, alice, None).await,
    )
    .await;
    assert!(timeline
        .as_array()
        .expect("timeline")
        .iter()
        .any(|post| post["authorId"] == bob.to_string()));

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/blocks/{alice}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/mutes/{alice}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
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

async fn create_comment(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    post_id: Uuid,
    body: &str,
) -> Value {
    let response = send(
        state,
        Method::POST,
        &format!("/v1/posts/{post_id}/comments"),
        app_id,
        user_id,
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn follow(state: &AppState, app_id: Uuid, follower_id: Uuid, followed_id: Uuid) -> Response {
    send(
        state,
        Method::PUT,
        &format!("/v1/follows/{followed_id}"),
        app_id,
        follower_id,
        None,
    )
    .await
}

async fn create_conversation(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    other_members: &[Uuid],
) -> Value {
    let response = send(
        state,
        Method::POST,
        "/v1/conversations",
        app_id,
        user_id,
        Some(json!({ "memberIds": other_members })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn send_message(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    conversation_id: Uuid,
    body: &str,
) -> Value {
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
    json_body(response).await
}

async fn send_message_status(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    conversation_id: Uuid,
    body: &str,
) -> StatusCode {
    send(
        state,
        Method::POST,
        &format!("/v1/conversations/{conversation_id}/messages"),
        app_id,
        user_id,
        Some(json!({ "body": body })),
    )
    .await
    .status()
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
