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

use crate::utils::Sensitive;
use ipnet::IpNet;
use serde::Deserialize;

/// Configuration for proxy-based OIDC authentication.
///
/// When present, the middleware reads identity information from HTTP headers
/// set by an authenticating reverse proxy (e.g., oauth2-proxy).
///
/// All header names have sensible defaults matching the oauth2-proxy convention.
/// If the user header is absent from a request, the middleware passes through
/// without setting an identity (no 401 error).
///
/// # Trusting the proxy
///
/// The `X-Auth-Request-*` headers carry the caller's identity **and roles**, so
/// they must only be honored when they genuinely originate from your proxy — a
/// client able to reach the app directly could otherwise spoof them. Configure
/// **at least one** of:
///
/// - [`trusted_proxies`](Self::trusted_proxies): CIDR ranges of the proxy. Headers
///   are honored only when the connection's peer IP falls within one of them.
/// - [`shared_secret`](Self::shared_secret): a secret the proxy echoes in
///   [`shared_secret_header`](Self::shared_secret_header).
///
/// When **neither** is configured, the headers are honored only outside production
/// (`RUST_ENV` unset / `prod` / `production` / `release` ⇒ production) and ignored
/// in production — i.e. **fail-closed by default**.
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

    /// Header containing comma-separated application roles.
    #[serde(default = "HttpProxyOidcConfig::default_roles_header")]
    pub roles_header: String,

    /// CIDR ranges of trusted reverse proxies. When non-empty, the
    /// `X-Auth-Request-*` headers are honored only for requests whose connection
    /// peer IP falls within one of these ranges. Use a full-length prefix for a
    /// single host, e.g. `"10.0.0.5/32"` or `"::1/128"`.
    ///
    /// **Important — this matches the direct TCP peer.** It is only sound when the
    /// proxy is the immediate connection peer (e.g. an `nginx auth_request`
    /// sidecar in the same pod). If a load balancer, ingress, or Kubernetes
    /// `Service`/kube-proxy sits between the proxy and this app, the peer IP is the
    /// *load balancer*, not the proxy — so a CIDR allow-list will either deny
    /// everything (peer never in range) or, if widened to the LB range, let any
    /// workload routing through that hop spoof identity. **For load-balanced
    /// topologies use [`shared_secret`](Self::shared_secret) instead**, which does
    /// not depend on the peer IP.
    #[serde(default)]
    pub trusted_proxies: Vec<IpNet>,

    /// Shared secret the trusted proxy must echo in
    /// [`shared_secret_header`](Self::shared_secret_header). When set, identity
    /// headers are honored only if the request carries a matching secret.
    /// Supports `{{ ENV_VAR }}` substitution.
    #[serde(default)]
    pub shared_secret: Option<Sensitive<String>>,

    /// Name of the header carrying [`shared_secret`](Self::shared_secret)
    /// (default `X-Proxy-Secret`).
    #[serde(default = "HttpProxyOidcConfig::default_shared_secret_header")]
    pub shared_secret_header: String,
}

impl Default for HttpProxyOidcConfig {
    fn default() -> Self {
        Self {
            user_header: Self::default_user_header(),
            email_header: Self::default_email_header(),
            groups_header: Self::default_groups_header(),
            preferred_username_header: Self::default_preferred_username_header(),
            access_token_header: Self::default_access_token_header(),
            roles_header: Self::default_roles_header(),
            trusted_proxies: Vec::new(),
            shared_secret: None,
            shared_secret_header: Self::default_shared_secret_header(),
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

    fn default_roles_header() -> String {
        "X-Auth-Request-Roles".to_string()
    }

    fn default_shared_secret_header() -> String {
        "X-Proxy-Secret".to_string()
    }

    /// The configured shared secret, treating an empty/whitespace-only value as
    /// absent. An empty secret can arise from `shared_secret = "{{ VAR }}"` with
    /// `VAR` unset, and must **not** be honored — otherwise an attacker sending an
    /// empty secret header would be trusted.
    #[must_use]
    pub(crate) fn effective_shared_secret(&self) -> Option<&str> {
        self.shared_secret
            .as_ref()
            .map(|s| s.0.trim())
            .filter(|s| !s.is_empty())
    }

    /// Whether any proxy-trust mechanism (CIDR allow-list or a non-empty shared
    /// secret) is configured. When `false`, header trust falls back to the
    /// environment default (fail-closed in production).
    #[must_use]
    pub fn has_trust_anchor(&self) -> bool {
        !self.trusted_proxies.is_empty() || self.effective_shared_secret().is_some()
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
