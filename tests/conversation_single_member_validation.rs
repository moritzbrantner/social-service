#[path = "support/validation.rs"]
mod validation_support;

use validation_support::assert_validation_case;

#[tokio::test]
async fn conversation_single_member_returns_bad_request() {
    assert_validation_case("conversation-single-member").await;
}
