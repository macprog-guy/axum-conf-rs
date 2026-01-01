use http::{HeaderName, Method};
use serde::Deserialize;
use std::{str::FromStr, time::Duration};

/// Configuration for Cross-Origin Resource Sharing (CORS).
///
/// CORS controls which web domains can make requests to your API from a browser.
/// All fields are optional - if not specified, permissive defaults will be used.
///
/// # Important Notes
///
/// - When `allow_credentials` is `true`, you cannot use wildcard (`*`) values
///   for origins, methods, or headers. Explicit values must be provided.
/// - If `allowed_origins` is not specified, all origins will be allowed
/// - If `allowed_methods` is not specified, all standard methods will be allowed
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// [http.cors]
/// allow_credentials = true
/// allowed_origins = ["https://app.example.com", "https://admin.example.com"]
/// allowed_methods = ["GET", "POST", "PUT", "DELETE"]
/// allowed_headers = ["content-type", "authorization"]
/// exposed_headers = ["x-request-id"]
/// max_age = "1h"
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HttpCorsConfig {
    /// Whether to allow credentials (cookies, authorization headers) in CORS requests.
    /// When true, `allowed_origins`, `allowed_methods`, and `allowed_headers` must
    /// be explicitly specified (no wildcards allowed).
    pub allow_credentials: Option<bool>,

    /// List of origins allowed to make CORS requests.
    /// For example: `["https://app.example.com", "https://admin.example.com"]`.
    /// If not specified, all origins are allowed.
    pub allowed_origins: Option<Vec<String>>,

    /// List of HTTP methods allowed in CORS requests.
    /// For example: `["GET", "POST", "PUT", "DELETE"]`.
    /// If not specified, all standard methods are allowed.
    pub allowed_methods: Option<Vec<CorsMethod>>,

    /// List of headers allowed in CORS requests.
    /// For example: `["content-type", "authorization", "x-api-key"]`.
    /// If not specified, common headers are allowed.
    pub allowed_headers: Option<Vec<CorsHeader>>,

    /// List of headers exposed to the browser in CORS responses.
    /// For example: `["x-request-id", "x-ratelimit-remaining"]`.
    /// These headers become accessible to JavaScript in the browser.
    pub exposed_headers: Option<Vec<CorsHeader>>,

    /// Maximum time (in seconds) that browsers should cache CORS preflight responses.
    /// For example: `"1h"` for 1 hour, `"30m"` for 30 minutes.
    /// This reduces the number of preflight requests made by browsers.
    #[serde(default, with = "humantime_serde")]
    pub max_age: Option<Duration>,
}

impl HttpCorsConfig {
    /// Enables credentials (cookies, authorization headers) in CORS requests.
    ///
    /// When credentials are enabled, you cannot use wildcard (`*`) values for
    /// origins, methods, or headers. You must explicitly specify allowed values.
    ///
    /// # Example
    ///
    /// ```rust
    /// use axum_conf::config::http::HttpCorsConfig;
    ///
    /// let cors = HttpCorsConfig::default()
    ///     .with_allow_credentials()
    ///     .with_allowed_origins(vec!["https://app.example.com".into()]);
    /// ```
    pub fn with_allow_credentials(mut self) -> Self {
        self.allow_credentials = Some(true);
        self
    }

    /// Sets the list of origins allowed to make CORS requests.
    ///
    /// Origins should be full URLs including the scheme (e.g., `https://example.com`).
    /// If not set, all origins are allowed (wildcard behavior).
    ///
    /// # Arguments
    ///
    /// * `origins` - List of allowed origin URLs
    ///
    /// # Example
    ///
    /// ```rust
    /// use axum_conf::config::http::HttpCorsConfig;
    ///
    /// let cors = HttpCorsConfig::default()
    ///     .with_allowed_origins(vec![
    ///         "https://app.example.com".into(),
    ///         "https://admin.example.com".into(),
    ///     ]);
    /// ```
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = Some(origins);
        self
    }

    /// Sets the HTTP methods allowed in CORS requests.
    ///
    /// Common methods include GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS.
    /// If not set, all standard methods are allowed.
    ///
    /// # Arguments
    ///
    /// * `methods` - List of allowed HTTP methods
    pub fn with_allowed_methods(mut self, methods: Vec<CorsMethod>) -> Self {
        self.allowed_methods = Some(methods);
        self
    }

    /// Sets the headers allowed in CORS requests.
    ///
    /// Common headers include `content-type`, `authorization`, `x-api-key`.
    /// If not set, common headers are allowed by default.
    ///
    /// # Arguments
    ///
    /// * `headers` - List of allowed request headers
    pub fn with_allowed_headers(mut self, headers: Vec<CorsHeader>) -> Self {
        self.allowed_headers = Some(headers);
        self
    }

    /// Sets the headers exposed to the browser in CORS responses.
    ///
    /// By default, browsers can only access a limited set of response headers.
    /// Use this to expose additional headers like `x-request-id` or `x-ratelimit-remaining`.
    ///
    /// # Arguments
    ///
    /// * `headers` - List of headers to expose to the browser
    pub fn with_exposed_headers(mut self, headers: Vec<CorsHeader>) -> Self {
        self.exposed_headers = Some(headers);
        self
    }

    /// Sets the maximum time browsers should cache CORS preflight responses.
    ///
    /// Longer cache times reduce the number of preflight requests, improving performance.
    /// A typical value is 1 hour (`Duration::from_secs(3600)`).
    ///
    /// # Arguments
    ///
    /// * `max_age` - Duration to cache preflight responses
    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = Some(max_age);
        self
    }
}

/// Wrapper type for HTTP methods in CORS configuration.
///
/// This type enables deserialization of HTTP methods from strings in TOML.
/// Valid method names include: GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS, etc.
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// allowed_methods = ["GET", "POST", "PUT"]
/// ```
#[derive(Debug, Clone)]
pub struct CorsMethod(pub Method);

impl<'de> Deserialize<'de> for CorsMethod {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let method = Method::from_str(&s).map_err(serde::de::Error::custom)?;
        Ok(CorsMethod(method))
    }
}

/// Wrapper type for HTTP header names in CORS configuration.
///
/// This type enables deserialization of HTTP header names from strings in TOML.
/// Header names are case-insensitive and will be validated according to HTTP standards.
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// allowed_headers = ["content-type", "authorization", "x-api-key"]
/// exposed_headers = ["x-request-id", "x-ratelimit-remaining"]
/// ```
#[derive(Debug, Clone)]
pub struct CorsHeader(pub HeaderName);

impl<'de> Deserialize<'de> for CorsHeader {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let header = HeaderName::from_str(&s).map_err(serde::de::Error::custom)?;
        Ok(CorsHeader(header))
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;
    use std::time::Duration;

    #[test]
    fn test_cors_config_default() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_none());
    }

    #[test]
    fn test_cors_config_empty() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.allowed_origins.is_none());
        assert!(cors.allowed_methods.is_none());
        assert!(cors.allowed_headers.is_none());
        assert!(cors.exposed_headers.is_none());
        assert!(cors.max_age.is_none());
        assert!(cors.allow_credentials.is_none());
    }

    #[test]
    fn test_cors_config_allowed_origins() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allowed_origins = ["https://example.com", "https://api.example.com"]
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.allowed_origins.is_some());
        let origins = cors.allowed_origins.unwrap();
        assert_eq!(origins.len(), 2);
        assert_eq!(origins[0], "https://example.com");
        assert_eq!(origins[1], "https://api.example.com");
    }

    #[test]
    fn test_cors_config_allowed_methods() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allowed_methods = ["GET", "POST", "PUT", "DELETE"]
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.allowed_methods.is_some());
        let methods = cors.allowed_methods.unwrap();
        assert_eq!(methods.len(), 4);
    }

    #[test]
    fn test_cors_config_allowed_headers() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allowed_headers = ["Content-Type", "Authorization", "X-Custom-Header"]
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.allowed_headers.is_some());
        let headers = cors.allowed_headers.unwrap();
        assert_eq!(headers.len(), 3);
    }

    #[test]
    fn test_cors_config_exposed_headers() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
exposed_headers = ["X-Total-Count", "X-Page-Number"]
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.exposed_headers.is_some());
        let headers = cors.exposed_headers.unwrap();
        assert_eq!(headers.len(), 2);
    }

    #[test]
    fn test_cors_config_max_age() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
max_age = "3600s"
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.max_age.is_some());
        assert_eq!(cors.max_age.unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn test_cors_config_allow_credentials() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allow_credentials = true
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.allow_credentials.is_some());
        assert!(cors.allow_credentials.unwrap());
    }

    #[test]
    fn test_cors_config_complete() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allowed_origins = ["https://example.com"]
allowed_methods = ["GET", "POST"]
allowed_headers = ["Content-Type", "Authorization"]
exposed_headers = ["X-Total-Count"]
max_age = "7200s"
allow_credentials = true
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();

        assert!(cors.allowed_origins.is_some());
        assert_eq!(cors.allowed_origins.unwrap().len(), 1);

        assert!(cors.allowed_methods.is_some());
        assert_eq!(cors.allowed_methods.unwrap().len(), 2);

        assert!(cors.allowed_headers.is_some());
        assert_eq!(cors.allowed_headers.unwrap().len(), 2);

        assert!(cors.exposed_headers.is_some());
        assert_eq!(cors.exposed_headers.unwrap().len(), 1);

        assert!(cors.max_age.is_some());
        assert_eq!(cors.max_age.unwrap(), Duration::from_secs(7200));

        assert!(cors.allow_credentials.is_some());
        assert!(cors.allow_credentials.unwrap());
    }

    #[test]
    fn test_cors_config_custom_method() {
        // HTTP spec allows custom method names, so CUSTOM_METHOD should parse successfully
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allowed_methods = ["GET", "CUSTOM_METHOD"]
        "#;

        let result = config_str.parse::<Config>();
        assert!(result.is_ok());
        let config = result.unwrap();
        assert!(config.http.cors.is_some());
        let cors = config.http.cors.unwrap();
        assert!(cors.allowed_methods.is_some());
        assert_eq!(cors.allowed_methods.unwrap().len(), 2);
    }

    #[test]
    fn test_cors_config_invalid_header() {
        let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.cors]
allowed_headers = ["Invalid Header Name!"]
        "#;

        let result = config_str.parse::<Config>();
        assert!(result.is_err());
    }
}
