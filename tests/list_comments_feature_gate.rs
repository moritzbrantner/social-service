mod support;

use axum::http::Method;

#[tokio::test]
async fn list_comments_returns_feature_disabled() {
    support::assert_feature_disabled(
        Method::GET,
        "/v1/posts/00000000-0000-0000-0000-000000000020/comments",
        "comments",
    )
    .await;
}
