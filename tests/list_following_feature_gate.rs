mod support;

use axum::http::Method;

#[tokio::test]
async fn list_following_returns_feature_disabled() {
    let method = Method::GET;
    let uri = "/v1/follows/00000000-0000-0000-0000-000000000030/following";
    support::assert_feature_disabled(method, uri, "follows").await;
}
