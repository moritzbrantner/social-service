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
async fn reactions_are_idempotent_visible_and_removed_across_safety_boundaries() {
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
        pool.clone(),
        FeatureSet::from_csv("reactions,comments,blocks")
            .expect("reaction test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    let reactor_id = Uuid::new_v4();
    let third_id = Uuid::new_v4();

    for (user_id, display_name) in [
        (owner_id, "Owner"),
        (reactor_id, "Reactor"),
        (third_id, "Third"),
    ] {
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

    let post_id = create_post(&state, app_id, owner_id, "public post", None).await;

    for _ in 0..2 {
        let response = send(
            &state,
            Method::PUT,
            &reaction_path("post", post_id),
            app_id,
            Some(reactor_id),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    let summary = get_summary(&state, app_id, reactor_id, "post", post_id).await;
    assert_eq!(
        summary["counts"],
        json!([{ "reactionType": "like", "count": 1 }])
    );
    assert_eq!(summary["currentUserReactions"], json!(["like"]));

    let response = send(
        &state,
        Method::PUT,
        &reaction_path("post", post_id),
        app_id,
        Some(third_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let summary = get_summary(&state, app_id, owner_id, "post", post_id).await;
    assert_eq!(
        summary["counts"],
        json!([{ "reactionType": "like", "count": 2 }])
    );
    assert_eq!(summary["currentUserReactions"], json!([]));

    let comment = create_comment(&state, app_id, owner_id, post_id, "root comment").await;
    let comment_id = id(&comment);
    let response = send(
        &state,
        Method::PUT,
        &reaction_path("comment", comment_id),
        app_id,
        Some(reactor_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let summary = get_summary(&state, app_id, reactor_id, "comment", comment_id).await;
    assert_eq!(
        summary["counts"],
        json!([{ "reactionType": "like", "count": 1 }])
    );

    let private_post_id =
        create_post(&state, app_id, owner_id, "private post", Some("private")).await;
    let response = send(
        &state,
        Method::PUT,
        &reaction_path("post", private_post_id),
        app_id,
        Some(reactor_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(
        &state,
        Method::GET,
        &format!("/v1/reactions/post/{private_post_id}"),
        app_id,
        Some(reactor_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let third_post_id = create_post(&state, app_id, third_id, "third-party post", None).await;
    let response = send(
        &state,
        Method::PUT,
        &reaction_path("post", third_post_id),
        app_id,
        Some(reactor_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::PUT,
        &reaction_path("post", post_id),
        app_id,
        Some(reactor_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/blocks/{reactor_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let remaining_pair_reactions = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM reactions WHERE app_id = $1 AND user_id = $2 AND ((target_type = 'post' AND post_id = $3) OR (target_type = 'comment' AND comment_id = $4))",
    )
    .bind(app_id)
    .bind(reactor_id)
    .bind(post_id)
    .bind(comment_id)
    .fetch_one(&pool)
    .await
    .expect("reaction count should load");
    assert_eq!(
        remaining_pair_reactions, 0,
        "blocking must clear direct reactions to the blocked user's content"
    );

    let summary = get_summary(&state, app_id, owner_id, "post", third_post_id).await;
    assert_eq!(
        summary["counts"],
        json!([]),
        "a blocked actor must not contribute to the viewer's aggregate count"
    );
    let summary = get_summary(&state, app_id, third_id, "post", third_post_id).await;
    assert_eq!(
        summary["counts"],
        json!([{ "reactionType": "like", "count": 1 }]),
        "blocking another viewer must not delete a reaction to unrelated content"
    );

    let response = send(
        &state,
        Method::PUT,
        &reaction_path("post", post_id),
        app_id,
        Some(reactor_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/blocks/{reactor_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let summary = get_summary(&state, app_id, owner_id, "post", post_id).await;
    assert_eq!(
        summary["counts"],
        json!([{ "reactionType": "like", "count": 1 }])
    );

    let leaf = create_comment(&state, app_id, owner_id, post_id, "leaf").await;
    let leaf_id = id(&leaf);
    let response = send(
        &state,
        Method::PUT,
        &reaction_path("comment", leaf_id),
        app_id,
        Some(third_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
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
    let leaf_reactions = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM reactions WHERE app_id = $1 AND target_type = 'comment' AND target_id = $2",
    )
    .bind(app_id)
    .bind(leaf_id)
    .fetch_one(&pool)
    .await
    .expect("leaf reaction count should load");
    assert_eq!(
        leaf_reactions, 0,
        "physical comment deletion must cascade reactions"
    );

    let parent = create_comment(&state, app_id, owner_id, post_id, "parent").await;
    let parent_id = id(&parent);
    let child = reply_to_comment(&state, app_id, third_id, post_id, parent_id, "child").await;
    assert_ne!(id(&child), parent_id);
    let response = send(
        &state,
        Method::PUT,
        &reaction_path("comment", parent_id),
        app_id,
        Some(third_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{post_id}/comments/{parent_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = send(
        &state,
        Method::GET,
        &format!("/v1/reactions/comment/{parent_id}"),
        app_id,
        Some(third_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(
        &state,
        Method::DELETE,
        &reaction_path("comment", parent_id),
        app_id,
        Some(third_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let tombstone_reactions = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM reactions WHERE app_id = $1 AND target_type = 'comment' AND target_id = $2",
    )
    .bind(app_id)
    .bind(parent_id)
    .fetch_one(&pool)
    .await
    .expect("tombstone reaction count should load");
    assert_eq!(
        tombstone_reactions, 0,
        "users must be able to remove stale reactions from hidden tombstones"
    );

    let response = send(
        &state,
        Method::PUT,
        &reaction_path("post", post_id),
        app_id,
        Some(third_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{post_id}"),
        app_id,
        Some(owner_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let post_reactions = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM reactions WHERE app_id = $1 AND (post_id = $2 OR comment_id IN (SELECT id FROM comments WHERE app_id = $1 AND post_id = $2))",
    )
    .bind(app_id)
    .bind(post_id)
    .fetch_one(&pool)
    .await
    .expect("post reaction count should load");
    assert_eq!(
        post_reactions, 0,
        "post deletion must cascade its reaction graph"
    );
}

async fn create_post(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    body: &str,
    visibility: Option<&str>,
) -> Uuid {
    let mut value = json!({ "body": body });
    if let Some(visibility) = visibility {
        value["visibility"] = json!(visibility);
    }
    let response = send(
        state,
        Method::POST,
        "/v1/posts",
        app_id,
        Some(user_id),
        Some(value),
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

async fn get_summary(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    target_type: &str,
    target_id: Uuid,
) -> Value {
    let response = send(
        state,
        Method::GET,
        &format!("/v1/reactions/{target_type}/{target_id}"),
        app_id,
        Some(user_id),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

fn reaction_path(target_type: &str, target_id: Uuid) -> String {
    format!("/v1/reactions/{target_type}/{target_id}/like")
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
