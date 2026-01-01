use crate::Result;

#[cfg(any(feature = "deduplication", feature = "keycloak"))]
use crate::Error;
use serde::Deserialize;

/// Configuration for which middleware to enable or disable.
///
/// By default, all middleware are enabled. Use this to selectively include or exclude
/// specific middleware from the stack.
///
/// # Variants
///
/// - `Include` - Only enable the specified middleware (whitelist approach)
/// - `Exclude` - Enable all middleware except those specified (blacklist approach)
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// # Exclude specific middleware
/// [http.middleware]
/// exclude = ["rate-limiting", "metrics"]
///
/// # Or include only specific middleware
/// [http.middleware]
/// include = ["logging", "cors", "liveness", "readiness"]
/// ```
#[derive(Debug, Clone, Deserialize)]
pub enum HttpMiddlewareConfig {
    /// Only enable the middleware in this list. All others will be disabled.
    Include(Vec<HttpMiddleware>),
    /// Enable all middleware except those in this list.
    Exclude(Vec<HttpMiddleware>),
}

impl HttpMiddlewareConfig {
    /// Checks if a specific middleware is enabled based on this configuration.
    ///
    /// For `Include` configs, returns `true` if the middleware is in the list.
    /// For `Exclude` configs, returns `true` if the middleware is NOT in the list.
    ///
    /// # Arguments
    ///
    /// * `middleware` - The middleware to check
    ///
    /// # Returns
    ///
    /// `true` if the middleware should be enabled, `false` otherwise.
    pub fn is_enabled(&self, middleware: HttpMiddleware) -> bool {
        match self {
            HttpMiddlewareConfig::Include(list) => list.contains(&middleware),
            HttpMiddlewareConfig::Exclude(list) => !list.contains(&middleware),
        }
    }

    /// Validates middleware dependencies are satisfied.
    ///
    /// # Dependencies
    ///
    /// - `RequestDeduplication` requires `RequestId` (uses x-request-id header as idempotency key)
    /// - `Oidc` requires `Session` (when keycloak feature is enabled)
    pub fn validate(&self) -> Result<()> {
        // RequestDeduplication depends on RequestId (only when deduplication feature is enabled)
        #[cfg(feature = "deduplication")]
        if self.is_enabled(HttpMiddleware::RequestDeduplication)
            && !self.is_enabled(HttpMiddleware::RequestId)
        {
            return Err(Error::invalid_input(
                "RequestDeduplication requires RequestId. Remove 'request-id' from Exclude list or add both to Include list.",
            ));
        }

        // Oidc depends on Session (when keycloak feature is enabled)
        #[cfg(feature = "keycloak")]
        if self.is_enabled(HttpMiddleware::Oidc) && !self.is_enabled(HttpMiddleware::Session) {
            return Err(Error::invalid_input(
                "Oidc requires Session middleware. Remove 'session' from Exclude list or add both to Include list.",
            ));
        }

        // Note: BasicAuth and Oidc mutual exclusion is enforced at config level
        // (in HttpConfig::validate()) rather than middleware level, since
        // middleware can be "enabled" by default even without config.

        Ok(())
    }
}

/// Available middleware that can be enabled or disabled in the HTTP stack.
///
/// These middleware are applied in a specific order (see [`crate::fluent::FluentRouter::setup_middleware`]).
/// Use [`HttpMiddlewareConfig`] to include or exclude specific middleware.
///
/// # TOML Names
///
/// In configuration files, use kebab-case names (e.g., `rate-limiting`, `catch-panic`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HttpMiddleware {
    /// OIDC/Keycloak authentication middleware.
    /// Validates JWT tokens and extracts user information.
    /// Requires the `keycloak` feature and `Session` middleware.
    Oidc,

    /// HTTP Basic Authentication and API key middleware.
    /// Requires the `basic-auth` feature.
    /// Cannot be used together with `Oidc`.
    #[cfg(feature = "basic-auth")]
    BasicAuth,

    /// Request deduplication middleware.
    /// Prevents duplicate processing of requests with the same request ID.
    /// Requires `RequestId` middleware to be enabled.
    /// Requires the `deduplication` feature.
    RequestDeduplication,

    /// Rate limiting middleware.
    /// Limits requests per IP address to prevent abuse.
    /// Configured via `http.max_requests_per_sec` in TOML.
    /// Requires the `rate-limiting` feature.
    RateLimiting,

    /// Concurrency limit middleware.
    /// Limits the maximum number of concurrent requests.
    /// Configured via `http.max_concurrent_requests` in TOML.
    /// Requires the `concurrency-limit` feature.
    ConcurrencyLimit,

    /// Request payload size limit middleware.
    /// Rejects requests exceeding the configured body size.
    /// Configured via `http.max_payload_size_bytes` in TOML.
    /// Requires the `payload-limit` feature.
    MaxPayloadSize,

    /// Response compression middleware.
    /// Compresses responses using gzip, brotli, deflate, or zstd.
    /// Requires the `compression` feature.
    Compression,

    /// Path normalization middleware.
    /// Normalizes URL paths by handling trailing slashes consistently.
    /// Requires the `path-normalization` feature.
    PathNormalization,

    /// Sensitive header redaction middleware.
    /// Redacts sensitive headers (like Authorization) from logs.
    /// Requires the `sensitive-headers` feature.
    SensitiveHeaders,

    /// Request ID middleware.
    /// Adds a unique UUIDv7 identifier to each request.
    /// The ID is available via the `x-request-id` header.
    RequestId,

    /// API versioning middleware.
    /// Extracts API version from path, header, or query parameter.
    /// Requires the `api-versioning` feature.
    ApiVersioning,

    /// CORS (Cross-Origin Resource Sharing) middleware.
    /// Handles preflight requests and adds CORS headers.
    /// Configured via `http.cors` in TOML.
    /// Requires the `cors` feature.
    Cors,

    /// Security headers middleware.
    /// Adds security headers like X-Frame-Options, X-Content-Type-Options.
    /// Requires the `security-headers` feature.
    SecurityHeaders,

    /// Request/response logging middleware.
    /// Logs request method, path, status, and duration.
    /// Uses the configured log format (JSON, compact, etc.).
    Logging,

    /// Prometheus metrics middleware.
    /// Exposes metrics at the `/metrics` endpoint.
    /// Requires the `metrics` feature.
    Metrics,

    /// Liveness probe endpoint middleware.
    /// Adds a `/live` endpoint for Kubernetes liveness probes.
    /// Returns 200 OK if the service is running.
    Liveness,

    /// Readiness probe endpoint middleware.
    /// Adds a `/ready` endpoint for Kubernetes readiness probes.
    /// Returns 200 OK if the service is ready to accept traffic.
    Readiness,

    /// Request timeout middleware.
    /// Aborts requests that exceed the configured timeout.
    /// Configured via `http.request_timeout` in TOML.
    Timeout,

    /// Panic catching middleware.
    /// Catches panics in handlers and returns a 500 error.
    /// Prevents panics from crashing the server.
    CatchPanic,

    /// Session management middleware.
    /// Provides cookie-based session storage.
    /// Requires the `session` feature.
    #[cfg(feature = "session")]
    Session,

    /// OpenTelemetry tracing middleware.
    /// Adds distributed tracing spans to requests.
    /// Requires the `opentelemetry` feature.
    #[cfg(feature = "opentelemetry")]
    OpenTelemetry,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_enabled_include() {
        let config =
            HttpMiddlewareConfig::Include(vec![HttpMiddleware::Logging, HttpMiddleware::Metrics]);
        assert!(config.is_enabled(HttpMiddleware::Logging));
        assert!(config.is_enabled(HttpMiddleware::Metrics));
        assert!(!config.is_enabled(HttpMiddleware::Cors));
    }

    #[test]
    fn test_is_enabled_exclude() {
        let config = HttpMiddlewareConfig::Exclude(vec![HttpMiddleware::RateLimiting]);
        assert!(!config.is_enabled(HttpMiddleware::RateLimiting));
        assert!(config.is_enabled(HttpMiddleware::Logging));
        assert!(config.is_enabled(HttpMiddleware::Cors));
    }

    #[cfg(feature = "deduplication")]
    #[test]
    fn test_validate_deduplication_without_request_id_fails() {
        // Exclude RequestId but keep RequestDeduplication enabled
        let config = HttpMiddlewareConfig::Exclude(vec![HttpMiddleware::RequestId]);
        let result = config.validate();
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("RequestDeduplication requires RequestId")
        );
    }

    #[test]
    fn test_validate_deduplication_with_request_id_succeeds() {
        // Both enabled (nothing excluded)
        let config = HttpMiddlewareConfig::Exclude(vec![HttpMiddleware::RateLimiting]);
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_deduplication_both_disabled_succeeds() {
        // Both disabled - no dependency issue
        let config = HttpMiddlewareConfig::Exclude(vec![
            HttpMiddleware::RequestDeduplication,
            HttpMiddleware::RequestId,
        ]);
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[cfg(feature = "deduplication")]
    #[test]
    fn test_validate_include_deduplication_without_request_id_fails() {
        // Include only deduplication without RequestId
        let config = HttpMiddlewareConfig::Include(vec![HttpMiddleware::RequestDeduplication]);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[cfg(feature = "deduplication")]
    #[test]
    fn test_validate_include_deduplication_with_request_id_succeeds() {
        // Include both - should work
        let config = HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::RequestDeduplication,
            HttpMiddleware::RequestId,
        ]);
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[cfg(feature = "keycloak")]
    #[test]
    fn test_validate_oidc_without_session_fails() {
        // Exclude Session but keep Oidc enabled
        let config = HttpMiddlewareConfig::Exclude(vec![HttpMiddleware::Session]);
        let result = config.validate();
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Oidc requires Session"));
    }

    #[cfg(feature = "keycloak")]
    #[test]
    fn test_validate_oidc_with_session_succeeds() {
        // Both enabled (nothing excluded)
        let config = HttpMiddlewareConfig::Exclude(vec![HttpMiddleware::RateLimiting]);
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[cfg(feature = "keycloak")]
    #[test]
    fn test_validate_include_oidc_without_session_fails() {
        // Include only Oidc without Session
        let config = HttpMiddlewareConfig::Include(vec![HttpMiddleware::Oidc]);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[cfg(feature = "keycloak")]
    #[test]
    fn test_validate_include_oidc_with_session_succeeds() {
        // Include both - should work
        let config =
            HttpMiddlewareConfig::Include(vec![HttpMiddleware::Oidc, HttpMiddleware::Session]);
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_empty_exclude_succeeds() {
        let config = HttpMiddlewareConfig::Exclude(vec![]);
        let result = config.validate();
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_empty_include_succeeds() {
        // No middleware enabled means no dependencies to check
        let config = HttpMiddlewareConfig::Include(vec![]);
        let result = config.validate();
        assert!(result.is_ok());
    }
}
