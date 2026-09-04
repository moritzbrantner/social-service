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
async fn visibility_and_follow_graph_hold_against_postgres() {
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
        FeatureSet::from_csv("comments,follows").expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();

    let response = send(
        &state,
        Method::PUT,
        "/v1/profiles/me",
        app_id,
        Some(owner_id),
        Some(json!({
            "displayName": "Private owner",
            "visibility": "private"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["visibility"], "private");

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/profiles/{owner_id}"),
        app_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/profiles/{owner_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::PUT,
        "/v1/profiles/me",
        app_id,
        Some(other_id),
        Some(json!({ "displayName": "Public target" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["visibility"], "public");

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/follows/{other_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/follows/{other_id}/followers"),
        app_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let followers = json_body(response).await;
    let followers = followers.as_array().expect("followers should be an array");
    assert_eq!(followers.len(), 1);
    assert_eq!(followers[0]["followerId"], owner_id.to_string());
    assert_eq!(followers[0]["followedId"], other_id.to_string());

    let response = send(
        &state,
        Method::POST,
        "/v1/posts",
        app_id,
        Some(owner_id),
        Some(json!({
            "body": "owner-only post",
            "visibility": "private"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let post = json_body(response).await;
    assert_eq!(post["visibility"], "private");
    let post_id = post["id"].as_str().expect("post should have an id");

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}"),
        app_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}"),
        app_id,
        Some(other_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{post_id}/comments"),
        app_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::GET,
        "/v1/timeline",
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let timeline = json_body(response).await;
    let timeline = timeline.as_array().expect("timeline should be an array");
    assert!(timeline.iter().any(|candidate| candidate["id"] == post_id));
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
