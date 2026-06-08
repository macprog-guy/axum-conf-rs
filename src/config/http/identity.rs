//! Authenticated identity types shared across all authentication methods.

use crate::utils::Sensitive;
use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use http::{StatusCode, request::Parts};
use std::convert::Infallible;
use std::sync::Arc;

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
    /// Application-specific roles assigned to the authenticated user.
    pub roles: Vec<String>,
    /// Preferred username for display purposes (optional).
    pub preferred_username: Option<String>,
    /// Access token (optional, wrapped in Sensitive to prevent logging).
    pub access_token: Option<Sensitive<String>>,
}

impl AuthenticatedIdentity {
    /// Looks up the shared identity from request extensions.
    ///
    /// The built-in auth middleware stores `Arc<AuthenticatedIdentity>` so that
    /// role checks, span recording, and handler extraction share one allocation.
    /// A bare `AuthenticatedIdentity` (e.g. inserted by custom middleware) is
    /// accepted as a fallback for backwards compatibility.
    pub(crate) fn arc_from_extensions(
        extensions: &http::Extensions,
    ) -> Option<Arc<AuthenticatedIdentity>> {
        if let Some(arc) = extensions.get::<Arc<AuthenticatedIdentity>>() {
            Some(Arc::clone(arc))
        } else {
            extensions
                .get::<AuthenticatedIdentity>()
                .map(|id| Arc::new(id.clone()))
        }
    }

    /// Returns true if an authenticated identity is present in the extensions.
    #[cfg(feature = "keycloak")]
    pub(crate) fn present_in(extensions: &http::Extensions) -> bool {
        extensions.get::<Arc<AuthenticatedIdentity>>().is_some()
            || extensions.get::<AuthenticatedIdentity>().is_some()
    }

    /// Borrows the identity from request extensions without cloning.
    ///
    /// Useful for read-only paths (e.g. recording the user to a tracing span)
    /// that neither need ownership nor a refcount bump.
    pub(crate) fn from_extensions_ref(
        extensions: &http::Extensions,
    ) -> Option<&AuthenticatedIdentity> {
        if let Some(arc) = extensions.get::<Arc<AuthenticatedIdentity>>() {
            Some(arc.as_ref())
        } else {
            extensions.get::<AuthenticatedIdentity>()
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedIdentity {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Self::arc_from_extensions(&parts.extensions)
            .map(|arc| (*arc).clone())
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for AuthenticatedIdentity {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Option<Self>, Self::Rejection> {
        Ok(Self::arc_from_extensions(&parts.extensions).map(|arc| (*arc).clone()))
    }
}
