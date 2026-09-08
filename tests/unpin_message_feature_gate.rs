mod support;

use axum::http::Method;

#[tokio::test]
async fn unpin_message_returns_feature_disabled() {
    let method = Method::DELETE;
    let uri = "/v1/conversations/00000000-0000-0000-0000-000000000001/pins/00000000-0000-0000-0000-000000000002";
    support::assert_feature_disabled(method, uri, "chat").await;
}
