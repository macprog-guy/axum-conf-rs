//! Middleware for recording authenticated user identity to the tracing span.
//!
//! This module provides middleware that runs after authentication to record
//! the authenticated username to the current tracing span's `user` field.

use axum::{body::Body, extract::Request, middleware::Next, response::Response};

/// Records the authenticated username to the current tracing span.
///
/// This middleware should run AFTER authentication middleware to ensure
/// the user identity is available in request extensions. It checks for
/// `AuthenticatedIdentity` in request extensions, which is inserted by
/// any of the supported authentication methods (Basic Auth, API Key, OIDC, ProxyOidc).
///
/// If no authenticated user is found, the span's `user` field remains empty.
pub(crate) async fn record_user_to_span(request: Request<Body>, next: Next) -> Response {
    let username = get_username_from_request(&request);

    if let Some(user) = username {
        tracing::Span::current().record("user", user.as_str());
    }

    next.run(request).await
}

/// Extracts the username from request extensions.
///
/// Uses `preferred_username` if available, otherwise falls back to `user`.
fn get_username_from_request(request: &Request<Body>) -> Option<String> {
    request
        .extensions()
        .get::<crate::AuthenticatedIdentity>()
        .map(|id| {
            id.preferred_username
                .clone()
                .unwrap_or_else(|| id.user.clone())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthMethod, AuthenticatedIdentity};

    #[test]
    fn test_get_username_with_preferred() {
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(AuthenticatedIdentity {
            method: AuthMethod::BasicAuth,
            user: "user-id".to_string(),
            email: None,
            groups: vec![],
            preferred_username: Some("display-name".to_string()),
            access_token: None,
        });

        let username = get_username_from_request(&request);
        assert_eq!(username, Some("display-name".to_string()));
    }

    #[test]
    fn test_get_username_without_preferred() {
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(AuthenticatedIdentity {
            method: AuthMethod::ApiKey,
            user: "api-service".to_string(),
            email: None,
            groups: vec![],
            preferred_username: None,
            access_token: None,
        });

        let username = get_username_from_request(&request);
        assert_eq!(username, Some("api-service".to_string()));
    }

    #[test]
    fn test_get_username_no_auth() {
        let request = Request::new(Body::empty());
        let username = get_username_from_request(&request);
        assert_eq!(username, None);
    }
}
