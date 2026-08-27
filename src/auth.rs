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

fn required_uuid(headers: &HeaderMap, name: &HeaderName) -> Result<Uuid, ApiError> {
    let value = headers
        .get(name)
        .ok_or_else(|| ApiError::BadRequest(format!("missing `{name}` header")))?
        .to_str()
        .map_err(|_| ApiError::BadRequest(format!("invalid `{name}` header")))?;
    value
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("`{name}` must be a UUID")))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use uuid::Uuid;

    use super::RequestContext;

    #[test]
    fn parses_request_context() {
        let app = Uuid::new_v4();
        let user = Uuid::new_v4();
        let mut headers = HeaderMap::new();
        headers.insert("x-app-id", HeaderValue::from_str(&app.to_string()).expect("UUID header"));
        headers.insert("x-user-id", HeaderValue::from_str(&user.to_string()).expect("UUID header"));

        let context = RequestContext::from_headers(&headers).expect("valid headers");
        assert_eq!(context.app_id.0, app);
        assert_eq!(context.user_id.0, user);
    }
}
