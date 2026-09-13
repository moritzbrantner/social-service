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
async fn threaded_comments_preserve_structure_and_policy() {
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
        FeatureSet::from_csv("comments,mutes").expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();

    for (user_id, display_name) in [(owner_id, "Owner"), (other_id, "Other")] {
        let response = send(
            &state,
            Method::PUT,
            "/v1/profiles/me",
            app_id,
            Some(user_id),
            Some(json!({ "displayName": display_name })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let post_id = create_post(&state, app_id, owner_id, "threaded post").await;
    let other_post_id = create_post(&state, app_id, owner_id, "other post").await;

    let root = create_comment(&state, app_id, owner_id, post_id, "root").await;
    let root_id = id(&root);
    assert!(root["parentCommentId"].is_null());
    assert!(root["deletedAt"].is_null());

    let leaf = create_comment(&state, app_id, owner_id, post_id, "leaf root").await;
    let leaf_id = id(&leaf);

    let reply = reply_to_comment(&state, app_id, other_id, post_id, root_id, "first reply").await;
    let reply_id = id(&reply);
    assert_eq!(reply["parentCommentId"], root_id.to_string());

    let nested =
        reply_to_comment(&state, app_id, owner_id, post_id, reply_id, "nested reply").await;
    let nested_id = id(&nested);
    assert_eq!(nested["parentCommentId"], reply_id.to_string());

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let roots = json_body(response).await;
    let roots = roots.as_array().expect("root comments should be an array");
    assert_eq!(roots.len(), 2);
    assert!(
        roots
            .iter()
            .any(|comment| comment["id"] == root_id.to_string())
    );
    assert!(
        roots
            .iter()
            .any(|comment| comment["id"] == leaf_id.to_string())
    );
    assert!(
        !roots
            .iter()
            .any(|comment| comment["id"] == reply_id.to_string())
    );

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/posts/{other_post_id}/comments/{root_id}/replies"),
        app_id,
        Some(other_id),
        Some(json!({ "body": "cross-post reply" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/mutes/{other_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments/{root_id}/replies"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("replies should be an array")
            .is_empty(),
        "muted authors must stay filtered from reply reads"
    );

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/mutes/{other_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments/{root_id}/replies"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let replies = json_body(response).await;
    let replies = replies.as_array().expect("replies should be an array");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["id"], reply_id.to_string());

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{post_id}/comments/{root_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let roots = json_body(response).await;
    let tombstone = roots
        .as_array()
        .expect("root comments should be an array")
        .iter()
        .find(|comment| comment["id"] == root_id.to_string())
        .expect("deleted parent should remain as a tombstone");
    assert_eq!(tombstone["body"], "");
    assert!(tombstone["deletedAt"].is_string());

    let response = send(
        &state,
        Method::POST,
        &format!("/v1/posts/{post_id}/comments/{root_id}/replies"),
        app_id,
        Some(other_id),
        Some(json!({ "body": "too late" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments/{root_id}/replies"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let replies = json_body(response).await;
    assert_eq!(
        replies
            .as_array()
            .expect("replies should be an array")
            .first()
            .expect("existing child should remain")["id"],
        reply_id.to_string()
    );

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{post_id}/comments/{nested_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments/{reply_id}/replies"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("nested replies should be an array")
            .is_empty(),
        "leaf deletion should remove the row rather than leave a tombstone"
    );

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{post_id}/comments/{leaf_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("root comments should be an array")
            .iter()
            .all(|comment| comment["id"] != leaf_id.to_string()),
        "leaf root deletion should physically remove the comment"
    );
}

async fn create_post(state: &AppState, app_id: Uuid, user_id: Uuid, body: &str) -> Uuid {
    let response = send(
        state,
        Method::POST,
        "/v1/posts",
        app_id,
        Some(user_id),
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    id(&json_body(response).await)
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
        Some(user_id),
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn reply_to_comment(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    post_id: Uuid,
    comment_id: Uuid,
    body: &str,
) -> Value {
    let response = send(
        state,
        Method::POST,
        &format!("/v1/posts/{post_id}/comments/{comment_id}/replies"),
        app_id,
        Some(user_id),
        Some(json!({ "body": body })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().expect("resource should have an id"))
        .expect("resource id should be a UUID")
}

async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    app_id: Uuid,
    user_id: Option<Uuid>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-app-id", app_id.to_string());
    if let Some(user_id) = user_id {
        builder = builder.header("x-user-id", user_id.to_string());
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
