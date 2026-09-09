mod support;

use axum::http::Method;

#[tokio::test]
async fn list_pinned_messages_returns_feature_disabled() {
    let method = Method::GET;
    let uri = "/v1/conversations/00000000-0000-0000-0000-000000000001/pins";
    support::assert_feature_disabled(method, uri, "chat").await;
}
