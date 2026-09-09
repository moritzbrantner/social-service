mod support;

use axum::http::Method;

#[tokio::test]
async fn list_saved_posts_returns_feature_disabled() {
    let method = Method::GET;
    let uri = "/v1/saved-posts";
    support::assert_feature_disabled(method, uri, "saves").await;
}
