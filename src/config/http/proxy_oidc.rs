//! Proxy OIDC authentication configuration.
//!
//! This module provides configuration for reverse-proxy-based OIDC authentication,
//! where an upstream proxy (e.g., oauth2-proxy with Nginx `auth_request`) handles
//! OIDC authentication and passes identity information via HTTP headers.
//!
//! # Example
//!
//! ```toml
//! [http.proxy_oidc]
//! # All fields are optional with sensible defaults matching oauth2-proxy conventions
//! user_header = "X-Auth-Request-User"
//! email_header = "X-Auth-Request-Email"
//! groups_header = "X-Auth-Request-Groups"
//! preferred_username_header = "X-Auth-Request-Preferred-Username"
//! access_token_header = "X-Auth-Request-Access-Token"
//! ```

use serde::Deserialize;

/// Configuration for proxy-based OIDC authentication.
///
/// When present, the middleware reads identity information from HTTP headers
/// set by an authenticating reverse proxy (e.g., oauth2-proxy).
///
/// All header names have sensible defaults matching the oauth2-proxy convention.
/// If the user header is absent from a request, the middleware passes through
/// without setting an identity (no 401 error).
#[derive(Debug, Clone, Deserialize)]
pub struct HttpProxyOidcConfig {
    /// Header containing the authenticated user identifier.
    #[serde(default = "HttpProxyOidcConfig::default_user_header")]
    pub user_header: String,

    /// Header containing the user's email address.
    #[serde(default = "HttpProxyOidcConfig::default_email_header")]
    pub email_header: String,

    /// Header containing comma-separated group memberships.
    #[serde(default = "HttpProxyOidcConfig::default_groups_header")]
    pub groups_header: String,

    /// Header containing the preferred username for display.
    #[serde(default = "HttpProxyOidcConfig::default_preferred_username_header")]
    pub preferred_username_header: String,

    /// Header containing the access token (when proxy passes it).
    #[serde(default = "HttpProxyOidcConfig::default_access_token_header")]
    pub access_token_header: String,
}

impl Default for HttpProxyOidcConfig {
    fn default() -> Self {
        Self {
            user_header: Self::default_user_header(),
            email_header: Self::default_email_header(),
            groups_header: Self::default_groups_header(),
            preferred_username_header: Self::default_preferred_username_header(),
            access_token_header: Self::default_access_token_header(),
        }
    }
}

impl HttpProxyOidcConfig {
    fn default_user_header() -> String {
        "X-Auth-Request-User".to_string()
    }

    fn default_email_header() -> String {
        "X-Auth-Request-Email".to_string()
    }

    fn default_groups_header() -> String {
        "X-Auth-Request-Groups".to_string()
    }

    fn default_preferred_username_header() -> String {
        "X-Auth-Request-Preferred-Username".to_string()
    }

    fn default_access_token_header() -> String {
        "X-Auth-Request-Access-Token".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn test_proxy_oidc_defaults() {
        let config = HttpProxyOidcConfig::default();
        assert_eq!(config.user_header, "X-Auth-Request-User");
        assert_eq!(config.email_header, "X-Auth-Request-Email");
        assert_eq!(config.groups_header, "X-Auth-Request-Groups");
        assert_eq!(
            config.preferred_username_header,
            "X-Auth-Request-Preferred-Username"
        );
        assert_eq!(config.access_token_header, "X-Auth-Request-Access-Token");
    }

    #[test]
    fn test_proxy_oidc_config_parsing() {
        let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.proxy_oidc]
user_header = "X-Custom-User"
email_header = "X-Custom-Email"
        "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");
        assert!(config.http.proxy_oidc.is_some());
        let proxy_oidc = config.http.proxy_oidc.unwrap();
        assert_eq!(proxy_oidc.user_header, "X-Custom-User");
        assert_eq!(proxy_oidc.email_header, "X-Custom-Email");
        assert_eq!(proxy_oidc.groups_header, "X-Auth-Request-Groups");
    }

    #[test]
    fn test_proxy_oidc_config_all_defaults() {
        let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.proxy_oidc]
        "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");
        assert!(config.http.proxy_oidc.is_some());
        let proxy_oidc = config.http.proxy_oidc.unwrap();
        assert_eq!(proxy_oidc.user_header, "X-Auth-Request-User");
    }

    #[test]
    fn test_no_proxy_oidc_config() {
        let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
        "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");
        assert!(config.http.proxy_oidc.is_none());
    }
}
