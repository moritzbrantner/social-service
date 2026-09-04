mod support;

use axum::http::Method;

#[tokio::test]
async fn list_followers_returns_feature_disabled() {
    let method = Method::GET;
    let uri = "/v1/follows/00000000-0000-0000-0000-000000000030/followers";
    support::assert_feature_disabled(method, uri, "follows").await;
}
