//! Browser login redirect middleware.
//!
//! When OIDC auth code flow is enabled with `auto_redirect_to_login = true`,
//! this middleware redirects unauthenticated browser requests to the login route.
//! It runs after all auth middleware so that `AuthenticatedIdentity` is already
//! resolved (from Bearer tokens, sessions, or Basic Auth).

use axum::{
    extract::Request,
    http::header,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use std::sync::Arc;
use tower_sessions::Session;

use crate::AuthenticatedIdentity;

const SESSION_RETURN_URL: &str = "oidc_return_url";

/// Configuration for the browser redirect middleware.
#[derive(Clone)]
pub(crate) struct BrowserRedirectConfig {
    pub login_route: String,
    pub skip_paths: Vec<String>,
}

/// Middleware that redirects unauthenticated browser requests to the OIDC login route.
///
/// Pass-through conditions (does NOT redirect):
/// 1. `AuthenticatedIdentity` already in extensions (user is authenticated)
/// 2. Request path matches a skip path (login/callback/logout, health, metrics)
/// 3. Request has an `Authorization` header (let auth layers handle it)
/// 4. `Accept` header does NOT contain `text/html` (API client, not browser)
///
/// When redirecting, stores the original URL in session as `oidc_return_url`
/// so the callback handler can return the user to where they were going.
pub(crate) async fn browser_redirect_middleware(
    config: Arc<BrowserRedirectConfig>,
    request: Request,
    next: Next,
) -> Response {
    // 1. Already authenticated → pass through
    if request.extensions().get::<AuthenticatedIdentity>().is_some() {
        return next.run(request).await;
    }

    // 2. Skip paths (login/callback/logout, health, metrics)
    let path = request.uri().path();
    if config.skip_paths.iter().any(|skip| path.starts_with(skip)) {
        return next.run(request).await;
    }

    // 3. Has Authorization header → let auth layers handle it
    if request.headers().contains_key(header::AUTHORIZATION) {
        return next.run(request).await;
    }

    // 4. Not a browser request (no text/html in Accept)
    let is_browser = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));

    if !is_browser {
        return next.run(request).await;
    }

    // Store original URL in session for post-login redirect
    let original_url = request
        .uri()
        .path_and_query()
        .map(|pq| pq.to_string())
        .unwrap_or_else(|| "/".to_string());

    if let Some(session) = request.extensions().get::<Session>() {
        let _ = session.insert(SESSION_RETURN_URL, &original_url).await;
    }

    Redirect::temporary(&config.login_route).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::get;
    use tower::ServiceExt;

    fn test_config() -> Arc<BrowserRedirectConfig> {
        Arc::new(BrowserRedirectConfig {
            login_route: "/auth/login".to_string(),
            skip_paths: vec![
                "/auth/".to_string(),
                "/live".to_string(),
                "/ready".to_string(),
                "/metrics".to_string(),
            ],
        })
    }

    /// Build a router with the browser redirect middleware applied.
    fn test_router() -> Router {
        let config = test_config();
        Router::new()
            .route("/dashboard", get(|| async { "ok" }))
            .route("/api/data", get(|| async { "ok" }))
            .route("/live", get(|| async { "ok" }))
            .route("/ready", get(|| async { "ok" }))
            .route("/metrics", get(|| async { "ok" }))
            .route("/auth/login", get(|| async { "login page" }))
            .route_layer(axum::middleware::from_fn(move |request, next| {
                let config = Arc::clone(&config);
                browser_redirect_middleware(config, request, next)
            }))
    }

    /// Helper to build a request with given path and optional headers.
    fn build_request(
        path: &str,
        accept: Option<&str>,
        auth: Option<&str>,
    ) -> http::Request<axum::body::Body> {
        let mut builder = http::Request::builder().uri(path);
        if let Some(accept) = accept {
            builder = builder.header(header::ACCEPT, accept);
        }
        if let Some(auth) = auth {
            builder = builder.header(header::AUTHORIZATION, auth);
        }
        builder.body(axum::body::Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn test_skip_path_passes_through() {
        let app = test_router();
        let response = app
            .oneshot(build_request("/auth/login", Some("text/html"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_authorization_header_passes_through() {
        let app = test_router();
        let response = app
            .oneshot(build_request(
                "/dashboard",
                Some("text/html"),
                Some("Bearer token123"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_request_passes_through() {
        let app = test_router();
        let response = app
            .oneshot(build_request("/api/data", Some("application/json"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_no_accept_header_passes_through() {
        let app = test_router();
        let response = app
            .oneshot(build_request("/dashboard", None, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_unauthenticated_browser_redirects() {
        let app = test_router();
        let response = app
            .oneshot(build_request("/dashboard", Some("text/html"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/auth/login"
        );
    }

    #[tokio::test]
    async fn test_health_endpoint_passes_through() {
        let app = test_router();
        let response = app
            .oneshot(build_request("/live", Some("text/html"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_metrics_endpoint_passes_through() {
        let app = test_router();
        let response = app
            .oneshot(build_request("/metrics", Some("text/html"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_authenticated_request_passes_through() {
        // Build a router that injects identity before the redirect middleware
        let config = test_config();
        let app = Router::new()
            .route("/dashboard", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn({
                let config = Arc::clone(&config);
                move |request, next| {
                    let config = Arc::clone(&config);
                    browser_redirect_middleware(config, request, next)
                }
            }))
            .layer(axum::middleware::from_fn(
                |mut request: Request, next: Next| async move {
                    request
                        .extensions_mut()
                        .insert(AuthenticatedIdentity {
                            method: crate::AuthMethod::Oidc,
                            user: "test-user".to_string(),
                            email: None,
                            groups: vec![],
                            roles: vec![],
                            preferred_username: None,
                            access_token: None,
                        });
                    next.run(request).await
                },
            ));

        let response = app
            .oneshot(build_request("/dashboard", Some("text/html"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
