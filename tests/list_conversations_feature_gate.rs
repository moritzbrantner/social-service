mod support;

use axum::http::Method;

#[tokio::test]
async fn list_conversations_returns_feature_disabled() {
    let method = Method::GET;
    let uri = "/v1/conversations";
    support::assert_feature_disabled(method, uri, "chat").await;
}
