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
async fn approved_follower_audiences_use_durable_approval_across_post_surfaces() {
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
        FeatureSet::from_csv("posts,comments,reactions,follows,follow_requests,saves,blocks")
            .expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
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
                Some(user_id),
                Some(json!({ "displayName": name })),
            )
            .await
            .status(),
            StatusCode::OK
        );
    }

    let approved_post = json_body(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            Some(bob),
            Some(json!({
                "body": "approved audience",
                "audience": "approved_followers"
            })),
        )
        .await,
    )
    .await;
    let approved_post_id = post_id(&approved_post);
    assert_eq!(approved_post["audience"], "approved_followers");
    assert_eq!(
        approved_post["visibility"], "private",
        "legacy visibility must remain a deterministic non-public projection"
    );

    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/posts/{approved_post_id}"),
            app_id,
            None,
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND,
        "approved-follower posts are not public"
    );
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/posts/{approved_post_id}"),
            app_id,
            Some(alice),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/follows/{bob}"),
            app_id,
            Some(carol),
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
            &format!("/v1/posts/{approved_post_id}"),
            app_id,
            Some(carol),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND,
        "an ordinary unilateral follow must not grant audience access"
    );
    assert!(!timeline_contains(&state, app_id, carol, approved_post_id).await);

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/follow-requests/outgoing/{bob}"),
            app_id,
            Some(alice),
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
            &format!("/v1/follow-requests/incoming/{alice}/accept"),
            app_id,
            Some(bob),
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    let visible = send(
        &state,
        Method::GET,
        &format!("/v1/posts/{approved_post_id}"),
        app_id,
        Some(alice),
        None,
    )
    .await;
    assert_eq!(visible.status(), StatusCode::OK);
    assert!(timeline_contains(&state, app_id, alice, approved_post_id).await);

    let comment = json_body(
        send(
            &state,
            Method::POST,
            &format!("/v1/posts/{approved_post_id}/comments"),
            app_id,
            Some(alice),
            Some(json!({ "body": "approved comment" })),
        )
        .await,
    )
    .await;
    assert!(comment["id"].is_string());
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/posts/{approved_post_id}/comments"),
            app_id,
            Some(alice),
            None,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/reactions/post/{approved_post_id}/like"),
            app_id,
            Some(alice),
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
            &format!("/v1/posts/{approved_post_id}/save"),
            app_id,
            Some(alice),
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
            Some(alice),
            None,
        )
        .await,
    )
    .await;
    assert!(
        saved
            .as_array()
            .expect("saved posts")
            .iter()
            .any(|post| post["id"] == approved_post_id.to_string())
    );

    assert_eq!(
        send(
            &state,
            Method::DELETE,
            &format!("/v1/follow-approvals/{alice}"),
            app_id,
            Some(bob),
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
            &format!("/v1/posts/{approved_post_id}"),
            app_id,
            Some(alice),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert!(!timeline_contains(&state, app_id, alice, approved_post_id).await);
    let saved = json_body(
        send(
            &state,
            Method::GET,
            "/v1/saved-posts",
            app_id,
            Some(alice),
            None,
        )
        .await,
    )
    .await;
    assert!(
        !saved
            .as_array()
            .expect("saved posts")
            .iter()
            .any(|post| post["id"] == approved_post_id.to_string()),
        "a stale private save must not bypass the current audience policy"
    );
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/reactions/post/{approved_post_id}"),
            app_id,
            Some(alice),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let owner_only = json_body(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            Some(bob),
            Some(json!({ "body": "legacy private", "visibility": "private" })),
        )
        .await,
    )
    .await;
    assert_eq!(owner_only["audience"], "owner_only");
    assert_eq!(owner_only["visibility"], "private");
    assert_eq!(
        send(
            &state,
            Method::GET,
            &format!("/v1/posts/{}", post_id(&owner_only)),
            app_id,
            Some(alice),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let public = json_body(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            Some(bob),
            Some(json!({ "body": "legacy public", "visibility": "public" })),
        )
        .await,
    )
    .await;
    assert_eq!(public["audience"], "public");
    assert_eq!(public["visibility"], "public");

    assert_eq!(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            Some(bob),
            Some(json!({
                "body": "conflicting policy",
                "visibility": "public",
                "audience": "approved_followers"
            })),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    assert_eq!(
        send(
            &state,
            Method::PUT,
            &format!("/v1/follow-requests/outgoing/{bob}"),
            app_id,
            Some(alice),
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
            &format!("/v1/follow-requests/incoming/{alice}/accept"),
            app_id,
            Some(bob),
            None,
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    let without_approvals = AppState::new(
        pool,
        FeatureSet::from_csv("posts,follows").expect("minimal post deployment should resolve"),
    );
    assert_eq!(
        send(
            &without_approvals,
            Method::GET,
            &format!("/v1/posts/{approved_post_id}"),
            app_id,
            Some(alice),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND,
        "approved-follower content must fail closed when approval capability is disabled"
    );
    assert!(!timeline_contains(&without_approvals, app_id, alice, approved_post_id).await);
    assert_eq!(
        send(
            &without_approvals,
            Method::GET,
            &format!("/v1/posts/{approved_post_id}"),
            app_id,
            Some(bob),
            None,
        )
        .await
        .status(),
        StatusCode::OK,
        "the author must retain access independently of optional audience relationships"
    );
}

async fn timeline_contains(state: &AppState, app_id: Uuid, user_id: Uuid, post_id: Uuid) -> bool {
    let timeline = json_body(
        send(
            state,
            Method::GET,
            "/v1/timeline",
            app_id,
            Some(user_id),
            None,
        )
        .await,
    )
    .await;
    timeline
        .as_array()
        .expect("timeline")
        .iter()
        .any(|post| post["id"] == post_id.to_string())
}

fn post_id(value: &Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().expect("post id")).expect("post id should be UUID")
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
