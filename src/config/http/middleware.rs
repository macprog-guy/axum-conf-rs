use crate::Result;

#[cfg(any(feature = "deduplication", feature = "keycloak"))]
use crate::Error;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub enum HttpMiddlewareConfig {
    Include(Vec<HttpMiddleware>),
    Exclude(Vec<HttpMiddleware>),
}

impl HttpMiddlewareConfig {
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

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HttpMiddleware {
    Oidc,
    #[cfg(feature = "basic-auth")]
    BasicAuth,
    RequestDeduplication,
    RateLimiting,
    ConcurrencyLimit,
    MaxPayloadSize,
    Compression,
    PathNormalization,
    SensitiveHeaders,
    RequestId,
    ApiVersioning,
    Cors,
    SecurityHeaders,
    Logging,
    Metrics,
    Liveness,
    Readiness,
    Timeout,
    CatchPanic,
    #[cfg(feature = "session")]
    Session,
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
