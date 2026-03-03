//! Authenticated identity types shared across all authentication methods.

use crate::utils::Sensitive;
use axum::extract::FromRequestParts;
use http::{StatusCode, request::Parts};

/// The authentication method used for a request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    /// HTTP Basic Auth (RFC 7617).
    BasicAuth,
    /// API Key authentication.
    ApiKey,
    /// OIDC/Keycloak JWT authentication.
    Oidc,
    /// Proxy-based OIDC authentication (e.g., oauth2-proxy).
    ProxyOidc,
}

/// Identity of an authenticated user or service.
///
/// This struct is inserted into request extensions after successful authentication.
/// Use the Axum extractor to access it in handlers:
///
/// ```rust,ignore
/// use axum_conf::AuthenticatedIdentity;
///
/// // Required - returns 401 if not authenticated
/// async fn handler(identity: AuthenticatedIdentity) -> String {
///     format!("Hello, {}!", identity.user)
/// }
///
/// // Optional - returns None if not authenticated
/// async fn optional_handler(identity: Option<AuthenticatedIdentity>) -> String {
///     match identity {
///         Some(id) => format!("Hello, {}!", id.user),
///         None => "Hello, anonymous!".to_string(),
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    /// The authentication method used.
    pub method: AuthMethod,
    /// The authenticated user identifier.
    pub user: String,
    /// Email address of the authenticated user (optional).
    pub email: Option<String>,
    /// Groups the authenticated user belongs to.
    pub groups: Vec<String>,
    /// Preferred username for display purposes (optional).
    pub preferred_username: Option<String>,
    /// Access token (optional, wrapped in Sensitive to prevent logging).
    pub access_token: Option<Sensitive<String>>,
}

impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedIdentity {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))
    }
}
