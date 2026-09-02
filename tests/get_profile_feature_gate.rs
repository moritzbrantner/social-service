mod support;

use axum::http::Method;

#[tokio::test]
async fn get_profile_returns_feature_disabled() {
    support::assert_feature_disabled(
        Method::GET,
        "/v1/profiles/00000000-0000-0000-0000-000000000010",
        "profiles",
    )
    .await;
}
