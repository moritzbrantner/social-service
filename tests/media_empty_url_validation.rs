#[path = "support/validation.rs"]
mod validation_support;

use validation_support::assert_validation_case;

#[tokio::test]
async fn media_empty_url_returns_bad_request() {
    assert_validation_case("media-empty-url").await;
}
