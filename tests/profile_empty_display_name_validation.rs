#[path = "support/validation.rs"]
mod validation_support;

use validation_support::assert_validation_case;

#[tokio::test]
async fn profile_empty_display_name_returns_bad_request() {
    assert_validation_case("profile-empty-display-name").await;
}
