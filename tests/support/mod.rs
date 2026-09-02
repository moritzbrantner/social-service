use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use social_service::{app, features::FeatureSet, state::AppState};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const APP_ID: &str = "00000000-0000-0000-0000-000000000001";
const USER_ID: &str = "00000000-0000-0000-0000-000000000002";

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

#[derive(Clone, Copy)]
pub enum ValidationCase {
    ProfileEmptyDisplayName,
    MediaEmptyUrl,
    PostEmptyBody,
    CommentEmptyBody,
    ConversationSingleMember,
}

impl ValidationCase {
    fn request(
        self,
    ) -> (
        &'static str,
        Method,
        &'static str,
        &'static str,
        bool,
        &'static str,
    ) {
        match self {
            Self::ProfileEmptyDisplayName => (
                "profiles",
                Method::PUT,
                "/v1/profiles/me",
                r#"{"displayName":"   ","bio":null,"avatarMediaId":null}"#,
                false,
                "bad request: displayName must contain 1-120 characters",
            ),
            Self::MediaEmptyUrl => (
                "profiles,media",
                Method::POST,
                "/v1/media",
                r#"{"url":"   ","contentType":"image/png"}"#,
                true,
                "bad request: url must contain 1-4096 characters",
            ),
            Self::PostEmptyBody => (
                "profiles,posts",
                Method::POST,
                "/v1/posts",
                r#"{"body":"   ","mediaIds":[]}"#,
                true,
                "bad request: body must contain 1-10000 characters",
            ),
            Self::CommentEmptyBody => (
                "profiles,posts,comments",
                Method::POST,
                "/v1/posts/00000000-0000-0000-0000-000000000020/comments",
                r#"{"body":"   "}"#,
                false,
                "bad request: body must contain 1-5000 characters",
            ),
            Self::ConversationSingleMember => (
                "profiles,chat",
                Method::POST,
                "/v1/conversations",
                r#"{"memberIds":[]}"#,
                true,
                "bad request: a conversation must contain 2-100 unique members",
            ),
        }
    }
}

pub async fn assert_validation_case(case: ValidationCase) {
    let (features, method, uri, body, authenticated, message) = case.request();
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if authenticated {
        request = request
            .header("x-app-id", APP_ID)
            .header("x-user-id", USER_ID);
    }

    let response = app(test_state(features))
        .oneshot(
            request
                .body(Body::from(body))
                .expect("validation request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body should be readable")
        .to_bytes();
    let document: serde_json::Value =
        serde_json::from_slice(&body).expect("validation response should be valid JSON");

    assert_eq!(
        document,
        json!({
            "error": "bad_request",
            "message": message,
        })
    );
}
