use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use social_service::{app, features::FeatureSet, state::AppState};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn readiness_reports_usable_postgres() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("PostgreSQL should be reachable");
    let state = AppState::new(
        pool,
        FeatureSet::from_csv("").expect("test feature set should be valid"),
    );

    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/ready")
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
