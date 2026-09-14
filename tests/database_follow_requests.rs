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
async fn follow_requests_are_explicit_idempotent_and_separate_from_visibility() {
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
        FeatureSet::from_csv("posts,follows,follow_requests,blocks")
            .expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();
    let carol = Uuid::new_v4();

    upsert_profile(&state, app_id, alice, "Alice", "public").await;
    upsert_profile(&state, app_id, bob, "Bob", "private").await;
    upsert_profile(&state, app_id, carol, "Carol", "public").await;

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
        StatusCode::NOT_FOUND,
        "private visibility remains owner-only before approval"
    );

    for _ in 0..2 {
        assert_eq!(
            request_follow(&state, app_id, alice, bob).await.status(),
            StatusCode::NO_CONTENT,
            "requesting the same follow must be idempotent"
        );
    }

    let outgoing = json_body(
        send(
            &state,
            Method::GET,
            "/v1/follow-requests/outgoing",
            app_id,
            alice,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(outgoing.as_array().expect("outgoing requests").len(), 1);
    assert_eq!(outgoing[0]["requesterId"], alice.to_string());
    assert_eq!(outgoing[0]["targetId"], bob.to_string());

    let incoming = json_body(
        send(
            &state,
            Method::GET,
            "/v1/follow-requests/incoming",
            app_id,
            bob,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(incoming.as_array().expect("incoming requests").len(), 1);
    assert_eq!(
        follow_count(&state, app_id, alice, bob).await,
        0,
        "a pending request is not a follow edge"
    );
    assert_eq!(approval_count(&state, app_id, alice, bob).await, 0);

    for _ in 0..2 {
        assert_eq!(
            accept_follow(&state, app_id, bob, alice).await.status(),
            StatusCode::NO_CONTENT,
            "acceptance must remain idempotent after durable approval exists"
        );
    }
    assert_eq!(follow_count(&state, app_id, alice, bob).await, 1);
    assert_eq!(
        approval_count(&state, app_id, alice, bob).await,
        1,
        "accepted consent must remain distinguishable from the follow edge"
    );
    assert_eq!(pending_count(&state, app_id, alice, bob).await, 0);

    let approvals = json_body(
        send(
            &state,
            Method::GET,
            "/v1/follow-approvals",
            app_id,
            bob,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(approvals.as_array().expect("approvals").len(), 1);
    assert_eq!(approvals[0]["requesterId"], alice.to_string());
    assert_eq!(approvals[0]["targetId"], bob.to_string());
    assert!(approvals[0]["approvedAt"].is_string());

    assert_eq!(
        request_follow(&state, app_id, alice, bob).await.status(),
        StatusCode::NO_CONTENT,
        "an already approved relationship must not recreate a pending request"
    );
    assert_eq!(pending_count(&state, app_id, alice, bob).await, 0);

    let private_post = create_post(&state, app_id, bob, "private post", "private").await;
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
        StatusCode::NOT_FOUND,
        "approval must not silently become profile authorization"
    );
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/posts/{private_post}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND,
        "approval must not silently become post authorization"
    );
    let timeline =
        json_body(send(&state, Method::GET, "/v1/timeline", app_id, alice, None).await).await;
    assert!(
        !timeline
            .as_array()
            .expect("timeline")
            .iter()
            .any(|post| post["id"] == private_post.to_string())
    );

    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/follows/{bob}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(follow_count(&state, app_id, alice, bob).await, 0);
    assert_eq!(
        approval_count(&state, app_id, alice, bob).await,
        0,
        "unfollowing must invalidate approval so a later follow requires fresh consent"
    );

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/follows/{alice}"),
            app_id,
            carol,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(follow_count(&state, app_id, carol, alice).await, 1);
    assert_eq!(approval_count(&state, app_id, carol, alice).await, 0);
    assert_eq!(
        accept_follow(&state, app_id, alice, carol).await.status(),
        StatusCode::NOT_FOUND,
        "a unilateral public follow must never masquerade as an approved request"
    );

    assert_eq!(
        request_follow(&state, app_id, carol, alice).await.status(),
        StatusCode::NO_CONTENT,
        "an existing public follower may still request explicit approval"
    );
    assert_eq!(pending_count(&state, app_id, carol, alice).await, 1);
    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/follows/{alice}"),
            app_id,
            carol,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(follow_count(&state, app_id, carol, alice).await, 0);
    assert_eq!(
        pending_count(&state, app_id, carol, alice).await,
        0,
        "unfollow must cancel a same-direction pending approval request"
    );
    assert_eq!(
        accept_follow(&state, app_id, alice, carol).await.status(),
        StatusCode::NOT_FOUND,
        "a target must not be able to accept stale consent after a newer unfollow"
    );

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/follows/{alice}"),
            app_id,
            carol,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request_follow(&state, app_id, carol, alice).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        accept_follow(&state, app_id, alice, carol).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(approval_count(&state, app_id, carol, alice).await, 1);
    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/follow-approvals/{carol}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(approval_count(&state, app_id, carol, alice).await, 0);
    assert_eq!(
        follow_count(&state, app_id, carol, alice).await,
        0,
        "revoking approval must remove the approved follow edge"
    );
    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/follow-approvals/{carol}"),
            app_id,
            alice,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT,
        "revoking an already absent approval must be idempotent"
    );

    assert_eq!(
        request_follow(&state, app_id, alice, carol).await.status(),
        StatusCode::NO_CONTENT
    );
    for _ in 0..2 {
        assert_eq!(
            send(
                &state,
                Method::DELETE,
                &format!("/v1/follow-requests/outgoing/{carol}"),
                app_id,
                alice,
                None,
            )
            .await
            .status(),
            StatusCode::NO_CONTENT,
            "cancelling an outgoing request must be idempotent"
        );
    }

    assert_eq!(
        request_follow(&state, app_id, carol, bob).await.status(),
        StatusCode::NO_CONTENT
    );
    for _ in 0..2 {
        assert_eq!(
            send(
                &state,
                Method::DELETE,
                &format!("/v1/follow-requests/incoming/{carol}"),
                app_id,
                bob,
                None,
            )
            .await
            .status(),
            StatusCode::NO_CONTENT,
            "declining a request must be idempotent"
        );
    }

    assert_eq!(
        request_follow(&state, app_id, carol, bob).await.status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(pending_count(&state, app_id, carol, bob).await, 1);
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/blocks/{carol}"),
            app_id,
            bob,
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        pending_count(&state, app_id, carol, bob).await,
        0,
        "blocking must remove pending requests in either direction"
    );
    assert_eq!(
        request_follow(&state, app_id, carol, bob).await.status(),
        StatusCode::NOT_FOUND,
        "a block must prevent new follow requests"
    );

    assert_eq!(
        request_follow(&state, app_id, alice, alice).await.status(),
        StatusCode::BAD_REQUEST
    );
}

async fn upsert_profile(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    name: &str,
    visibility: &str,
) {
    assert_eq!(
        send(
            state,
            Method::PUT,
            "/v1/profiles/me",
            app_id,
            user_id,
            Some(json!({ "displayName": name, "visibility": visibility })),
        )
        .await
        .status(),
        StatusCode::OK
    );
}

async fn create_post(
    state: &AppState,
    app_id: Uuid,
    user_id: Uuid,
    body: &str,
    visibility: &str,
) -> Uuid {
    let response = send(
        state,
        Method::POST,
        "/v1/posts",
        app_id,
        user_id,
        Some(json!({ "body": body, "visibility": visibility })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    Uuid::parse_str(body["id"].as_str().expect("post id")).expect("post id should be UUID")
}

async fn request_follow(state: &AppState, app_id: Uuid, requester: Uuid, target: Uuid) -> Response {
    send(
        state,
        Method::PUT,
        &format!("/v1/follow-requests/outgoing/{target}"),
        app_id,
        requester,
        None,
    )
    .await
}

async fn accept_follow(state: &AppState, app_id: Uuid, target: Uuid, requester: Uuid) -> Response {
    send(
        state,
        Method::PUT,
        &format!("/v1/follow-requests/incoming/{requester}/accept"),
        app_id,
        target,
        None,
    )
    .await
}

async fn follow_count(state: &AppState, app_id: Uuid, follower: Uuid, followed: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM follows WHERE app_id = $1 AND follower_id = $2 AND followed_id = $3",
    )
    .bind(app_id)
    .bind(follower)
    .bind(followed)
    .fetch_one(&state.pool)
    .await
    .expect("follow count")
}

async fn pending_count(state: &AppState, app_id: Uuid, requester: Uuid, target: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM follow_requests WHERE app_id = $1 AND requester_id = $2 AND target_id = $3",
    )
    .bind(app_id)
    .bind(requester)
    .bind(target)
    .fetch_one(&state.pool)
    .await
    .expect("follow request count")
}

async fn approval_count(state: &AppState, app_id: Uuid, requester: Uuid, target: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM follow_approvals WHERE app_id = $1 AND requester_id = $2 AND target_id = $3",
    )
    .bind(app_id)
    .bind(requester)
    .bind(target)
    .fetch_one(&state.pool)
    .await
    .expect("follow approval count")
}

async fn json_body(response: Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response should be JSON")
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
    let request_body = if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    app(state.clone())
        .oneshot(builder.body(request_body).expect("request should build"))
        .await
        .expect("router should respond")
}
