mod support;

use support::{ValidationCase, assert_validation_case};

#[tokio::test]
async fn conversation_single_member_returns_bad_request() {
    assert_validation_case(ValidationCase::ConversationSingleMember).await;
}
