mod cors;
mod dedup;
mod middleware;
mod oidc;
mod staticdir;
#[cfg(feature = "basic-auth")]
mod basic_auth;
#[cfg(feature = "circuit-breaker")]
mod circuit_breaker;

pub use cors::*;
pub use dedup::*;
pub use middleware::*;
#[cfg(feature = "keycloak")]
pub use oidc::*;
pub use staticdir::*;
#[cfg(feature = "basic-auth")]
pub use basic_auth::*;
#[cfg(feature = "circuit-breaker")]
pub use circuit_breaker::*;

use {crate::Result, serde::Deserialize, std::fmt, std::time::Duration};

/// X-Frame-Options header value configuration.
///
/// This enum is used for configuration parsing. When the `security-headers` feature
/// is enabled, it will be converted to the actual `axum_helmet::XFrameOptions` value.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum XFrameOptions {
    /// Prevents the page from being displayed in a frame
    #[default]
    Deny,
    /// Allows the page to be displayed in a frame on the same origin
    SameOrigin,
    /// Allows the page to be displayed in a frame on the specified origin
    #[serde(rename = "ALLOW-FROM")]
    AllowFrom(String),
}

impl fmt::Display for XFrameOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XFrameOptions::Deny => write!(f, "DENY"),
            XFrameOptions::SameOrigin => write!(f, "SAMEORIGIN"),
            XFrameOptions::AllowFrom(url) => write!(f, "ALLOW-FROM {}", url),
        }
    }
}

///
/// Configuration for the HTTP server
///
/// This configuration includes many settings that control the behavior
/// of the HTTP server, including binding address and port, request limits,
/// timeouts, and specific route paths.
///
#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    /// IP address to bind the HTTP server to
    /// The default `bind_addr` is "127.0.0.1".
    #[serde(default = "HttpConfig::default_bind_addr")]
    pub bind_addr: String,

    /// Port to bind the HTTP server to
    /// The default `bind_port` is 3000.
    #[serde(default = "HttpConfig::default_bind_port")]
    pub bind_port: u16,

    /// Maximum number of concurrent requests to handle.
    /// If the number of concurrent requests exceeds this number, new requests
    /// will be rejected with a 503 Service Unavailable response.
    /// By default `max_concurrent_requests` is set to 4096.
    #[serde(default = "HttpConfig::default_max_concurrent_requests")]
    pub max_concurrent_requests: u32,

    /// Maximum number of request per second (per IP address).
    /// If the rate is exceeded, new requests to the server will be rejected
    /// with a 429 Too Many Requests. The default is 100 requests per second.
    #[serde(default = "HttpConfig::default_max_requests_per_sec")]
    pub max_requests_per_sec: u32,

    /// Maximum allowed time for a request to complete before timing out.
    /// If a request takes longer than this it will be aborted with a 408
    /// Request Timeout response. Too many such responses in a short time
    /// interval will make the server unavailable during a readiness check.
    /// By default `request_timeout` is None.
    #[serde(default, with = "humantime_serde")]
    pub request_timeout: Option<Duration>,

    /// Maximum payload size in bytes for incoming HTTP requests.
    /// Requests with payloads larger than this will be rejected with
    /// a 413 Payload Too Large response.
    /// By default `max_payload_size_bytes` is set to 32KiB.
    pub max_payload_size_bytes: byte_unit::Byte,

    /// Whether or not to support gzip/brotli/deflate/zstd request and
    /// response compression. By default compression is disabled.
    #[serde(default)]
    pub support_compression: bool,

    /// Whether or not to expose Prometheus metrics endpoint.
    /// By default `with_metrics` is set to true.
    #[serde(default = "HttpConfig::default_with_metrics")]
    pub with_metrics: bool,

    /// Whether or not to trim trailing slashes from the request path.
    /// By default `trim_trailing_slash` is set to true.
    #[serde(default = "HttpConfig::default_trim_trailing_slash")]
    pub trim_trailing_slash: bool,

    /// Route for liveness checks.
    /// By default `liveness` is "/live".
    #[serde(default = "HttpConfig::default_liveness_route")]
    pub liveness_route: String,

    /// Route for readiness checks.
    /// The readiness check will return a 429 Too Many Requests when unable
    /// to handle the load. By default `readiness` is set to "/ready".
    #[serde(default = "HttpConfig::default_readiness_route")]
    pub readiness_route: String,

    /// Route for metrics.
    /// Our Kubernetes infrastructure can scrape this endpoint for
    /// Prometheus metrics. By default `metrics` is set to "/metrics".
    #[serde(default = "HttpConfig::default_metrics_route")]
    pub metrics_route: String,

    /// Whether to set the X-Content-Type-Options header to "nosniff".
    /// By default `x_content_type_nosniff` is set to true.
    #[serde(default = "HttpConfig::default_x_content_type_nosniff")]
    pub x_content_type_nosniff: bool,

    /// Whether to set the X-Frame-Options header to "DENY", "SAMEORIGIN" or a URI.
    /// By default `x_frame_options` is set to "DENY".
    #[serde(default = "HttpConfig::default_x_frame_options")]
    pub x_frame_options: HttpXFrameConfig,

    /// Configuration for serving static files.
    #[serde(default)]
    pub directories: Vec<StaticDirConfig>,

    /// OIDC authentication configuration.
    /// Only included if the "keycloak" feature is enabled.
    /// When None, OIDC authentication is disabled.
    #[cfg(feature = "keycloak")]
    #[serde(default)]
    pub oidc: Option<HttpOidcConfig>,

    /// Basic Auth / API Key configuration.
    /// Only included if the "basic-auth" feature is enabled.
    /// When None, basic authentication is disabled.
    #[cfg(feature = "basic-auth")]
    #[serde(default)]
    pub basic_auth: Option<HttpBasicAuthConfig>,

    /// CORS configuration. If not present defaults to permissive CORS.
    pub cors: Option<HttpCorsConfig>,

    /// Default API version to use when clients don't specify one.
    /// Used by the API versioning middleware. Defaults to 1.
    #[serde(default = "HttpConfig::default_api_version")]
    pub default_api_version: u32,

    /// Request deduplication configuration.
    /// When enabled, duplicate requests (same request-id) will be rejected
    /// while a request is still being processed.
    #[serde(default)]
    pub deduplication: Option<HttpDeduplicationConfig>,

    /// Maximum time to wait for graceful shutdown to complete.
    /// After this timeout, the server will force shutdown.
    /// By default `shutdown_timeout` is set to 30 seconds.
    #[serde(
        default = "HttpConfig::default_shutdown_timeout",
        with = "humantime_serde"
    )]
    pub shutdown_timeout: Duration,

    #[serde(flatten)]
    pub middleware: Option<HttpMiddlewareConfig>,
}

impl HttpConfig {
    ///
    /// Returns the full bind address as a string in the format "IP:PORT".
    ///
    pub fn full_bind_addr(&self) -> String {
        format!("{}:{}", self.bind_addr, self.bind_port)
    }

    fn default_bind_addr() -> String {
        "127.0.0.1".into()
    }

    fn default_bind_port() -> u16 {
        3000
    }

    fn default_max_concurrent_requests() -> u32 {
        4096
    }

    fn default_max_requests_per_sec() -> u32 {
        100
    }

    fn default_max_payload_size_bytes() -> byte_unit::Byte {
        byte_unit::Byte::from_u64(32 * 1024)
    }

    fn default_trim_trailing_slash() -> bool {
        true
    }
    fn default_with_metrics() -> bool {
        true
    }
    fn default_liveness_route() -> String {
        "/live".into()
    }

    fn default_readiness_route() -> String {
        "/ready".into()
    }

    fn default_metrics_route() -> String {
        "/metrics".into()
    }

    fn default_x_content_type_nosniff() -> bool {
        true
    }
    fn default_x_frame_options() -> HttpXFrameConfig {
        HttpXFrameConfig(XFrameOptions::Deny)
    }

    fn default_api_version() -> u32 {
        1
    }

    fn default_shutdown_timeout() -> Duration {
        Duration::from_secs(30)
    }

    pub fn validate(&self) -> Result<()> {
        // Validate bind address
        if self.bind_addr.trim().is_empty() {
            return Err(crate::Error::invalid_input(
                "HTTP bind_addr is required. Set [http] bind_addr = \"0.0.0.0\" or \"127.0.0.1\" in config.",
            ));
        }

        // Validate bind address format (basic IP address validation)
        if self.bind_addr.parse::<std::net::IpAddr>().is_err() {
            return Err(crate::Error::invalid_input(
                "HTTP bind_addr must be a valid IP address. Examples: \"127.0.0.1\", \"0.0.0.0\", \"::1\"",
            ));
        }

        // Validate max_concurrent_requests is not zero
        if self.max_concurrent_requests == 0 {
            return Err(crate::Error::invalid_input(
                "HTTP max_concurrent_requests must be > 0. Set [http] max_concurrent_requests = 4096 in config.",
            ));
        }

        #[cfg(feature = "keycloak")]
        if let Some(oidc_config) = &self.oidc {
            oidc_config.validate()?;
        }

        #[cfg(feature = "basic-auth")]
        if let Some(basic_auth_config) = &self.basic_auth {
            basic_auth_config.validate()?;
        }

        // Mutual exclusion: basic_auth and oidc cannot both be configured
        #[cfg(all(feature = "basic-auth", feature = "keycloak"))]
        if self.basic_auth.is_some() && self.oidc.is_some() {
            return Err(crate::Error::invalid_input(
                "Cannot configure both [http.basic_auth] and [http.oidc]. Choose one authentication method.",
            ));
        }

        // Validate individual static directories
        for dir in &self.directories {
            dir.validate()?;
        }

        // Validate that there's at most one fallback directory
        let fallback_count = self.directories.iter().filter(|d| d.is_fallback()).count();
        if fallback_count > 1 {
            return Err(crate::Error::invalid_input(
                "Only one static directory can be configured as fallback",
            ));
        }

        // Validate middleware dependencies
        if let Some(middleware_config) = &self.middleware {
            middleware_config.validate()?;
        }

        // Warn if CORS is not explicitly configured (will use permissive defaults)
        if self.cors.is_none() {
            tracing::warn!(
                "No CORS configuration found. Permissive defaults will be used, \
                 which allows all origins. Consider configuring explicit CORS rules \
                 for production environments."
            );
        }

        Ok(())
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            bind_addr: Self::default_bind_addr(),
            bind_port: Self::default_bind_port(),
            max_payload_size_bytes: Self::default_max_payload_size_bytes(),
            max_concurrent_requests: Self::default_max_concurrent_requests(),
            max_requests_per_sec: Self::default_max_requests_per_sec(),
            support_compression: false,
            with_metrics: Self::default_with_metrics(),
            trim_trailing_slash: Self::default_trim_trailing_slash(),
            request_timeout: None,
            liveness_route: Self::default_liveness_route(),
            readiness_route: Self::default_readiness_route(),
            metrics_route: Self::default_metrics_route(),
            x_content_type_nosniff: Self::default_x_content_type_nosniff(),
            x_frame_options: Self::default_x_frame_options(),
            default_api_version: Self::default_api_version(),
            directories: Vec::new(),
            #[cfg(feature = "keycloak")]
            oidc: None,
            #[cfg(feature = "basic-auth")]
            basic_auth: None,
            cors: None,
            deduplication: None,
            shutdown_timeout: Self::default_shutdown_timeout(),
            middleware: None,
        }
    }
}

/// Configuration wrapper for the X-Frame-Options security header.
///
/// This wrapper enables custom TOML deserialization for the `XFrameOptions` enum
/// from the `axum-helmet` crate. It supports three variants:
///
/// - `"DENY"` or `"deny"` - Prevents the page from being displayed in a frame
/// - `"SAMEORIGIN"` or `"sameorigin"` - Allows framing only from the same origin
/// - Any other string - Treated as `AllowFrom(url)` to allow specific origins
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// x_frame_options = "DENY"           # Denies all framing
/// x_frame_options = "SAMEORIGIN"     # Same-origin only
/// x_frame_options = "https://example.com"  # Allow specific origin
/// ```
#[derive(Clone)]
pub struct HttpXFrameConfig(pub XFrameOptions);

impl HttpXFrameConfig {
    pub fn deny() -> Self {
        HttpXFrameConfig(XFrameOptions::Deny)
    }
    pub fn same_origin() -> Self {
        HttpXFrameConfig(XFrameOptions::SameOrigin)
    }
    pub fn allow_from(url: impl Into<String>) -> Self {
        HttpXFrameConfig(XFrameOptions::AllowFrom(url.into()))
    }
}

impl Default for HttpXFrameConfig {
    fn default() -> Self {
        HttpXFrameConfig(XFrameOptions::SameOrigin)
    }
}

impl std::fmt::Debug for HttpXFrameConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for HttpXFrameConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let x_frame_options = match s.to_uppercase().as_str() {
            "DENY" => XFrameOptions::Deny,
            "SAMEORIGIN" => XFrameOptions::SameOrigin,
            _ => XFrameOptions::AllowFrom(s),
        };
        Ok(HttpXFrameConfig(x_frame_options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn test_x_content_type_nosniff_default() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.x_content_type_nosniff);
    }

    #[test]
    fn test_x_content_type_nosniff_enabled() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_content_type_nosniff = true
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.x_content_type_nosniff);
    }

    #[test]
    fn test_x_content_type_nosniff_disabled() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_content_type_nosniff = false
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(!config.http.x_content_type_nosniff);
    }

    #[test]
    fn test_x_frame_options_default() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(matches!(config.http.x_frame_options.0, XFrameOptions::Deny));
    }

    #[test]
    fn test_x_frame_options_deny() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_frame_options = "DENY"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(matches!(config.http.x_frame_options.0, XFrameOptions::Deny));
    }

    #[test]
    fn test_x_frame_options_deny_lowercase() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_frame_options = "deny"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(matches!(config.http.x_frame_options.0, XFrameOptions::Deny));
    }

    #[test]
    fn test_x_frame_options_sameorigin() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_frame_options = "SAMEORIGIN"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(matches!(
            config.http.x_frame_options.0,
            XFrameOptions::SameOrigin
        ));
    }

    #[test]
    fn test_x_frame_options_sameorigin_lowercase() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_frame_options = "sameorigin"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(matches!(
            config.http.x_frame_options.0,
            XFrameOptions::SameOrigin
        ));
    }

    #[test]
    fn test_x_frame_options_allow_from() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_frame_options = "https://example.com"
        "#;

        let config: Config = config_str.parse().unwrap();
        match &config.http.x_frame_options.0 {
            XFrameOptions::AllowFrom(url) => {
                assert_eq!(url, "https://example.com");
            }
            _ => panic!("Expected AllowFrom variant"),
        }
    }

    #[test]
    fn test_x_frame_options_allow_from_with_port() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_frame_options = "https://example.com:8080"
        "#;

        let config: Config = config_str.parse().unwrap();
        match &config.http.x_frame_options.0 {
            XFrameOptions::AllowFrom(url) => {
                assert_eq!(url, "https://example.com:8080");
            }
            _ => panic!("Expected AllowFrom variant"),
        }
    }

    #[test]
    fn test_security_headers_combined() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
x_content_type_nosniff = true
x_frame_options = "SAMEORIGIN"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.x_content_type_nosniff);
        assert!(matches!(
            config.http.x_frame_options.0,
            XFrameOptions::SameOrigin
        ));
    }

    #[test]
    fn test_security_headers_in_default_config() {
        let config = Config::default();
        assert!(config.http.x_content_type_nosniff);
        assert!(matches!(config.http.x_frame_options.0, XFrameOptions::Deny));
    }
}
