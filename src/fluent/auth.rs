//! Authentication middleware: OIDC, Basic Auth, and user span recording.

use super::router::FluentRouter;
use super::user_span;

#[allow(unused_imports)]
use crate::{HttpMiddleware, Result};

#[cfg(feature = "keycloak")]
use {
    crate::Role,
    axum_keycloak_auth::{
        PassthroughMode, Url, decode::ProfileAndEmail, instance::KeycloakAuthInstance,
        instance::KeycloakConfig, layer::KeycloakAuthLayer,
    },
};

#[cfg(feature = "basic-auth")]
use {super::basic_auth, std::sync::Arc};

#[cfg(feature = "keycloak")]
fn keycloak_token_to_identity(
    token: &axum_keycloak_auth::decode::KeycloakToken<
        crate::Role,
        axum_keycloak_auth::decode::ProfileAndEmail,
    >,
) -> crate::AuthenticatedIdentity {
    crate::AuthenticatedIdentity {
        method: crate::AuthMethod::Oidc,
        user: token.subject.clone(),
        email: {
            let email = &token.extra.email.email;
            if email.is_empty() {
                None
            } else {
                Some(email.clone())
            }
        },
        groups: token.roles.iter().map(|r| r.role().clone()).collect(),
        preferred_username: {
            let pref = &token.extra.profile.preferred_username;
            if pref.is_empty() {
                None
            } else {
                Some(pref.clone())
            }
        },
        access_token: None,
    }
}

#[cfg(feature = "keycloak")]
async fn map_keycloak_to_identity(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    type KcToken = axum_keycloak_auth::decode::KeycloakToken<
        crate::Role,
        axum_keycloak_auth::decode::ProfileAndEmail,
    >;
    type KcStatus = axum_keycloak_auth::KeycloakAuthStatus<
        crate::Role,
        axum_keycloak_auth::decode::ProfileAndEmail,
    >;

    // Extract identity from KeycloakToken.
    // Block mode stores bare KeycloakToken; Pass mode wraps it in KeycloakAuthStatus::Success.
    let identity = request
        .extensions()
        .get::<KcToken>()
        .map(keycloak_token_to_identity)
        .or_else(|| {
            request.extensions().get::<KcStatus>().and_then(|status| {
                if let axum_keycloak_auth::KeycloakAuthStatus::Success(token) = status {
                    Some(keycloak_token_to_identity(token))
                } else {
                    None
                }
            })
        });

    if let Some(identity) = identity {
        request.extensions_mut().insert(identity);
    }

    next.run(request).await
}

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up OpenID Connect (OIDC) authentication using Keycloak.
    ///
    /// Configures JWT token validation and role-based access control. Requires
    /// the `keycloak` feature to be enabled.
    ///
    /// When `redirect_uri` is configured, uses `PassthroughMode::Pass` so that
    /// requests without a Bearer token can be authenticated via session cookies
    /// instead. Without `redirect_uri`, uses `PassthroughMode::Block` (401 for
    /// unauthenticated requests).
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.oidc]
    /// issuer_url = "https://keycloak.example.com/realms/myrealm"
    /// realm = "myrealm"
    /// audiences = ["my-client"]
    /// client_id = "my-client"
    /// client_secret = "{{ KEYCLOAK_CLIENT_SECRET }}"
    /// ```
    #[cfg(feature = "keycloak")]
    pub fn setup_oidc(mut self) -> Result<Self> {
        if let Some(oidc) = &self.config.http.oidc
            && self.is_middleware_enabled(HttpMiddleware::Oidc)
        {
            tracing::trace!(
                realm = %oidc.realm,
                issuer_url = %oidc.issuer_url,
                auth_code_flow = oidc.auth_code_flow_enabled(),
                "OIDC middleware enabled"
            );
            let keycloak_auth_instance = KeycloakAuthInstance::new(
                KeycloakConfig::builder()
                    .server(Url::parse(&oidc.issuer_url)?)
                    .realm(oidc.realm.clone())
                    .build(),
            );

            // When auth code flow is enabled, use Pass mode so requests without
            // Bearer tokens pass through to session-to-identity middleware.
            let passthrough_mode = if oidc.auth_code_flow_enabled() {
                PassthroughMode::Pass
            } else {
                PassthroughMode::Block
            };

            // Map KeycloakToken → AuthenticatedIdentity (inner route_layer, runs second)
            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(map_keycloak_to_identity));

            // Validate Bearer tokens and set KeycloakToken (outer route_layer, runs first)
            self.inner = self.inner.route_layer(
                KeycloakAuthLayer::<Role, ProfileAndEmail>::builder()
                    .instance(keycloak_auth_instance)
                    .passthrough_mode(passthrough_mode)
                    .expected_audiences(oidc.audiences.clone())
                    .persist_raw_claims(true)
                    .build(),
            );

            // When auth code flow is enabled, add session-to-identity as a layer (runs
            // before route_layers). If a Bearer token is present, the route_layers above
            // will overwrite the session identity, so Bearer takes precedence.
            if oidc.auth_code_flow_enabled() {
                self.inner = self.inner.layer(axum::middleware::from_fn(
                    super::oidc_flow::session_to_identity,
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
            tracing::trace!(
                mode = ?basic_auth_config.mode,
                "BasicAuth middleware enabled"
            );
            let config = Arc::new(basic_auth_config.clone());

            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(move |request, next| {
                    let config = Arc::clone(&config);
                    basic_auth::basic_auth_middleware(config, request, next)
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

    /// Records the authenticated username to the logging span.
    ///
    /// This middleware runs after authentication and records the username
    /// to the `user` field of the current tracing span. This ensures all
    /// subsequent logs within the request include the authenticated user.
    ///
    /// Works with both Basic Auth (`AuthenticatedIdentity`) and OIDC
    /// (`KeycloakToken`) authentication methods.
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
