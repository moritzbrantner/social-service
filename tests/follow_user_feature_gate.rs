mod support;

use axum::http::Method;

#[tokio::test]
async fn follow_user_returns_feature_disabled() {
    let method = Method::PUT;
    let uri = "/v1/follows/00000000-0000-0000-0000-000000000030";
    support::assert_feature_disabled(method, uri, "follows").await;
}
