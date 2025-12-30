use {
    crate::{Error, Result},
    serde::Deserialize,
};

/// Configuration for serving static files from a directory.
///
/// This configuration allows serving static files (HTML, CSS, JavaScript, images, etc.)
/// either at a specific route prefix or as a fallback handler. Multiple directories
/// can be configured, but only one can be a fallback.
///
/// # Fields
///
/// - `directory`: Path to the directory containing static files (relative or absolute)
/// - `route`: How to route requests to this directory (specific route or fallback)
/// - `protected`: Whether authentication is required to access these files (default: false)
///
/// # Protected Directories
///
/// When `protected = true`, the directory requires OIDC authentication. **Important**:
///
/// - **Requires `keycloak` feature**: The `keycloak` feature must be enabled in `Cargo.toml`
/// - **Requires OIDC configuration**: The `[http.oidc]` section must be configured
/// - **Middleware order matters**: Protected directories must be set up *after* OIDC middleware
///
/// The authentication flow works as follows:
/// 1. User requests a file from the protected directory
/// 2. OIDC middleware validates the JWT token from the request
/// 3. If valid, the request proceeds to serve the file
/// 4. If invalid or missing, the request is rejected with `401 Unauthorized`
///
/// ## Setup Order for Protected Directories
///
/// When using `setup_middleware()`, protected directories are handled automatically.
/// For manual setup, call `setup_protected_files()` *after* `setup_oidc()`:
///
/// ```rust,ignore
/// FluentRouter::without_state(config)?
///     .setup_oidc()?                 // Set up OIDC first
///     .setup_protected_files()?      // Then protected directories
///     .setup_public_files()?         // Public files don't need OIDC
///     // ... other middleware
/// ```
///
/// # Restrictions
///
/// - Fallback directories cannot be protected (authentication is incompatible with fallback routing)
/// - Only one directory can be configured as a fallback
/// - Protected directories require the `keycloak` feature and OIDC configuration
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// # Public static assets - no authentication required
/// [[http.directories]]
/// directory = "./public"
/// route = "/static"
///
/// # Protected downloads - requires valid JWT token
/// [[http.directories]]
/// directory = "./downloads"
/// route = "/downloads"
/// protected = true
///
/// # SPA fallback - cannot be protected
/// [[http.directories]]
/// directory = "./dist"
/// fallback = true
/// ```
///
/// OIDC configuration (required for protected directories):
/// ```toml
/// [http.oidc]
/// issuer_url = "https://keycloak.example.com/realms/myrealm"
/// realm = "myrealm"
/// audiences = ["my-client"]
/// client_id = "my-client"
/// client_secret = "{{ OIDC_CLIENT_SECRET }}"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct StaticDirConfig {
    /// Path to the directory containing static files to serve.
    pub directory: String,

    /// Routing configuration - either a route prefix or fallback mode.
    #[serde(flatten)]
    pub route: StaticDirRoute,

    /// Whether authentication is required to access files in this directory.
    /// Cannot be true when using fallback routing.
    #[serde(default)]
    pub protected: bool,

    /// Maximum age for Cache-Control header in seconds.
    /// If set, adds `Cache-Control: public, max-age={value}` header to responses.
    /// Common values: 3600 (1 hour), 86400 (1 day), 31536000 (1 year).
    /// If None, no Cache-Control header is added.
    #[serde(default)]
    pub cache_max_age: Option<u64>,
}

/// Maximum allowed cache_max_age value (1 year in seconds).
/// This is the practical upper limit for HTTP Cache-Control max-age.
pub const MAX_CACHE_AGE_SECONDS: u64 = 31_536_000;

impl StaticDirConfig {
    pub fn is_fallback(&self) -> bool {
        matches!(self.route, StaticDirRoute::Fallback(_))
    }
    pub fn validate(&self) -> Result<()> {
        // Validate directory path is not empty
        if self.directory.trim().is_empty() {
            return Err(Error::invalid_input(
                "Static directory path is required. Set [[http.directories]] directory = \"./public\" in config.",
            ));
        }

        // Validate route path is not empty (for Route variant)
        if let StaticDirRoute::Route(route_path) = &self.route
            && route_path.trim().is_empty()
        {
            return Err(Error::invalid_input(
                "Static directory route is required. Set route = \"/static\" or use fallback = true.",
            ));
        }

        if self.is_fallback() && self.protected {
            return Err(Error::invalid_input(
                "Fallback directories cannot be protected. Remove protected = true or use route = \"/path\" instead.",
            ));
        }

        // Validate cache_max_age is within reasonable bounds
        if let Some(max_age) = self.cache_max_age
            && max_age > MAX_CACHE_AGE_SECONDS
        {
            return Err(Error::invalid_input(
                "cache_max_age exceeds 31536000 (1 year). Use values like 86400 (1 day) or 604800 (1 week).",
            ));
        }

        Ok(())
    }
}

/// Routing configuration for static file directories.
///
/// This enum determines how static files are served - either at a specific route
/// prefix or as a fallback handler for unmatched routes.
///
/// # Variants
///
/// - `Route(String)` - Serves static files at the specified route prefix.
///   For example, `route = "/static"` serves files at `/static/*`.
///
/// - `Fallback(bool)` - When `true`, serves static files as a fallback for any
///   unmatched routes. Useful for single-page applications where the index.html
///   should be served for client-side routing. Only one fallback directory is allowed.
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// # Serve at a specific route
/// [[http.directories]]
/// directory = "./public"
/// route = "/static"
///
/// # Serve as fallback (for SPAs)
/// [[http.directories]]
/// directory = "./dist"
/// fallback = true
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StaticDirRoute {
    /// Serve static files at the specified route prefix.
    /// For example, `Route("/assets")` serves files at `/assets/*`.
    Route(String),

    /// When true, serve static files as a fallback for unmatched routes.
    /// Useful for single-page applications with client-side routing.
    Fallback(bool),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn test_static_dir_config_parsing() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "route"

        [[http.directories]]
        directory = "public"
        fallback = true
        "#;

        let config = config_str.parse::<Config>().unwrap();
        assert_eq!(config.http.directories[0].directory, "static");
        assert!(matches!(
            config.http.directories[0].route,
            StaticDirRoute::Route(_)
        ));

        assert_eq!(config.http.directories[1].directory, "public");
        assert!(matches!(
            config.http.directories[1].route,
            StaticDirRoute::Fallback(_)
        ));
    }

    // ========================================================================
    // Edge case tests for cache_max_age bounds
    // ========================================================================

    #[test]
    fn test_cache_max_age_zero_is_valid() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 0
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.directories[0].validate().is_ok());
        assert_eq!(config.http.directories[0].cache_max_age, Some(0));
    }

    #[test]
    fn test_cache_max_age_one_second_is_valid() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 1
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.directories[0].validate().is_ok());
        assert_eq!(config.http.directories[0].cache_max_age, Some(1));
    }

    #[test]
    fn test_cache_max_age_one_hour_is_valid() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 3600
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.directories[0].validate().is_ok());
        assert_eq!(config.http.directories[0].cache_max_age, Some(3600));
    }

    #[test]
    fn test_cache_max_age_one_day_is_valid() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 86400
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.directories[0].validate().is_ok());
        assert_eq!(config.http.directories[0].cache_max_age, Some(86400));
    }

    #[test]
    fn test_cache_max_age_one_week_is_valid() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 604800
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.directories[0].validate().is_ok());
        assert_eq!(config.http.directories[0].cache_max_age, Some(604800));
    }

    #[test]
    fn test_cache_max_age_exactly_one_year_is_valid() {
        // Boundary: exactly MAX_CACHE_AGE_SECONDS (31536000 = 1 year)
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 31536000
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(
            config.http.directories[0].validate().is_ok(),
            "Exactly 1 year (31536000 seconds) should be valid"
        );
        assert_eq!(
            config.http.directories[0].cache_max_age,
            Some(MAX_CACHE_AGE_SECONDS)
        );
    }

    #[test]
    fn test_cache_max_age_one_second_over_limit_fails() {
        // Boundary: MAX_CACHE_AGE_SECONDS + 1
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 31536001
        "#;

        let config: Config = config_str.parse().unwrap();
        let result = config.http.directories[0].validate();
        assert!(
            result.is_err(),
            "One second over 1 year should fail validation"
        );
    }

    #[test]
    fn test_cache_max_age_two_years_fails() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 63072000
        "#;

        let config: Config = config_str.parse().unwrap();
        let result = config.http.directories[0].validate();
        assert!(
            result.is_err(),
            "Two years (63072000 seconds) should fail validation"
        );
    }

    #[test]
    fn test_cache_max_age_u64_max_fails() {
        // Extreme boundary: u64::MAX
        let config_str = format!(
            r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = {}
        "#,
            u64::MAX
        );

        let config: Config = config_str.parse().unwrap();
        let result = config.http.directories[0].validate();
        assert!(result.is_err(), "u64::MAX should fail validation");
    }

    #[test]
    fn test_cache_max_age_none_is_valid() {
        // No cache_max_age specified - should be valid (None)
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        "#;

        let config: Config = config_str.parse().unwrap();
        assert!(config.http.directories[0].validate().is_ok());
        assert_eq!(config.http.directories[0].cache_max_age, None);
    }

    #[test]
    fn test_cache_max_age_error_message_is_helpful() {
        let config_str = r#"
        [http]
        max_payload_size_bytes = "1KiB"

        [[http.directories]]
        directory = "static"
        route = "/static"
        cache_max_age = 100000000
        "#;

        let config: Config = config_str.parse().unwrap();
        let result = config.http.directories[0].validate();
        let err = result.unwrap_err();
        let error_message = err.to_string();

        // Error should mention the limit and suggest valid values
        assert!(
            error_message.contains("31536000") || error_message.contains("1 year"),
            "Error should mention the 1 year limit, got: {error_message}"
        );
        assert!(
            error_message.contains("86400") || error_message.contains("604800"),
            "Error should suggest valid values, got: {error_message}"
        );
    }
}
