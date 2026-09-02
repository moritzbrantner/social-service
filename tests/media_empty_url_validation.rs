mod support;

use support::{ValidationCase, assert_validation_case};

#[tokio::test]
async fn media_empty_url_returns_bad_request() {
    assert_validation_case(ValidationCase::MediaEmptyUrl).await;
}
