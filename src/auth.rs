use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::Response;
use subtle::ConstantTimeEq;

use crate::error::ApiError;

pub async fn require_bearer(
    State(token): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let authorized = bearer_token(request.headers())
        .is_some_and(|provided| bool::from(provided.as_bytes().ct_eq(token.as_bytes())));
    if !authorized {
        return Err(ApiError::Unauthorized);
    }
    Ok(next.run(request).await)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn token_of(value: &'static str) -> Option<String> {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static(value));
        bearer_token(&headers).map(str::to_owned)
    }

    #[test]
    fn extracts_bearer_token() {
        assert_eq!(token_of("Bearer abc").as_deref(), Some("abc"));
        assert_eq!(token_of("bearer  abc ").as_deref(), Some("abc"));
        assert_eq!(token_of("Basic abc"), None);
        assert_eq!(token_of("abc"), None);
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }
}
