mod support;

use axum::http::Method;

#[tokio::test]
async fn put_vote_returns_feature_disabled() {
    let method = Method::PUT;
    let uri = "/v1/votes/post/00000000-0000-0000-0000-000000000020/up";
    support::assert_feature_disabled(method, uri, "votes").await;
}
