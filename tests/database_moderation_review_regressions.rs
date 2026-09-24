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
async fn moderation_review_retains_complete_evidence_and_disables_unavailable_actors() {
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
        FeatureSet::from_csv("comments,follow_requests,moderation")
            .expect("test capabilities should resolve"),
    );
    let app_id = Uuid::new_v4();
    let admin_id = Uuid::new_v4();
    let moderator_id = Uuid::new_v4();
    let author_id = Uuid::new_v4();

    for (user_id, display_name) in [
        (admin_id, "Admin"),
        (moderator_id, "Moderator"),
        (author_id, "Author"),
    ] {
        let response = send(
            &state,
            Method::PUT,
            "/v1/profiles/me",
            app_id,
            user_id,
            None,
            Some(json!({ "displayName": display_name })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/roles/{moderator_id}"),
        app_id,
        admin_id,
        Some("roles.manage"),
        Some(json!({ "role": "moderator", "reason": "review duty" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{moderator_id}"),
        app_id,
        admin_id,
        Some("users.restrict"),
        Some(json!({ "state": "suspended", "reason": "temporary suspension" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    for capabilities in [None, Some("reports.read")] {
        let response = send(
            &state,
            Method::GET,
            "/v1/moderation/cases",
            app_id,
            moderator_id,
            capabilities,
            None,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "suspended actors must not retain authority through either persisted roles or trusted capability claims"
        );
    }

    let response = send(
        &state,
        Method::PUT,
        &format!("/v1/moderation/users/{moderator_id}"),
        app_id,
        admin_id,
        Some("users.restrict"),
        Some(json!({ "state": "active", "reason": "restored" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = send(
        &state,
        Method::GET,
        "/v1/moderation/cases",
        app_id,
        moderator_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let audience_post = json_body(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            author_id,
            None,
            Some(json!({
                "body": "approved follower evidence",
                "audience": "approved_followers"
            })),
        )
        .await,
    )
    .await;
    let audience_post_id = id(&audience_post);
    let response = send(
        &state,
        Method::POST,
        "/v1/reports",
        app_id,
        author_id,
        None,
        Some(json!({
            "targetType": "post",
            "targetId": audience_post_id,
            "category": "evidence",
            "idempotencyKey": "audience-snapshot"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let snapshot = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/moderation/content/post/{audience_post_id}"),
            app_id,
            moderator_id,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(snapshot["type"], "post");
    assert_eq!(snapshot["data"]["audience"], "approved_followers");
    assert_eq!(snapshot["data"]["visibility"], "private");
    assert_eq!(snapshot["data"]["mediaIds"], json!([]));

    let public_post = json_body(
        send(
            &state,
            Method::POST,
            "/v1/posts",
            app_id,
            author_id,
            None,
            Some(json!({ "body": "thread evidence" })),
        )
        .await,
    )
    .await;
    let public_post_id = id(&public_post);

    let root = json_body(
        send(
            &state,
            Method::POST,
            &format!("/v1/posts/{public_post_id}/comments"),
            app_id,
            author_id,
            None,
            Some(json!({ "body": "root" })),
        )
        .await,
    )
    .await;
    let root_id = id(&root);
    let child = json_body(
        send(
            &state,
            Method::POST,
            &format!("/v1/posts/{public_post_id}/comments/{root_id}/replies"),
            app_id,
            author_id,
            None,
            Some(json!({ "body": "child" })),
        )
        .await,
    )
    .await;
    let child_id = id(&child);

    for (target_id, key) in [(root_id, "root-snapshot"), (child_id, "child-snapshot")] {
        let response = send(
            &state,
            Method::POST,
            "/v1/reports",
            app_id,
            author_id,
            None,
            Some(json!({
                "targetType": "comment",
                "targetId": target_id,
                "category": "evidence",
                "idempotencyKey": key
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{public_post_id}/comments/{root_id}"),
        app_id,
        author_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let tombstone = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/moderation/content/comment/{root_id}"),
            app_id,
            moderator_id,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(tombstone["type"], "comment");
    assert_eq!(tombstone["data"]["body"], "");
    assert!(
        tombstone["data"]["deletedAt"].as_str().is_some(),
        "live moderation review must preserve the threaded-comment tombstone marker"
    );

    let response = send(
        &state,
        Method::DELETE,
        &format!("/v1/posts/{public_post_id}/comments/{child_id}"),
        app_id,
        author_id,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let retained = json_body(
        send(
            &state,
            Method::GET,
            &format!("/v1/moderation/content/comment/{child_id}"),
            app_id,
            moderator_id,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(retained["type"], "comment");
    assert_eq!(retained["data"]["id"], child_id.to_string());
    assert_eq!(retained["data"]["body"], "child");
    assert_eq!(retained["data"]["parentCommentId"], root_id.to_string());
    assert!(
        retained["data"]["deletedAt"].is_null(),
        "physically deleted content must fall back to the report-time evidence rather than inventing later state"
    );

    let retained_case_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM moderation_cases WHERE app_id = $1 AND target_id = $2 AND target_snapshot IS NOT NULL",
    )
    .bind(app_id)
    .bind(child_id)
    .fetch_one(&pool)
    .await
    .expect("snapshot case count");
    assert_eq!(retained_case_count, 1);
}

fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().expect("resource id")).expect("resource UUID")
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
