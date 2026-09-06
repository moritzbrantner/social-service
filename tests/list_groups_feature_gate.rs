mod support;

use axum::http::Method;

#[tokio::test]
async fn list_groups_returns_feature_disabled() {
    let method = Method::GET;
    let uri = "/v1/groups";
    support::assert_feature_disabled(method, uri, "groups").await;
}
