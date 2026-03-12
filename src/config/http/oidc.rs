//! OIDC/Keycloak authentication configuration.
//!
//! This module provides configuration for OpenID Connect (OIDC) authentication,
//! designed for use with Keycloak but compatible with other OIDC providers.
//!
//! # Feature Flag
//!
//! This module is always available, but the OIDC middleware is only enabled
//! with the `keycloak` feature flag.
//!
//! # Bearer-Only Example (API)
//!
//! ```toml
//! [http.oidc]
//! issuer_url = "https://keycloak.example.com"
//! realm = "my-realm"
//! client_id = "my-app"
//! client_secret = "{{ OIDC_CLIENT_SECRET }}"
//! audiences = ["my-app", "api"]
//! ```
//!
//! # Authorization Code Flow Example (Browser)
//!
//! Adding `redirect_uri` enables the full OIDC login flow with
//! `/auth/login`, `/auth/callback`, and `/auth/logout` routes.
//!
//! ```toml
//! [http.oidc]
//! issuer_url = "https://keycloak.example.com"
//! realm = "my-realm"
//! client_id = "my-app"
//! client_secret = "{{ OIDC_CLIENT_SECRET }}"
//! audiences = ["my-app"]
//! redirect_uri = "https://myapp.example.com/auth/callback"
//! scopes = ["openid"]
//! post_login_redirect = "/"
//! post_logout_redirect = "/"
//! ```
//!
//! # Compatibility
//!
//! OIDC and Basic Auth can coexist when auth code flow is enabled (`redirect_uri` set).
//! In bearer-only mode (no `redirect_uri`), they are mutually exclusive since both
//! compete for the `Authorization` header.

use crate::{Error, Result, utils::Sensitive};
use serde::Deserialize;

/// Configuration for OIDC (OpenID Connect) authentication.
///
/// Used to configure authentication against an OIDC provider like Keycloak.
/// All fields are required except `audiences` which defaults to an empty list.
///
/// When `redirect_uri` is set, the Authorization Code flow is enabled with
/// login, callback, and logout routes. Without it, only Bearer token
/// validation is active.
///
/// # Required Configuration
///
/// - `issuer_url` - Base URL of the OIDC provider (e.g., `https://keycloak.example.com`)
/// - `realm` - The OIDC realm/tenant name
/// - `client_id` - OAuth2 client ID for this application
/// - `client_secret` - OAuth2 client secret (use environment variable substitution)
///
/// # Authorization Code Flow (optional)
///
/// - `redirect_uri` - Callback URL registered with the OIDC provider. Enables auth code flow.
/// - `scopes` - OAuth2 scopes to request (default: `["openid"]`)
/// - `post_login_redirect` - Where to redirect after login (default: `"/"`)
/// - `post_logout_redirect` - Where to redirect after logout (default: `"/"`)
/// - `login_route` - Login endpoint path (default: `"/auth/login"`)
/// - `callback_route` - Callback endpoint path (default: `"/auth/callback"`)
/// - `logout_route` - Logout endpoint path (default: `"/auth/logout"`)
#[allow(unused)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HttpOidcConfig {
    #[serde(default)]
    pub issuer_url: String,
    #[serde(default = "HttpOidcConfig::default_realm")]
    pub realm: String,
    #[serde(default)]
    pub audiences: Vec<String>,
    pub client_id: String,
    pub client_secret: Sensitive<String>,

    /// Redirect URI for the OIDC callback. When set, enables the auth code flow routes.
    /// Must match the redirect URI registered with the OIDC provider.
    #[serde(default)]
    pub redirect_uri: Option<String>,

    /// OAuth2 scopes to request. Defaults to `["openid"]`.
    #[serde(default = "HttpOidcConfig::default_scopes")]
    pub scopes: Vec<String>,

    /// URL to redirect to after successful login. Defaults to `"/"`.
    #[serde(default = "HttpOidcConfig::default_redirect")]
    pub post_login_redirect: String,

    /// URL to redirect to after logout. Defaults to `"/"`.
    #[serde(default = "HttpOidcConfig::default_redirect")]
    pub post_logout_redirect: String,

    /// Route path for the login endpoint. Defaults to `"/auth/login"`.
    #[serde(default = "HttpOidcConfig::default_login_route")]
    pub login_route: String,

    /// Route path for the callback endpoint. Defaults to `"/auth/callback"`.
    #[serde(default = "HttpOidcConfig::default_callback_route")]
    pub callback_route: String,

    /// Route path for the logout endpoint. Defaults to `"/auth/logout"`.
    #[serde(default = "HttpOidcConfig::default_logout_route")]
    pub logout_route: String,

    /// Auto-redirect unauthenticated browser requests to the login route.
    /// Only effective when auth code flow is enabled (`redirect_uri` is set).
    /// Defaults to `false`.
    #[serde(default)]
    pub auto_redirect_to_login: bool,

    /// JWT claim key containing application-specific roles.
    /// Populated from a custom top-level JWT claim (e.g., set via a Keycloak protocol mapper).
    /// Defaults to `"applicationRoles"`.
    #[serde(default = "HttpOidcConfig::default_roles_claim")]
    pub roles_claim: String,
}

#[allow(unused)]
impl HttpOidcConfig {
    pub fn default_realm() -> String {
        "pictet".into()
    }

    pub fn default_scopes() -> Vec<String> {
        vec!["openid".into()]
    }

    pub fn default_redirect() -> String {
        "/".into()
    }

    pub fn default_login_route() -> String {
        "/auth/login".into()
    }

    pub fn default_callback_route() -> String {
        "/auth/callback".into()
    }

    pub fn default_logout_route() -> String {
        "/auth/logout".into()
    }

    pub fn default_roles_claim() -> String {
        "applicationRoles".into()
    }

    /// Returns true if the Authorization Code flow is enabled (redirect_uri is set).
    pub fn auth_code_flow_enabled(&self) -> bool {
        self.redirect_uri.is_some()
    }

    pub fn validate(&self) -> Result<()> {
        if self.issuer_url.trim().is_empty() {
            return Err(Error::invalid_input(
                "OIDC issuer_url is required. Set [http.oidc] issuer_url = \"https://your-keycloak-server\" in config.",
            ));
        }

        if !self.issuer_url.starts_with("http://") && !self.issuer_url.starts_with("https://") {
            return Err(Error::invalid_input(
                "OIDC issuer_url must start with http:// or https://. Example: \"https://keycloak.example.com\"",
            ));
        }

        if self.realm.trim().is_empty() {
            return Err(Error::invalid_input(
                "OIDC realm is required. Set [http.oidc] realm = \"your-realm\" in config.",
            ));
        }

        if self.client_id.trim().is_empty() {
            return Err(Error::invalid_input(
                "OIDC client_id is required. Set [http.oidc] client_id = \"your-client-id\" in config.",
            ));
        }

        if self.client_secret.0.is_empty() {
            return Err(Error::invalid_input(
                "OIDC client_secret is required. Set [http.oidc] client_secret = \"{{ OIDC_CLIENT_SECRET }}\" to use env var.",
            ));
        }

        // Validate redirect_uri format when present
        if let Some(redirect_uri) = &self.redirect_uri
            && !redirect_uri.starts_with("http://")
            && !redirect_uri.starts_with("https://")
        {
            return Err(Error::invalid_input(
                "OIDC redirect_uri must start with http:// or https://.",
            ));
        }

        Ok(())
    }
}

#[cfg(feature = "keycloak")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[tokio::test]
    async fn test_oidc_config_parsing() {
        let toml_str = r#"
[database]
url = "postgres://test:test@localhost:5432/test"
max_pool_size = 5

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"

[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "test-realm"
audiences = ["api", "web"]
client_id = "my-client"
client_secret = "my-secret"

[logging]
format = "json"
    "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");

        assert!(config.http.oidc.is_some());
        let oidc = config.http.oidc.unwrap();
        assert_eq!(oidc.issuer_url, "https://keycloak.example.com");
        assert_eq!(oidc.realm, "test-realm");
        assert_eq!(oidc.audiences, vec!["api", "web"]);
        assert_eq!(oidc.client_id, "my-client");
        assert!(oidc.client_secret == Sensitive::from("my-secret"));
        // Auth code flow fields should have defaults
        assert!(oidc.redirect_uri.is_none());
        assert_eq!(oidc.scopes, vec!["openid"]);
        assert_eq!(oidc.post_login_redirect, "/");
        assert_eq!(oidc.post_logout_redirect, "/");
        assert_eq!(oidc.login_route, "/auth/login");
        assert_eq!(oidc.callback_route, "/auth/callback");
        assert_eq!(oidc.logout_route, "/auth/logout");
    }

    #[tokio::test]
    async fn test_oidc_config_with_auth_code_flow() {
        let toml_str = r#"
[database]
url = "postgres://test:test@localhost:5432/test"
max_pool_size = 5

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"

[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "test-realm"
client_id = "my-client"
client_secret = "my-secret"
redirect_uri = "https://myapp.example.com/auth/callback"
scopes = ["openid", "email"]
post_login_redirect = "/dashboard"
post_logout_redirect = "/goodbye"
login_route = "/sso/login"
callback_route = "/sso/callback"
logout_route = "/sso/logout"

[logging]
format = "json"
    "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");

        let oidc = config.http.oidc.unwrap();
        assert!(oidc.auth_code_flow_enabled());
        assert_eq!(
            oidc.redirect_uri.as_deref(),
            Some("https://myapp.example.com/auth/callback")
        );
        assert_eq!(oidc.scopes, vec!["openid", "email"]);
        assert_eq!(oidc.post_login_redirect, "/dashboard");
        assert_eq!(oidc.post_logout_redirect, "/goodbye");
        assert_eq!(oidc.login_route, "/sso/login");
        assert_eq!(oidc.callback_route, "/sso/callback");
        assert_eq!(oidc.logout_route, "/sso/logout");
    }

    #[test]
    fn test_redirect_uri_validation() {
        let config = HttpOidcConfig {
            issuer_url: "https://keycloak.example.com".into(),
            realm: "test".into(),
            client_id: "app".into(),
            client_secret: Sensitive::from("secret"),
            redirect_uri: Some("not-a-url".into()),
            ..Default::default()
        };
        assert!(config.validate().is_err());

        let config = HttpOidcConfig {
            redirect_uri: Some("https://myapp.com/callback".into()),
            ..config
        };
        assert!(config.validate().is_ok());
    }
}
