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
//! # Example
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
//! # Important
//!
//! OIDC authentication cannot be used together with Basic Auth (`basic-auth` feature).
//! Choose one authentication method per application.

use crate::{Error, Result, utils::Sensitive};
use serde::Deserialize;

/// Configuration for OIDC (OpenID Connect) authentication.
///
/// Used to configure authentication against an OIDC provider like Keycloak.
/// All fields are required except `audiences` which defaults to an empty list.
///
/// # Required Configuration
///
/// - `issuer_url` - Base URL of the OIDC provider (e.g., `https://keycloak.example.com`)
/// - `realm` - The OIDC realm/tenant name
/// - `client_id` - OAuth2 client ID for this application
/// - `client_secret` - OAuth2 client secret (use environment variable substitution)
///
/// # Example
///
/// ```toml
/// [http.oidc]
/// issuer_url = "https://keycloak.example.com"
/// realm = "production"
/// client_id = "my-service"
/// client_secret = "{{ OIDC_CLIENT_SECRET }}"
/// audiences = ["my-service"]
/// ```
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
}

#[allow(unused)]
impl HttpOidcConfig {
    pub fn default_realm() -> String {
        "pictet".into()
    }
    pub fn validate(&self) -> Result<()> {
        // Validate issuer URL is not empty or whitespace
        if self.issuer_url.trim().is_empty() {
            return Err(Error::invalid_input(
                "OIDC issuer_url is required. Set [http.oidc] issuer_url = \"https://your-keycloak-server\" in config.",
            ));
        }

        // Validate issuer URL format (must be http:// or https://)
        if !self.issuer_url.starts_with("http://") && !self.issuer_url.starts_with("https://") {
            return Err(Error::invalid_input(
                "OIDC issuer_url must start with http:// or https://. Example: \"https://keycloak.example.com\"",
            ));
        }

        // Validate realm is not empty
        if self.realm.trim().is_empty() {
            return Err(Error::invalid_input(
                "OIDC realm is required. Set [http.oidc] realm = \"your-realm\" in config.",
            ));
        }

        // Validate client_id is not empty or whitespace
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
        // Test that OIDC configuration can be parsed correctly
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

        // Verify OIDC config was parsed correctly
        assert!(config.http.oidc.is_some());
        let oidc = config.http.oidc.unwrap();
        assert_eq!(oidc.issuer_url, "https://keycloak.example.com");
        assert_eq!(oidc.realm, "test-realm");
        assert_eq!(oidc.audiences, vec!["api", "web"]);
        assert_eq!(oidc.client_id, "my-client");
        assert!(oidc.client_secret == Sensitive::from("my-secret"));
    }
}
