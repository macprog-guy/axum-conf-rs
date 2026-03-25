//! Authentication middleware: OIDC, Basic Auth, and user span recording.

use super::router::FluentRouter;
use super::user_span;

#[allow(unused_imports)]
use crate::{HttpMiddleware, Result};

#[cfg(feature = "keycloak")]
use std::sync::Arc;

#[cfg(all(feature = "basic-auth", not(feature = "keycloak")))]
use std::sync::Arc;

#[cfg(feature = "basic-auth")]
use super::basic_auth;

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up OpenID Connect (OIDC) Bearer token authentication.
    ///
    /// Configures JWT token validation using JWKS fetched from the OIDC provider.
    /// Requires the `keycloak` feature to be enabled.
    ///
    /// When `redirect_uri` is configured, uses passthrough mode so that
    /// requests without a Bearer token can be authenticated via session cookies
    /// instead. Without `redirect_uri`, returns 401 for unauthenticated requests.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.oidc]
    /// issuer_url = "https://keycloak.example.com"
    /// realm = "myrealm"
    /// audiences = ["my-client"]
    /// client_id = "my-client"
    /// client_secret = "{{ KEYCLOAK_CLIENT_SECRET }}"
    /// ```
    #[cfg(feature = "keycloak")]
    pub async fn setup_oidc(mut self) -> Result<Self> {
        if let Some(oidc) = &self.config.http.oidc
            && self.is_middleware_enabled(HttpMiddleware::Oidc)
        {
            tracing::trace!(
                realm = %oidc.realm,
                issuer_url = %oidc.issuer_url,
                auth_code_flow = oidc.auth_code_flow_enabled(),
                "OIDC middleware enabled"
            );

            let issuer_base = oidc.issuer_url.trim_end_matches('/');
            let issuer = format!("{issuer_base}/realms/{}", oidc.realm);
            let jwks_url = format!("{issuer}/protocol/openid-connect/certs");

            let jwks = super::oidc_bearer::JwksProvider::new(jwks_url).await?;

            let bearer_config = Arc::new(super::oidc_bearer::BearerAuthConfig {
                audiences: oidc.audiences.clone(),
                issuer,
                passthrough: oidc.auth_code_flow_enabled(),
                roles_claim: oidc.roles_claim.clone(),
            });

            // Validate Bearer tokens and map to AuthenticatedIdentity (single route_layer)
            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(move |request, next| {
                    super::oidc_bearer::bearer_auth_middleware(
                        Arc::clone(&jwks),
                        Arc::clone(&bearer_config),
                        request,
                        next,
                    )
                }));

            // When auth code flow is enabled, add session-to-identity as a layer (runs
            // before route_layers). If a Bearer token is present, the route_layer above
            // will overwrite the session identity, so Bearer takes precedence.
            if oidc.auth_code_flow_enabled() {
                let roles_claim = Arc::new(oidc.roles_claim.clone());
                self.inner = self.inner.layer(axum::middleware::from_fn(
                    move |request, next| {
                        super::oidc_flow::session_to_identity(
                            Arc::clone(&roles_claim),
                            request,
                            next,
                        )
                    },
                ));
            }
        }
        Ok(self)
    }

    /// Sets up OIDC Authorization Code flow routes (login, callback, logout).
    ///
    /// These routes are added as public endpoints (after auth middleware) so they
    /// are accessible without authentication. Only enabled when `redirect_uri`
    /// is configured in `[http.oidc]`.
    ///
    /// This method performs OIDC Discovery to fetch provider metadata.
    #[cfg(feature = "keycloak")]
    pub async fn setup_oidc_routes(mut self) -> Result<Self> {
        if let Some(oidc_config) = &self.config.http.oidc
            && oidc_config.auth_code_flow_enabled()
            && self.is_middleware_enabled(HttpMiddleware::Oidc)
        {
            tracing::trace!(
                login_route = %oidc_config.login_route,
                callback_route = %oidc_config.callback_route,
                logout_route = %oidc_config.logout_route,
                "OIDC auth code flow routes enabled"
            );

            let oidc_client =
                std::sync::Arc::new(super::oidc_flow::OidcClient::discover(oidc_config).await?);

            let login_route = oidc_config.login_route.clone();
            let callback_route = oidc_config.callback_route.clone();
            let logout_route = oidc_config.logout_route.clone();

            self.inner = self
                .inner
                .route(
                    &login_route,
                    axum::routing::get(super::oidc_flow::login_handler),
                )
                .route(
                    &callback_route,
                    axum::routing::get(super::oidc_flow::callback_handler),
                )
                .route(
                    &logout_route,
                    axum::routing::get(super::oidc_flow::logout_handler),
                )
                .layer(axum::Extension(oidc_client));
        }
        Ok(self)
    }

    /// Sets up HTTP Basic Auth and/or API Key authentication.
    ///
    /// When configured, protects all routes (except health endpoints) with authentication.
    /// Supports HTTP Basic Auth (RFC 7617), API Key authentication, or both.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.basic_auth]
    /// mode = "either"  # "basic", "api_key", or "either"
    /// api_key_header = "X-API-Key"
    ///
    /// [[http.basic_auth.users]]
    /// username = "admin"
    /// password = "{{ ADMIN_PASSWORD }}"
    ///
    /// [[http.basic_auth.api_keys]]
    /// key = "{{ API_KEY }}"
    /// name = "service-a"
    /// ```
    ///
    /// # Extracting Identity in Handlers
    ///
    /// ```rust,ignore
    /// use axum::Extension;
    /// use axum_conf::AuthenticatedIdentity;
    ///
    /// async fn handler(Extension(identity): Extension<AuthenticatedIdentity>) -> String {
    ///     format!("Hello, {}!", identity.name)
    /// }
    /// ```
    #[cfg(feature = "basic-auth")]
    pub fn setup_basic_auth(mut self) -> Result<Self> {
        if let Some(basic_auth_config) = &self.config.http.basic_auth
            && self.is_middleware_enabled(HttpMiddleware::BasicAuth)
        {
            // When OIDC auth code flow is also configured, Basic Auth passes through
            // requests with no credentials so OIDC session auth can handle them.
            #[cfg(feature = "keycloak")]
            let passthrough = self
                .config
                .http
                .oidc
                .as_ref()
                .is_some_and(|o| o.auth_code_flow_enabled());
            #[cfg(not(feature = "keycloak"))]
            let passthrough = false;

            tracing::trace!(
                mode = ?basic_auth_config.mode,
                passthrough,
                "BasicAuth middleware enabled"
            );
            let config = Arc::new(basic_auth_config.clone());

            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(move |request, next| {
                    let config = Arc::clone(&config);
                    basic_auth::basic_auth_middleware(config, passthrough, request, next)
                }));
        }
        Ok(self)
    }

    /// Sets up Proxy OIDC authentication.
    ///
    /// When configured, reads identity from HTTP headers set by an authenticating
    /// reverse proxy (e.g., oauth2-proxy with Nginx `auth_request`).
    ///
    /// If the user header is absent from a request, it passes through without
    /// setting an identity (no 401 error).
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.proxy_oidc]
    /// user_header = "X-Auth-Request-User"
    /// email_header = "X-Auth-Request-Email"
    /// groups_header = "X-Auth-Request-Groups"
    /// preferred_username_header = "X-Auth-Request-Preferred-Username"
    /// access_token_header = "X-Auth-Request-Access-Token"
    /// ```
    pub fn setup_proxy_oidc(mut self) -> Self {
        if let Some(proxy_oidc_config) = &self.config.http.proxy_oidc
            && self.is_middleware_enabled(HttpMiddleware::ProxyOidc)
        {
            tracing::trace!("ProxyOidc middleware enabled");
            let config = std::sync::Arc::new(proxy_oidc_config.clone());

            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(move |request, next| {
                    let config = std::sync::Arc::clone(&config);
                    super::proxy_oidc::proxy_oidc_middleware(config, request, next)
                }));
        }
        self
    }

    /// Sets up browser login redirect middleware.
    ///
    /// When OIDC auth code flow is enabled with `auto_redirect_to_login = true`,
    /// unauthenticated browser requests (Accept: text/html) are redirected to the
    /// login route. The original URL is stored in the session for post-login redirect.
    ///
    /// This must be added as the innermost route_layer (before OIDC and BasicAuth)
    /// so it runs AFTER all authentication middleware has resolved identity.
    #[cfg(feature = "keycloak")]
    pub fn setup_browser_login_redirect(mut self) -> Self {
        if let Some(oidc) = &self.config.http.oidc
            && oidc.auth_code_flow_enabled()
            && oidc.auto_redirect_to_login
            && self.is_middleware_enabled(HttpMiddleware::Oidc)
        {
            tracing::trace!(
                login_route = %oidc.login_route,
                "Browser login redirect middleware enabled"
            );

            // Build skip paths: auth routes + health/metrics
            let mut skip_paths = vec![
                oidc.login_route.clone(),
                oidc.callback_route.clone(),
                oidc.logout_route.clone(),
                self.config.http.liveness_route.clone(),
                self.config.http.readiness_route.clone(),
                self.config.http.metrics_route.clone(),
            ];

            // Also skip static file paths
            for dir in &self.config.http.directories {
                if let crate::StaticDirRoute::Route(path) = &dir.route {
                    skip_paths.push(path.clone());
                }
            }

            let config = std::sync::Arc::new(super::browser_redirect::BrowserRedirectConfig {
                login_route: oidc.login_route.clone(),
                skip_paths,
            });

            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(move |request, next| {
                    let config = std::sync::Arc::clone(&config);
                    super::browser_redirect::browser_redirect_middleware(config, request, next)
                }));
        }
        self
    }

    /// Records the authenticated username to the logging span.
    ///
    /// This middleware runs after authentication and records the username
    /// to the `user` field of the current tracing span. This ensures all
    /// subsequent logs within the request include the authenticated user.
    ///
    /// Works with all authentication methods (OIDC, Basic Auth, Proxy OIDC)
    /// via `AuthenticatedIdentity`.
    ///
    /// For unauthenticated requests (e.g., health endpoints), the `user`
    /// field remains empty and won't appear in log output.
    #[must_use]
    pub fn setup_user_span(mut self) -> Self {
        tracing::trace!("UserSpan middleware enabled");
        self.inner = self
            .inner
            .layer(axum::middleware::from_fn(user_span::record_user_to_span));
        self
    }
}
