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
        self.inner = self
            .inner
            .layer(axum::middleware::from_fn(user_span::record_user_to_span));
        self
    }
}
