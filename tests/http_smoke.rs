use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use social_service::{
    app,
    features::FeatureSet,
    state::AppState,
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn test_state(features: &str) -> AppState {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@localhost/social_service")
        .expect("test database URL should be valid");
    let features = FeatureSet::from_csv(features).expect("test feature set should be valid");
    AppState::new(pool, features)
}

#[tokio::test]
async fn health_is_reachable_through_the_composed_router() {
    let response = app(test_state(""))
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    assert_eq!(body.as_ref(), b"ok");
}

#[tokio::test]
async fn feature_configuration_is_exposed_through_the_http_boundary() {
    let response = app(test_state("chat,profiles"))
        .oneshot(
            Request::builder()
                .uri("/v1/features")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    let document: serde_json::Value =
        serde_json::from_slice(&body).expect("feature response should be valid JSON");

    assert_eq!(document, json!({ "enabled": ["profiles", "chat"] }));
}
