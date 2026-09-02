mod support;

use support::{ValidationCase, assert_validation_case};

#[tokio::test]
async fn post_empty_body_returns_bad_request() {
    assert_validation_case(ValidationCase::PostEmptyBody).await;
}
