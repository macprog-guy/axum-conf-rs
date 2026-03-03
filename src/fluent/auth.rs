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
async fn map_keycloak_to_identity(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(token) = request
        .extensions()
        .get::<axum_keycloak_auth::decode::KeycloakToken<
            crate::Role,
            axum_keycloak_auth::decode::ProfileAndEmail,
        >>()
    {
        let identity = crate::AuthenticatedIdentity {
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
            groups: token
                .roles
                .iter()
                .map(|r| r.role().clone())
                .collect(),
            preferred_username: {
                let pref = &token.extra.profile.preferred_username;
                if pref.is_empty() {
                    None
                } else {
                    Some(pref.clone())
                }
            },
            access_token: None,
        };
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
    ///
    /// # Returns
    ///
    /// A `Result` containing the configured router or an error if OIDC setup fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - OIDC configuration is invalid
    /// - Cannot connect to the Keycloak server
    /// - Client credentials are incorrect
    #[cfg(feature = "keycloak")]
    pub fn setup_oidc(mut self) -> Result<Self> {
        if let Some(oidc) = &self.config.http.oidc
            && self.is_middleware_enabled(HttpMiddleware::Oidc)
        {
            tracing::trace!(
                realm = %oidc.realm,
                issuer_url = %oidc.issuer_url,
                "OIDC middleware enabled"
            );
            let keycloak_auth_instance = KeycloakAuthInstance::new(
                KeycloakConfig::builder()
                    .server(Url::parse(&oidc.issuer_url)?)
                    .realm(oidc.realm.clone())
                    .build(),
            );

            self.inner = self.inner.route_layer(
                KeycloakAuthLayer::<Role, ProfileAndEmail>::builder()
                    .instance(keycloak_auth_instance)
                    .passthrough_mode(PassthroughMode::Block)
                    .expected_audiences(oidc.audiences.clone())
                    .persist_raw_claims(true)
                    .build(),
            );

            // Map KeycloakToken to unified AuthenticatedIdentity
            self.inner = self
                .inner
                .layer(axum::middleware::from_fn(map_keycloak_to_identity));
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
