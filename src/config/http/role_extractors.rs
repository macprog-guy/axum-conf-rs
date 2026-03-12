//! Role-based authorization extractors.
//!
//! These extractors gate routes based on the roles present in
//! [`AuthenticatedIdentity`]. Use them alongside any authentication method
//! (OIDC, Basic Auth, API Key, Proxy OIDC) — roles are always available.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use axum_conf::{role, roles, WithRole, AnyRole, AllRoles};
//!
//! // Define roles as marker types
//! role!(Admin => "admin");
//! roles!(EditorOrViewer => "editor", "viewer");
//! roles!(AdminAndEditor => "admin", "editor");
//!
//! // Single role required
//! async fn admin_only(WithRole(identity, _): WithRole<Admin>) -> String {
//!     format!("Admin: {}", identity.user)
//! }
//!
//! // Any of the listed roles
//! async fn flexible(AnyRole(identity, _): AnyRole<EditorOrViewer>) -> String {
//!     format!("Editor or viewer: {}", identity.user)
//! }
//!
//! // All listed roles required
//! async fn strict(AllRoles(identity, _): AllRoles<AdminAndEditor>) -> String {
//!     format!("Admin AND editor: {}", identity.user)
//! }
//! ```

use crate::config::http::identity::AuthenticatedIdentity;
use axum::extract::FromRequestParts;
use http::{request::Parts, StatusCode};
use std::marker::PhantomData;
use std::ops::Deref;

/// A single application role that can be required on a route.
///
/// Implement this trait on a marker type to use with [`WithRole`].
/// The [`role!`] macro provides a convenient way to do this.
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::ApplicationRole;
///
/// struct Admin;
/// impl ApplicationRole for Admin {
///     const ROLE: &'static str = "admin";
/// }
///
/// // Or use the macro:
/// axum_conf::role!(Admin => "admin");
/// ```
pub trait ApplicationRole {
    /// The role name to check against `AuthenticatedIdentity.roles`.
    const ROLE: &'static str;
}

/// A set of application roles that can be required on a route.
///
/// Implement this trait on a marker type to use with [`AnyRole`] or [`AllRoles`].
/// The [`roles!`] macro provides a convenient way to do this.
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::ApplicationRoles;
///
/// struct EditorOrViewer;
/// impl ApplicationRoles for EditorOrViewer {
///     const ROLES: &'static [&'static str] = &["editor", "viewer"];
/// }
///
/// // Or use the macro:
/// axum_conf::roles!(EditorOrViewer => "editor", "viewer");
/// ```
pub trait ApplicationRoles {
    /// The role names to check against `AuthenticatedIdentity.roles`.
    const ROLES: &'static [&'static str];
}

/// Extractor that requires the authenticated user to have a specific role.
///
/// Returns 401 if not authenticated, 403 if the role is missing.
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::{role, WithRole};
///
/// role!(Admin => "admin");
///
/// async fn handler(WithRole(identity, _): WithRole<Admin>) -> String {
///     format!("Hello admin {}!", identity.user)
/// }
/// ```
pub struct WithRole<R: ApplicationRole>(pub AuthenticatedIdentity, pub PhantomData<R>);

impl<R: ApplicationRole> Deref for WithRole<R> {
    type Target = AuthenticatedIdentity;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync, R: ApplicationRole> FromRequestParts<S> for WithRole<R> {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let identity = parts
            .extensions
            .get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))?;

        if identity.roles.iter().any(|r| r == R::ROLE) {
            Ok(WithRole(identity, PhantomData))
        } else {
            Err((StatusCode::FORBIDDEN, "Insufficient role"))
        }
    }
}

/// Extractor that requires the authenticated user to have **any** of the specified roles.
///
/// Returns 401 if not authenticated, 403 if none of the roles match.
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::{roles, AnyRole};
///
/// roles!(EditorOrViewer => "editor", "viewer");
///
/// async fn handler(AnyRole(identity, _): AnyRole<EditorOrViewer>) -> String {
///     format!("Hello {}!", identity.user)
/// }
/// ```
pub struct AnyRole<R: ApplicationRoles>(pub AuthenticatedIdentity, pub PhantomData<R>);

impl<R: ApplicationRoles> Deref for AnyRole<R> {
    type Target = AuthenticatedIdentity;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync, R: ApplicationRoles> FromRequestParts<S> for AnyRole<R> {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let identity = parts
            .extensions
            .get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))?;

        if R::ROLES
            .iter()
            .any(|required| identity.roles.iter().any(|r| r == required))
        {
            Ok(AnyRole(identity, PhantomData))
        } else {
            Err((StatusCode::FORBIDDEN, "Insufficient role"))
        }
    }
}

/// Extractor that requires the authenticated user to have **all** of the specified roles.
///
/// Returns 401 if not authenticated, 403 if any role is missing.
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::{roles, AllRoles};
///
/// roles!(AdminAndEditor => "admin", "editor");
///
/// async fn handler(AllRoles(identity, _): AllRoles<AdminAndEditor>) -> String {
///     format!("Hello {}!", identity.user)
/// }
/// ```
pub struct AllRoles<R: ApplicationRoles>(pub AuthenticatedIdentity, pub PhantomData<R>);

impl<R: ApplicationRoles> Deref for AllRoles<R> {
    type Target = AuthenticatedIdentity;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: Send + Sync, R: ApplicationRoles> FromRequestParts<S> for AllRoles<R> {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let identity = parts
            .extensions
            .get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))?;

        if R::ROLES
            .iter()
            .all(|required| identity.roles.iter().any(|r| r == required))
        {
            Ok(AllRoles(identity, PhantomData))
        } else {
            Err((StatusCode::FORBIDDEN, "Insufficient role"))
        }
    }
}

/// Define a single application role as a marker type.
///
/// Creates a zero-sized struct that implements [`ApplicationRole`].
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::{role, WithRole};
///
/// role!(Admin => "admin");
///
/// async fn handler(WithRole(identity, _): WithRole<Admin>) -> String {
///     format!("Hello {}!", identity.user)
/// }
/// ```
#[macro_export]
macro_rules! role {
    ($name:ident => $role:expr) => {
        pub struct $name;
        impl $crate::ApplicationRole for $name {
            const ROLE: &'static str = $role;
        }
    };
}

/// Define a set of application roles as a marker type.
///
/// Creates a zero-sized struct that implements [`ApplicationRoles`].
/// Use with [`AnyRole`] (user needs at least one) or [`AllRoles`] (user needs all).
///
/// # Example
///
/// ```rust,ignore
/// use axum_conf::{roles, AnyRole, AllRoles};
///
/// roles!(EditorOrViewer => "editor", "viewer");
///
/// // User needs "editor" OR "viewer"
/// async fn any_handler(AnyRole(identity, _): AnyRole<EditorOrViewer>) -> String {
///     format!("Hello {}!", identity.user)
/// }
///
/// // User needs "editor" AND "viewer"
/// async fn all_handler(AllRoles(identity, _): AllRoles<EditorOrViewer>) -> String {
///     format!("Hello {}!", identity.user)
/// }
/// ```
#[macro_export]
macro_rules! roles {
    ($name:ident => $($role:expr),+ $(,)?) => {
        pub struct $name;
        impl $crate::ApplicationRoles for $name {
            const ROLES: &'static [&'static str] = &[$($role),+];
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::http::identity::AuthMethod;
    use axum::{routing::get, Router};
    use http::Request;
    use tower::ServiceExt;

    role!(Admin => "admin");
    roles!(EditorOrViewer => "editor", "viewer");
    roles!(AdminAndEditor => "admin", "editor");

    fn test_identity(roles: Vec<String>) -> AuthenticatedIdentity {
        AuthenticatedIdentity {
            method: AuthMethod::BasicAuth,
            user: "testuser".into(),
            email: None,
            groups: vec![],
            roles,
            preferred_username: None,
            access_token: None,
        }
    }

    async fn with_role_handler(WithRole(identity, _): WithRole<Admin>) -> String {
        identity.user.clone()
    }

    async fn any_role_handler(AnyRole(identity, _): AnyRole<EditorOrViewer>) -> String {
        identity.user.clone()
    }

    async fn all_roles_handler(AllRoles(identity, _): AllRoles<AdminAndEditor>) -> String {
        identity.user.clone()
    }

    #[tokio::test]
    async fn test_with_role_success() {
        let app = Router::new().route("/test", get(with_role_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec!["admin".into(), "viewer".into()]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_with_role_forbidden() {
        let app = Router::new().route("/test", get(with_role_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec!["viewer".into()]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_with_role_unauthorized() {
        let app = Router::new().route("/test", get(with_role_handler));
        let request = Request::get("/test").body(axum::body::Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_any_role_success() {
        let app = Router::new().route("/test", get(any_role_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec!["viewer".into()]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_any_role_forbidden() {
        let app = Router::new().route("/test", get(any_role_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec!["admin".into()]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_all_roles_success() {
        let app = Router::new().route("/test", get(all_roles_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec!["admin".into(), "editor".into(), "viewer".into()]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_all_roles_forbidden_missing_one() {
        let app = Router::new().route("/test", get(all_roles_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec!["admin".into()]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_all_roles_forbidden_empty_roles() {
        let app = Router::new().route("/test", get(all_roles_handler));
        let mut request = Request::get("/test").body(axum::body::Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(test_identity(vec![]));

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_deref() {
        let identity = test_identity(vec!["admin".into()]);
        let extractor = WithRole::<Admin>(identity.clone(), PhantomData);
        assert_eq!(extractor.user, "testuser");
        assert_eq!(extractor.roles, vec!["admin"]);
    }
}
