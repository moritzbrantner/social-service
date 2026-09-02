mod support;

use support::{ValidationCase, assert_validation_case};

#[tokio::test]
async fn profile_empty_display_name_returns_bad_request() {
    assert_validation_case(ValidationCase::ProfileEmptyDisplayName).await;
}
