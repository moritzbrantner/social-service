use std::{collections::BTreeSet, time::Duration};

use axum::{
    body::Body,
    extract::{MatchedPath, Request},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
};
use serde_json::Value;
use social_service::{app, features::FeatureSet, state::AppState};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn declared_operations() -> BTreeSet<(String, String)> {
    let inventory: Value = serde_json::from_str(include_str!("../openapi.json"))
        .expect("operation inventory should be valid JSON");
    inventory["paths"]
        .as_object()
        .expect("inventory should contain paths")
        .iter()
        .flat_map(|(path, methods)| {
            methods
                .as_object()
                .expect("path should contain operations")
                .keys()
                .map(|method| (path.clone(), method.to_uppercase()))
        })
        .collect()
}

// This guard supports the repository's current literal route registrations only.
// Runtime probes below independently exercise the composed public app.
fn literal_operations(source: &str, prefix: &str) -> BTreeSet<(String, String)> {
    let mut operations = BTreeSet::new();
    for declaration in source.split(".route(").skip(1) {
        let declaration = declaration.trim_start();
        assert!(
            declaration.starts_with('"'),
            "dynamic route needs an inventory adapter"
        );
        let (path, handler) = declaration[1..]
            .split_once('"')
            .expect("literal route should close");
        let handler = handler
            .trim_start()
            .strip_prefix(',')
            .expect("route should have a handler")
            .trim_start();
        let mut found = false;
        for method in [
            "get", "post", "put", "delete", "patch", "head", "options", "trace",
        ] {
            if handler.starts_with(&format!("{method}("))
                || handler.contains(&format!(".{method}("))
            {
                found = true;
                assert!(
                    operations.insert((format!("{prefix}{path}"), method.to_uppercase())),
                    "duplicate route operation"
                );
            }
        }
        assert!(
            found,
            "unsupported registration must not silently disappear"
        );
    }
    operations
}

#[test]
fn inventory_matches_literal_public_registrations() {
    let mut registered = literal_operations(include_str!("../src/lib.rs"), "");
    registered.extend(literal_operations(
        include_str!("../src/routes/mod.rs"),
        "/v1",
    ));
    assert_eq!(declared_operations(), registered);
}

async fn record_matched_path(request: Request, next: Next) -> axum::response::Response {
    let path = request.extensions().get::<MatchedPath>().map(|path| {
        HeaderValue::from_str(path.as_str()).expect("matched path should be a valid header")
    });
    let mut response = next.run(request).await;
    if let Some(path) = path {
        response
            .headers_mut()
            .insert("x-contract-matched-path", path);
    }
    response
}

#[tokio::test]
async fn declared_operations_reach_the_composed_public_router() {
    let pool = PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(10))
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/social_service")
        .expect("test database URL should be valid");
    let state = AppState::new(
        pool,
        FeatureSet::from_csv("").expect("feature set should be valid"),
    )
    .with_readiness_timeout(Duration::from_millis(20));
    let router = app(state).layer(middleware::from_fn(record_matched_path));
    for (path, method) in declared_operations() {
        let mut uri = path.clone();
        while let Some(start) = uri.find('{') {
            let end = uri[start..].find('}').expect("parameter should close") + start;
            uri.replace_range(start..=end, "00000000-0000-0000-0000-000000000010");
        }
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method.as_str())
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("public router should respond");
        assert_eq!(
            response
                .headers()
                .get("x-contract-matched-path")
                .and_then(|value| value.to_str().ok()),
            Some(path.as_str()),
            "{method} {path}"
        );
        assert_ne!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
    }
}
