mod support;

use axum::http::Method;

#[tokio::test]
async fn list_conversations_returns_feature_disabled() {
    support::assert_feature_disabled(
        Method::GET,
        "/v1/conversations",
        "chat",
    )
    .await;
}
