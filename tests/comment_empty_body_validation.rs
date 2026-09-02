mod support;

use support::{ValidationCase, assert_validation_case};

#[tokio::test]
async fn comment_empty_body_returns_bad_request() {
    assert_validation_case(ValidationCase::CommentEmptyBody).await;
}
