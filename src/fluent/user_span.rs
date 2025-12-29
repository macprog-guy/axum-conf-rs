//! Middleware for recording authenticated user identity to the tracing span.
//!
//! This module provides middleware that runs after authentication to record
//! the authenticated username to the current tracing span's `user` field.

use axum::{body::Body, extract::Request, middleware::Next, response::Response};

/// Records the authenticated username to the current tracing span.
///
/// This middleware should run AFTER authentication middleware to ensure
/// the user identity is available in request extensions. It checks for:
/// - `AuthenticatedIdentity` from Basic Auth (if `basic-auth` feature enabled)
/// - `KeycloakToken` from OIDC (if `keycloak` feature enabled)
///
/// If no authenticated user is found, the span's `user` field remains empty.
pub(crate) async fn record_user_to_span(request: Request<Body>, next: Next) -> Response {
    let username = get_username_from_request(&request);

    if let Some(user) = username {
        tracing::Span::current().record("user", user.as_str());
    }

    next.run(request).await
}

/// Extracts the username from request extensions based on available auth methods.
#[allow(unused_variables)]
fn get_username_from_request(request: &Request<Body>) -> Option<String> {
    // Try Basic Auth first (if feature enabled)
    #[cfg(feature = "basic-auth")]
    if let Some(identity) = request.extensions().get::<crate::AuthenticatedIdentity>() {
        return Some(identity.name.clone());
    }

    // Try Keycloak/OIDC (if feature enabled)
    // KeycloakToken<Role, ProfileAndEmail> stores profile data in extra.profile
    #[cfg(feature = "keycloak")]
    if let Some(token) = request
        .extensions()
        .get::<axum_keycloak_auth::decode::KeycloakToken<
            crate::Role,
            axum_keycloak_auth::decode::ProfileAndEmail,
        >>()
    {
        // Use preferred_username if not empty, otherwise fall back to subject
        let username = &token.extra.profile.preferred_username;
        if !username.is_empty() {
            return Some(username.clone());
        }
        // Fall back to subject claim (always present)
        return Some(token.subject.clone());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_get_username_from_basic_auth() {
        use crate::{AuthMethod, AuthenticatedIdentity};

        // Create a request with AuthenticatedIdentity in extensions
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(AuthenticatedIdentity {
            method: AuthMethod::BasicAuth,
            name: "test-user".to_string(),
        });

        let username = get_username_from_request(&request);
        assert_eq!(username, Some("test-user".to_string()));
    }

    #[cfg(feature = "basic-auth")]
    #[test]
    fn test_get_username_from_api_key() {
        use crate::{AuthMethod, AuthenticatedIdentity};

        // Create a request with API key identity
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(AuthenticatedIdentity {
            method: AuthMethod::ApiKey,
            name: "api-service".to_string(),
        });

        let username = get_username_from_request(&request);
        assert_eq!(username, Some("api-service".to_string()));
    }

    #[test]
    fn test_get_username_no_auth() {
        // Create a request without any authentication
        let request = Request::new(Body::empty());

        let username = get_username_from_request(&request);
        assert_eq!(username, None);
    }
}
