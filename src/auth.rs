use axum::http::{HeaderMap, HeaderName};
use uuid::Uuid;

use crate::error::ApiError;

const APP_ID: HeaderName = HeaderName::from_static("x-app-id");
const USER_ID: HeaderName = HeaderName::from_static("x-user-id");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestContext {
    pub app_id: AppId,
    pub user_id: UserId,
}

impl RequestContext {
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, ApiError> {
        Ok(Self {
            app_id: AppId(required_uuid(headers, &APP_ID)?),
            user_id: UserId(required_uuid(headers, &USER_ID)?),
        })
    }
}

pub fn app_id(headers: &HeaderMap) -> Result<AppId, ApiError> {
    Ok(AppId(required_uuid(headers, &APP_ID)?))
}

pub fn optional_user_id(headers: &HeaderMap) -> Result<Option<UserId>, ApiError> {
    headers
        .get(&USER_ID)
        .map(|value| parse_uuid(value, &USER_ID).map(UserId))
        .transpose()
}

fn required_uuid(headers: &HeaderMap, name: &HeaderName) -> Result<Uuid, ApiError> {
    let value = headers
        .get(name)
        .ok_or_else(|| ApiError::BadRequest(format!("missing `{name}` header")))?;
    parse_uuid(value, name)
}

fn parse_uuid(value: &axum::http::HeaderValue, name: &HeaderName) -> Result<Uuid, ApiError> {
    value
        .to_str()
        .map_err(|_| ApiError::BadRequest(format!("invalid `{name}` header")))?
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("`{name}` must be a UUID")))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use uuid::Uuid;

    use super::{RequestContext, optional_user_id};

    #[test]
    fn parses_request_context() {
        let app = Uuid::new_v4();
        let user = Uuid::new_v4();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-app-id",
            HeaderValue::from_str(&app.to_string()).expect("UUID header"),
        );
        headers.insert(
            "x-user-id",
            HeaderValue::from_str(&user.to_string()).expect("UUID header"),
        );

        let context = RequestContext::from_headers(&headers).expect("valid headers");
        assert_eq!(context.app_id.0, app);
        assert_eq!(context.user_id.0, user);
    }

    #[test]
    fn optional_user_id_allows_public_reads_without_a_user() {
        assert_eq!(
            optional_user_id(&HeaderMap::new()).expect("missing optional user is valid"),
            None
        );
    }

    #[test]
    fn optional_user_id_still_validates_a_present_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", HeaderValue::from_static("not-a-uuid"));
        let error = optional_user_id(&headers).expect_err("invalid UUID must fail");
        assert_eq!(error.to_string(), "bad request: `x-user-id` must be a UUID");
    }
}
