//! HTTP Basic Auth and API Key authentication configuration.
//!
//! This module provides configuration for simple authentication methods:
//! - HTTP Basic Auth (RFC 7617) with username/password
//! - API Key authentication via a configurable header
//!
//! # Feature Flag
//!
//! Requires the `basic-auth` feature to be enabled.
//!
//! # Example
//!
//! ```toml
//! [http.basic_auth]
//! mode = "either"  # Accept both Basic Auth and API Keys
//! api_key_header = "X-API-Key"
//!
//! [[http.basic_auth.users]]
//! username = "admin"
//! password = "{{ ADMIN_PASSWORD }}"
//!
//! [[http.basic_auth.api_keys]]
//! key = "{{ API_KEY }}"
//! name = "frontend-service"
//! ```
//!
//! # Important
//!
//! Basic Auth cannot be used together with OIDC authentication (`keycloak` feature).
//! Choose one authentication method per application.

use crate::{Error, Result, utils::Sensitive};
use serde::Deserialize;

/// Authentication mode for basic authentication.
///
/// Determines which authentication methods are accepted by the middleware.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BasicAuthMode {
    /// Only HTTP Basic Auth (RFC 7617) is accepted.
    Basic,
    /// Only API Key authentication is accepted.
    ApiKey,
    /// Either HTTP Basic Auth or API Key is accepted (default).
    #[default]
    Either,
}

/// A single user credential for HTTP Basic Auth.
///
/// # Example TOML
///
/// ```toml
/// [[http.basic_auth.users]]
/// username = "admin"
/// password = "{{ ADMIN_PASSWORD }}"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthUser {
    /// Username for authentication.
    pub username: String,
    /// Password wrapped in Sensitive for secure handling.
    pub password: Sensitive<String>,
}

/// A single API key credential.
///
/// # Example TOML
///
/// ```toml
/// [[http.basic_auth.api_keys]]
/// key = "{{ API_KEY_1 }}"
/// name = "frontend-app"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthApiKey {
    /// The API key value wrapped in Sensitive for secure handling.
    pub key: Sensitive<String>,
    /// Optional friendly name for logging and auditing purposes.
    #[serde(default)]
    pub name: Option<String>,
}

/// Configuration for HTTP Basic Auth and API Key authentication.
///
/// This configuration supports two authentication methods:
/// - HTTP Basic Auth (RFC 7617) with username/password credentials
/// - API Key authentication with a configurable header
///
/// # Example TOML
///
/// ```toml
/// [http.basic_auth]
/// mode = "either"           # "basic", "api_key", or "either"
/// api_key_header = "X-API-Key"
///
/// [[http.basic_auth.users]]
/// username = "admin"
/// password = "{{ ADMIN_PASSWORD }}"
///
/// [[http.basic_auth.api_keys]]
/// key = "{{ API_KEY_1 }}"
/// name = "frontend-app"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct HttpBasicAuthConfig {
    /// Authentication mode determining which methods are accepted.
    #[serde(default)]
    pub mode: BasicAuthMode,

    /// Header name for API key authentication.
    /// Defaults to "X-API-Key".
    #[serde(default = "HttpBasicAuthConfig::default_api_key_header")]
    pub api_key_header: String,

    /// User credentials for HTTP Basic Auth.
    #[serde(default)]
    pub users: Vec<BasicAuthUser>,

    /// API keys for API Key authentication.
    #[serde(default)]
    pub api_keys: Vec<BasicAuthApiKey>,
}

impl Default for HttpBasicAuthConfig {
    fn default() -> Self {
        Self {
            mode: BasicAuthMode::default(),
            api_key_header: Self::default_api_key_header(),
            users: Vec::new(),
            api_keys: Vec::new(),
        }
    }
}

impl HttpBasicAuthConfig {
    fn default_api_key_header() -> String {
        "X-API-Key".to_string()
    }

    /// Validates the basic auth configuration.
    ///
    /// Ensures that:
    /// - At least one credential is configured based on the mode
    /// - Usernames are not empty
    /// - Passwords are not empty
    /// - API keys are not empty
    /// - API key header name is not empty
    pub fn validate(&self) -> Result<()> {
        let has_users = !self.users.is_empty();
        let has_api_keys = !self.api_keys.is_empty();

        match self.mode {
            BasicAuthMode::Basic if !has_users => {
                return Err(Error::invalid_input(
                    "Basic auth mode 'basic' requires at least one user. Add [[http.basic_auth.users]] to config.",
                ));
            }
            BasicAuthMode::ApiKey if !has_api_keys => {
                return Err(Error::invalid_input(
                    "Basic auth mode 'api_key' requires at least one API key. Add [[http.basic_auth.api_keys]] to config.",
                ));
            }
            BasicAuthMode::Either if !has_users && !has_api_keys => {
                return Err(Error::invalid_input(
                    "Basic auth requires at least one user or API key. Add credentials to config.",
                ));
            }
            _ => {}
        }

        // Validate usernames and passwords are not empty
        for user in &self.users {
            if user.username.trim().is_empty() {
                return Err(Error::invalid_input("Basic auth username cannot be empty."));
            }
            if user.password.0.is_empty() {
                return Err(Error::invalid_input(
                    "Basic auth password cannot be empty. Use {{ ENV_VAR }} for secrets.",
                ));
            }
        }

        // Validate API keys are not empty
        for api_key in &self.api_keys {
            if api_key.key.0.is_empty() {
                return Err(Error::invalid_input(
                    "API key cannot be empty. Use {{ ENV_VAR }} for secrets.",
                ));
            }
        }

        // Validate API key header name
        if self.api_key_header.trim().is_empty() {
            return Err(Error::invalid_input("API key header name cannot be empty."));
        }

        Ok(())
    }
}

/// Identity of an authenticated user or service.
///
/// This struct is inserted into request extensions after successful authentication
/// and can be extracted in handlers using `Extension<AuthenticatedIdentity>`.
///
/// # Example
///
/// ```rust,ignore
/// use axum::Extension;
/// use axum_conf::AuthenticatedIdentity;
///
/// async fn handler(Extension(identity): Extension<AuthenticatedIdentity>) -> String {
///     format!("Hello, {}!", identity.name)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    /// The authentication method used.
    pub method: AuthMethod,
    /// Username (for Basic Auth) or API key name (for API Key).
    pub name: String,
}

/// The authentication method used for a request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    /// HTTP Basic Auth (RFC 7617).
    BasicAuth,
    /// API Key authentication.
    ApiKey,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_auth_mode_default() {
        let mode: BasicAuthMode = Default::default();
        assert_eq!(mode, BasicAuthMode::Either);
    }

    #[test]
    fn test_config_validation_empty_in_basic_mode() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::Basic,
            users: vec![],
            api_keys: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_in_api_key_mode() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::ApiKey,
            users: vec![],
            api_keys: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_in_either_mode() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::Either,
            users: vec![],
            api_keys: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_valid_user() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::Basic,
            users: vec![BasicAuthUser {
                username: "admin".to_string(),
                password: Sensitive::from("secret"),
            }],
            api_keys: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_valid_api_key() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::ApiKey,
            users: vec![],
            api_keys: vec![BasicAuthApiKey {
                key: Sensitive::from("my-api-key"),
                name: Some("test-key".to_string()),
            }],
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_empty_username() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::Basic,
            users: vec![BasicAuthUser {
                username: "  ".to_string(),
                password: Sensitive::from("secret"),
            }],
            api_keys: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_password() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::Basic,
            users: vec![BasicAuthUser {
                username: "admin".to_string(),
                password: Sensitive::from(""),
            }],
            api_keys: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_api_key() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::ApiKey,
            users: vec![],
            api_keys: vec![BasicAuthApiKey {
                key: Sensitive::from(""),
                name: None,
            }],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_header_name() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::ApiKey,
            api_key_header: "  ".to_string(),
            users: vec![],
            api_keys: vec![BasicAuthApiKey {
                key: Sensitive::from("my-key"),
                name: None,
            }],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_default_api_key_header() {
        let config: HttpBasicAuthConfig = Default::default();
        assert_eq!(config.api_key_header, "X-API-Key");
    }
}
