use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use social_service::{app, features::FeatureSet, state::AppState};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn test_state(features: &str) -> AppState {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@localhost/social_service")
        .expect("test database URL should be valid");
    let features = FeatureSet::from_csv(features).expect("test feature set should be valid");
    AppState::new(pool, features)
}

pub async fn assert_feature_disabled(method: Method, uri: &str, feature: &str) {
    let response = app(test_state(""))
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    let document: serde_json::Value =
        serde_json::from_slice(&body).expect("feature gate response should be valid JSON");

    assert_eq!(
        document,
        json!({
            "error": "feature_disabled",
            "message": format!("feature `{feature}` is disabled"),
        })
    );
}
