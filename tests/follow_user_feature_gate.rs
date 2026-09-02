mod support;

use axum::http::Method;

#[tokio::test]
async fn follow_user_returns_feature_disabled() {
    support::assert_feature_disabled(
        Method::PUT,
        "/v1/follows/00000000-0000-0000-0000-000000000030",
        "follows",
    )
    .await;
}
