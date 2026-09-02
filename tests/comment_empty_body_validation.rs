#[path = "support/validation.rs"]
mod validation_support;

use validation_support::assert_validation_case;

#[tokio::test]
async fn comment_empty_body_returns_bad_request() {
    assert_validation_case("comment-empty-body").await;
}
