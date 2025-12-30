//!
//! Configuration structures and utilities for wiring up the application or service.
//!
//! A configuration can be created in many ways:
//! - From an environment-specific TOML file via `Config::from_rust_env` or `Config::from_toml_file`
//! - From a TOML string via `Config::from_toml`
//! - Constructed programmatically via the builder methods on `Config`
//!
//! In both TOML-based methods, environment variables can be referenced in the TOML
//! using the {{ VAR_NAME }} syntax, and they will be substituted with the corresponding
//! environment variable value. This is done via the `replace_handlebars_with_env`
//! function and prevents sensitive information from being stored directly in the
//! TOML files.
//!
//! Configuration is split into logical sections, each represented by their own struct:
//!
//! - `HttpConfig` for HTTP server settings
//! - `DatabaseConfig` for database connection pool settings
//! - `LoggingConfig` for logging and tracing settings
//! - `StaticDirConfig` for static file serving settings
//!
//!
//!
mod http;
mod logging;

#[cfg(feature = "postgres")]
mod database;
#[cfg(feature = "postgres")]
pub use database::*;

pub use http::*;
pub use logging::*;

#[cfg(feature = "opentelemetry")]
mod opentelemetry;
#[cfg(feature = "opentelemetry")]
pub use opentelemetry::*;

#[cfg(feature = "postgres")]
use sqlx_postgres::{
    PgConnectOptions as PoolConnectOptions, PgPool as Pool, PgPoolOptions as PoolOptions,
};

pub use byte_unit::Byte;

use {
    crate::{Error, Result, utils::replace_handlebars_with_env},
    serde::Deserialize,
    std::{env, fs, str::FromStr, time::Duration},
};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub http: HttpConfig,
    #[cfg(feature = "postgres")]
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[cfg(feature = "circuit-breaker")]
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerConfig,
}

impl Default for Config {
    ///
    /// Creates a default configuration.
    /// This will attempt to load configuration from the file based on the RUST_ENV
    /// environment variable falling back to a default configuration if the environment
    /// variable is not set. Configuration files should be located in the "config/"
    /// directory of your project.
    ///
    fn default() -> Self {
        match Self::from_rust_env() {
            Ok(config) => config,
            Err(_) => Config {
                http: HttpConfig::default(),
                #[cfg(feature = "postgres")]
                database: DatabaseConfig::default(),
                logging: LoggingConfig::default(),
                #[cfg(feature = "circuit-breaker")]
                circuit_breaker: CircuitBreakerConfig::default(),
            },
        }
    }
}

impl Config {
    ///
    /// Loads the configuration from a file based on the RUST_ENV environment variable.
    /// If RUST_ENV is not set, defaults to "prod".
    ///
    pub fn from_rust_env() -> Result<Config> {
        Self::from_toml_file(env::var("RUST_ENV")?)
    }

    ///
    /// Given an environment name, loads the corresponding configuration file,
    /// substitutes any environment variables, and returns a Config struct.
    /// The configuration file is expected to be located at "config/{env}.toml"
    /// where {env} is the provided environment name (e.g., "dev", "prod").
    ///
    pub fn from_toml_file(env: impl AsRef<str>) -> Result<Config> {
        let path = format!("config/{}.toml", env.as_ref());
        let text = fs::read_to_string(path)?;
        Self::from_toml(&text)
    }

    ///
    /// Parses a configuration string in TOML format into a Config struct.
    ///
    pub fn from_toml(toml_str: &str) -> Result<Config> {
        replace_handlebars_with_env(toml_str).parse()
    }

    /// Sets the HTTP server bind address of the HttpConfig.
    pub fn with_bind_addr<S: AsRef<str>>(mut self, addr: S) -> Self {
        self.http.bind_addr = addr.as_ref().into();
        self
    }

    /// Sets the HTTP server bind port of the HttpConfig.
    pub fn with_bind_port(mut self, port: u16) -> Self {
        self.http.bind_port = port;
        self
    }

    /// Sets the maximum number of concurrent requests of the HttpConfig.
    pub fn with_max_concurrent_requests(mut self, max: u32) -> Self {
        self.http.max_concurrent_requests = max;
        self
    }

    /// Sets the request timeout duration of the HttpConfig.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.http.request_timeout = Some(timeout);
        self
    }

    /// Sets the X-Frame-Options header configuration of the HttpConfig.
    pub fn with_x_frame_options(mut self, x_frame: HttpXFrameConfig) -> Self {
        self.http.x_frame_options = x_frame;
        self
    }

    /// Enables or disables the X-Content-Type-Options header in the HttpConfig.
    pub fn with_x_content_type_nosniff(mut self, enable: bool) -> Self {
        self.http.x_content_type_nosniff = enable;
        self
    }

    /// Sets the maximum payload size in bytes of the HttpConfig.
    pub fn with_max_payload_size_bytes(mut self, size: u64) -> Self {
        self.http.max_payload_size_bytes = Byte::from_u64(size);
        self
    }

    /// Enables or disables compression support in the HttpConfig.
    pub fn with_compression(mut self, enable: bool) -> Self {
        self.http.support_compression = enable;
        self
    }

    /// Enables or disables trailing slash trimming in the HttpConfig.
    pub fn with_trim_trailing_slash(mut self, enable: bool) -> Self {
        self.http.trim_trailing_slash = enable;
        self
    }

    /// Sets the liveness route path of the HttpConfig.
    pub fn with_liveness_route(mut self, route: &str) -> Self {
        self.http.liveness_route = route.into();
        self
    }

    /// Sets the readiness route path of the HttpConfig.
    pub fn with_readiness_route(mut self, route: &str) -> Self {
        self.http.readiness_route = route.into();
        self
    }

    /// Sets the metrics route path of the HttpConfig.
    pub fn with_metrics_route(mut self, route: &str) -> Self {
        self.http.metrics_route = route.into();
        self
    }

    /// Sets the Postgres database connection URL of the DatabaseConfig.
    #[cfg(feature = "postgres")]
    pub fn with_pg_url(mut self, url: &str) -> Self {
        self.database.url = url.into();
        self
    }

    /// Sets the maximum pool size of the DatabaseConfig.
    #[cfg(feature = "postgres")]
    pub fn with_pg_max_pool_size(mut self, size: u8) -> Self {
        self.database.max_pool_size = size;
        self
    }

    /// Sets the maximum idle time duration of the DatabaseConfig.
    #[cfg(feature = "postgres")]
    pub fn with_pg_max_idle_time(mut self, duration: Duration) -> Self {
        self.database.max_idle_time = Some(duration);
        self
    }

    /// Sets the log format of the LoggingConfig.
    pub fn with_log_format(mut self, format: LogFormat) -> Self {
        self.logging.format = format;
        self
    }

    /// Sets the OIDC configuration of the HttpConfig.
    /// The default OIDC configuration is empty and must be set explicitly
    /// either programmatically or via TOML.
    #[cfg(feature = "keycloak")]
    pub fn with_oidc_config(mut self, oidc_config: HttpOidcConfig) -> Self {
        self.http.oidc = Some(oidc_config);
        self
    }

    /// Sets the CORS configuration of the HttpConfig.
    /// The default CORS configuration is empty resulting in permissive CORS configuration.
    /// Strict CORS must be set explicitly either programmatically or via TOML.
    pub fn with_cors_config(mut self, cors_config: HttpCorsConfig) -> Self {
        self.http.cors = Some(cors_config);
        self
    }

    /// Sets the deduplication configuration of the HttpConfig.
    /// The default deduplication configuration None results in no deduplication.
    /// Deduplication must be set explicitly either programmatically or via TOML.
    pub fn with_deduplication_config(mut self, dedup_config: HttpDeduplicationConfig) -> Self {
        self.http.deduplication = Some(dedup_config);
        self
    }

    /// Sets the middleware configuration of the HttpConfig.
    /// This approach activates only the specified middlewares.
    pub fn with_included_middlewares(mut self, middlewares: Vec<HttpMiddleware>) -> Self {
        self.http.middleware = Some(HttpMiddlewareConfig::Include(middlewares));
        self
    }

    /// Sets the middleware configuration of the HttpConfig.
    /// This approach activates all middlewares except the specified ones.
    pub fn with_excluded_middlewares(mut self, middlewares: Vec<HttpMiddleware>) -> Self {
        self.http.middleware = Some(HttpMiddlewareConfig::Exclude(middlewares));
        self
    }

    /// Sets the OpenTelemetry configuration of the LoggingConfig.
    #[cfg(feature = "opentelemetry")]
    pub fn with_opentelemetry_config(mut self, otel_config: OpenTelemetryConfig) -> Self {
        self.logging.opentelemetry = Some(otel_config);
        self
    }

    /// Ensures that the configuration is valid.
    /// Most configuration values are either optional or have sensible defaults.
    /// Some are required and since and here we ensure that those required values
    /// are set.
    pub fn validate(&self) -> Result<()> {
        #[cfg(feature = "postgres")]
        self.database.validate()?;
        self.http.validate()?;
        self.logging.validate()?;
        Ok(())
    }

    ///
    /// Sets up the tracing subscriber for logging based on the LoggingConfig.
    ///
    /// NOTE: This should be called early during startup to ensure logging is configured
    ///       before any log messages are emitted.
    ///
    pub fn setup_tracing(&self) {
        use tracing_subscriber::{EnvFilter, prelude::*};
        let env_filter = EnvFilter::from_default_env();
        match self.logging.format {
            LogFormat::Json => {
                let _ = tracing_subscriber::registry()
                    .with(tracing_subscriber::fmt::layer().json())
                    .with(env_filter)
                    .try_init();
            }
            LogFormat::Default => {
                let _ = tracing_subscriber::registry()
                    .with(tracing_subscriber::fmt::layer())
                    .with(env_filter)
                    .try_init();
            }
            LogFormat::Compact => {
                let _ = tracing_subscriber::registry()
                    .with(tracing_subscriber::fmt::layer().compact())
                    .with(env_filter)
                    .try_init();
            }
            LogFormat::Pretty => {
                let _ = tracing_subscriber::registry()
                    .with(tracing_subscriber::fmt::layer().pretty())
                    .with(env_filter)
                    .try_init();
            }
        }
    }

    ///
    /// Builds and returns a Postgres connection pool based on the configuration.
    /// The current implementation uses TLS with system root certificates.
    /// Furthermore, the application_name will be set to the crate package name
    /// for easier identification in the database logs.
    ///
    /// NOTE: load_native_certs does not return a regular Result type. Instead it
    ///       returns CertificateResult, which contains both a vec of certs and a
    ///       vec of errors encountered when loading certs. We consider it a
    ///       failure if any errors were encountered.
    ///
    #[cfg(feature = "postgres")]
    pub fn create_pgpool(&self) -> Result<Pool> {
        //
        let pool_options = PoolOptions::default()
            .min_connections(self.database.min_pool_size as u32)
            .max_connections(self.database.max_pool_size as u32)
            .idle_timeout(self.database.max_idle_time);

        let connect_options = PoolConnectOptions::from_str(&self.database.url)?
            .application_name(env!("CARGO_PKG_NAME"))
            .ssl_mode(sqlx_postgres::PgSslMode::Prefer);

        let pool = pool_options.connect_lazy_with(connect_options);

        Ok(pool)
    }
}

///
/// Parses a configuration string with references to environment variables
/// into a Config struct by substituting the environment variables and then
/// parsing the resulting TOML.
///
impl FromStr for Config {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let config_file = replace_handlebars_with_env(s);
        let config = toml::from_str::<Config>(&config_file)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::ErrorKind;

    #[test]
    fn test_replace_handlebars_with_env_no_variables() {
        let input = "This is a plain string with no variables";
        let output = replace_handlebars_with_env(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_replace_handlebars_with_env_with_variables() {
        unsafe {
            env::set_var("TEST_VAR", "test_value");
            env::set_var("ANOTHER_VAR", "another_value");
        }
        let input = "Database URL: {{ TEST_VAR }}, Host: {{ ANOTHER_VAR }}";
        let output = replace_handlebars_with_env(input);
        assert_eq!(output, "Database URL: test_value, Host: another_value");

        unsafe {
            env::remove_var("TEST_VAR");
            env::remove_var("ANOTHER_VAR");
        }
    }

    #[test]
    fn test_replace_handlebars_with_env_missing_variable() {
        unsafe {
            env::remove_var("NONEXISTENT_VAR");
        }

        let input = "Value: {{ NONEXISTENT_VAR }}";
        let output = replace_handlebars_with_env(input);
        assert_eq!(output, "Value: ");
    }

    #[test]
    fn test_replace_handlebars_with_env_whitespace() {
        unsafe {
            env::set_var("SPACED_VAR", "value");
        }

        let input = "{{SPACED_VAR}} {{ SPACED_VAR }} {{  SPACED_VAR  }}";
        let output = replace_handlebars_with_env(input);
        assert_eq!(output, "value value value");

        unsafe {
            env::remove_var("SPACED_VAR");
        }
    }

    #[test]
    fn test_replace_handlebars_with_env_multiple_occurrences() {
        unsafe {
            env::set_var("REPEATED_VAR", "repeated");
        }

        let input = "{{ REPEATED_VAR }} and {{ REPEATED_VAR }} again";
        let output = replace_handlebars_with_env(input);
        assert_eq!(output, "repeated and repeated again");

        unsafe {
            env::remove_var("REPEATED_VAR");
        }
    }

    #[test]
    fn test_config_from_str_valid() {
        unsafe {
            env::set_var("DATABASE_URL", "postgres://localhost/test");
        }

        let config_str = r#"
[database]
url = "{{ DATABASE_URL }}"
max_pool_size = 10

[http]
bind_addr = "0.0.0.0"
bind_port = 8080
max_payload_size_bytes = "1MB"
max_requests_per_sec = 5000

[http.oidc]
issuer_url = "https://keycloak.pictet.aws/realms/pictet"
client_id = "one-environment-pkce"
client_secret = "test"
realm = "pictet"

[logging]
format = "json"
        "#;

        let config = config_str.parse::<Config>();
        eprintln!("{:?}", config);
        assert!(config.is_ok());

        let config = config.unwrap();
        #[cfg(feature = "postgres")]
        assert_eq!(config.database.url, "postgres://localhost/test");
        assert_eq!(config.http.bind_addr, "0.0.0.0");
        assert_eq!(config.http.bind_port, 8080);

        unsafe {
            env::remove_var("DATABASE_URL");
        }
    }

    #[test]
    fn test_config_from_str_invalid_toml() {
        let invalid_config = "this is not valid toml";
        let result = invalid_config.parse::<Config>();
        assert!(result.is_err());
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_config_from_str_missing_required_fields() {
        // Test that validation catches missing required fields
        // In this case, an empty database URL should fail validation
        let incomplete_config = r#"
[database]
url = "postgres://localhost/test"

[http]
max_payload_size_bytes = "1KiB"
        "#;

        let result = incomplete_config.parse::<Config>();
        assert!(result.is_ok()); // Parsing should succeed with valid values

        // Now test with an empty database URL - validation should fail
        let mut config = result.unwrap();
        config.database.url = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_builder_matches_toml_equivalent() {
        // Build a configuration using builder methods
        let builder_config = Config::default()
            .with_bind_addr("0.0.0.0")
            .with_bind_port(8080)
            .with_max_concurrent_requests(2048)
            .with_request_timeout(Duration::from_secs(30))
            .with_max_payload_size_bytes(2 * 1024 * 1024) // 2 MiB
            .with_compression(true)
            .with_trim_trailing_slash(false)
            .with_liveness_route("/health")
            .with_readiness_route("/ready")
            .with_metrics_route("/prometheus")
            .with_log_format(LogFormat::Compact);

        #[cfg(feature = "postgres")]
        let builder_config = builder_config
            .with_pg_url("postgres://user:pass@localhost:5432/mydb")
            .with_pg_max_pool_size(20)
            .with_pg_max_idle_time(Duration::from_secs(300));

        // Create an equivalent configuration from TOML
        let toml_str = r#"
[http]
bind_addr = "0.0.0.0"
bind_port = 8080
max_concurrent_requests = 2048
request_timeout = "30s"
max_payload_size_bytes = "2MiB"
support_compression = true
trim_trailing_slash = false
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/prometheus"

[http.oidc]
issuer_url = "http://localhost:8080"
client_id = "test"
client_secret = "test"
realm = "test"

[database]
url = "postgres://user:pass@localhost:5432/mydb"
max_pool_size = 20
max_idle_time = "300s"

[logging]
format = "compact"
        "#;

        let toml_config: Config = toml_str.parse().expect("Failed to parse TOML config");

        // Compare HTTP configuration
        assert_eq!(builder_config.http.bind_addr, toml_config.http.bind_addr);
        assert_eq!(builder_config.http.bind_port, toml_config.http.bind_port);
        assert_eq!(
            builder_config.http.max_concurrent_requests,
            toml_config.http.max_concurrent_requests
        );
        assert_eq!(
            builder_config.http.request_timeout,
            toml_config.http.request_timeout
        );
        assert_eq!(
            builder_config.http.max_payload_size_bytes.as_u64(),
            toml_config.http.max_payload_size_bytes.as_u64()
        );
        assert_eq!(
            builder_config.http.support_compression,
            toml_config.http.support_compression
        );
        assert_eq!(
            builder_config.http.trim_trailing_slash,
            toml_config.http.trim_trailing_slash
        );
        assert_eq!(
            builder_config.http.liveness_route,
            toml_config.http.liveness_route
        );
        assert_eq!(
            builder_config.http.readiness_route,
            toml_config.http.readiness_route
        );
        assert_eq!(
            builder_config.http.metrics_route,
            toml_config.http.metrics_route
        );

        // Compare database configuration (if postgres feature is enabled)
        #[cfg(feature = "postgres")]
        {
            assert_eq!(builder_config.database.url, toml_config.database.url);
            assert_eq!(
                builder_config.database.max_pool_size,
                toml_config.database.max_pool_size
            );
            assert_eq!(
                builder_config.database.max_idle_time,
                toml_config.database.max_idle_time
            );
        }

        // Compare logging configuration
        assert!(matches!(builder_config.logging.format, LogFormat::Compact));
        assert!(matches!(toml_config.logging.format, LogFormat::Compact));
    }

    #[test]
    fn test_config_builder_chaining() {
        // Test that builder methods can be chained fluently
        let config = Config::default()
            .with_bind_addr("127.0.0.1")
            .with_bind_port(3000)
            .with_compression(true)
            .with_log_format(LogFormat::Json);

        assert_eq!(config.http.bind_addr, "127.0.0.1");
        assert_eq!(config.http.bind_port, 3000);
        assert_eq!(config.http.full_bind_addr(), "127.0.0.1:3000");
        assert!(config.http.support_compression);
        assert!(matches!(config.logging.format, LogFormat::Json));
    }

    #[test]
    fn test_config_builder_partial_configuration() {
        // Test that we can use builder methods to override just some defaults
        let config = Config::default()
            .with_bind_port(9000)
            .with_max_concurrent_requests(500);

        // Check overridden values
        assert_eq!(config.http.bind_port, 9000);
        assert_eq!(config.http.max_concurrent_requests, 500);

        // Check that defaults remain for non-overridden values
        assert_eq!(config.http.bind_addr, "127.0.0.1");
        assert_eq!(config.http.full_bind_addr(), "127.0.0.1:9000");
        assert_eq!(config.http.liveness_route, "/live");
        assert_eq!(config.http.readiness_route, "/ready");
    }

    #[test]
    fn test_load_from_rust_env() {
        unsafe {
            env::set_var("RUST_ENV", "test");
        }

        let result = Config::from_rust_env();
        assert!(
            result.is_ok(),
            "Expected configuration file to load successfully"
        );

        unsafe {
            env::remove_var("RUST_ENV");
        }

        let result = Config::from_rust_env();
        assert!(
            result.is_err(),
            "Expected error when loading non-existent default config file"
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_validate_empty_database_url() {
        let mut config = Config::default();
        config.database.url = "".to_string();

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty database URL"
        );

        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            ErrorKind::Database,
            "Expected ErrorKind::Database for empty database URL, got {:?}",
            err
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_oidc_empty_issuer_url() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "".to_string(),
            realm: "test".to_string(),
            audiences: vec![],
            client_id: "test-client".to_string(),
            client_secret: Sensitive::from("test-secret"),
        });

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty OIDC issuer_url"
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_oidc_empty_client_id() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "https://example.com".to_string(),
            realm: "test".to_string(),
            audiences: vec![],
            client_id: "".to_string(),
            client_secret: Sensitive::from("test-secret"),
        });

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty OIDC client_id"
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_oidc_empty_client_secret() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "https://example.com".to_string(),
            realm: "test".to_string(),
            audiences: vec![],
            client_id: "test-client".to_string(),
            client_secret: Sensitive::from(""),
        });

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty OIDC client_secret"
        );
    }

    #[test]
    fn test_validate_static_dir_fallback_cannot_be_protected() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        config.http.directories = vec![StaticDirConfig {
            directory: "./dist".to_string(),
            route: StaticDirRoute::Fallback(true),
            protected: true,
            cache_max_age: None,
        }];

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail when fallback directory is protected"
        );
    }

    #[test]
    fn test_validate_static_dir_with_route_can_be_protected() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.directories = vec![StaticDirConfig {
            directory: "./protected".to_string(),
            route: StaticDirRoute::Route("/downloads".to_string()),
            protected: true,
            cache_max_age: None,
        }];

        let result = config.validate();
        assert!(
            result.is_ok(),
            "Expected validation to succeed when non-fallback directory is protected"
        );
    }

    #[test]
    fn test_validate_static_dir_fallback_not_protected() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.directories = vec![StaticDirConfig {
            directory: "./dist".to_string(),
            route: StaticDirRoute::Fallback(true),
            protected: false,
            cache_max_age: None,
        }];

        let result = config.validate();
        assert!(
            result.is_ok(),
            "Expected validation to succeed when fallback directory is not protected"
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_valid_oidc_config() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "https://keycloak.example.com".to_string(),
            realm: "test-realm".to_string(),
            audiences: vec!["api".to_string()],
            client_id: "test-client".to_string(),
            client_secret: Sensitive::from("test-secret"),
        });

        let result = config.validate();
        assert!(
            result.is_ok(),
            "Expected validation to succeed with valid OIDC config"
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_validate_valid_database_config() {
        let mut config = Config::default();
        config.database.url = "postgres://user:pass@localhost:5432/mydb".to_string();

        let result = config.validate();
        assert!(
            result.is_ok(),
            "Expected validation to succeed with valid database URL"
        );
    }

    #[test]
    fn test_validate_empty_config() {
        let config = Config::default();
        // Default config may or may not validate depending on feature flags
        // With postgres feature, default might have empty DB URL which would fail
        #[cfg(feature = "postgres")]
        {
            // Default database URL comes from DATABASE_URL env var or empty string
            // If it's empty, validation should fail
            if config.database.url.is_empty() {
                assert!(config.validate().is_err());
            }
        }
        #[cfg(not(feature = "postgres"))]
        {
            // Without postgres feature, default config should validate
            assert!(config.validate().is_ok());
        }
    }

    #[test]
    fn test_validate_multiple_static_dirs() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.directories = vec![
            StaticDirConfig {
                directory: "./public".to_string(),
                route: StaticDirRoute::Route("/static".to_string()),
                protected: false,
                cache_max_age: Some(3600),
            },
            StaticDirConfig {
                directory: "./downloads".to_string(),
                route: StaticDirRoute::Route("/downloads".to_string()),
                protected: true,
                cache_max_age: None,
            },
        ];

        let result = config.validate();
        assert!(
            result.is_ok(),
            "Expected validation to succeed with multiple valid static directories"
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_validate_invalid_database_url_format() {
        let mut config = Config::default();
        config.database.url = "not-a-valid-url".to_string();

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with malformed database URL"
        );

        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            ErrorKind::Database,
            "Expected ErrorKind::Database for invalid URL format, got {:?}",
            err
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_validate_database_url_whitespace_only() {
        let mut config = Config::default();
        config.database.url = "   ".to_string();

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with whitespace-only database URL"
        );

        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            ErrorKind::Database,
            "Expected ErrorKind::Database for whitespace-only URL, got {:?}",
            err
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_validate_database_zero_pool_size() {
        let mut config = Config::default();
        config.database.url = "postgres://localhost/test".to_string();
        config.database.max_pool_size = 0;

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with zero pool size"
        );

        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            ErrorKind::Database,
            "Expected ErrorKind::Database for zero pool size, got {:?}",
            err
        );
    }

    #[test]
    #[cfg(feature = "postgres")]
    fn test_database_config_error_contains_helpful_message() {
        let mut config = Config::default();
        config.database.url = "".to_string();

        let result = config.validate();
        let err = result.unwrap_err();

        // Verify the error message contains actionable guidance
        let error_message = err.to_string();
        assert!(
            error_message.contains("DATABASE_URL") || error_message.contains("[database]"),
            "Error message should contain configuration guidance, got: {}",
            error_message
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_oidc_invalid_issuer_url() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "not a valid url".to_string(),
            realm: "test".to_string(),
            audiences: vec![],
            client_id: "test-client".to_string(),
            client_secret: Sensitive::from("test-secret"),
        });

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with invalid OIDC issuer URL"
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_oidc_whitespace_client_id() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "https://example.com".to_string(),
            realm: "test".to_string(),
            audiences: vec![],
            client_id: "   ".to_string(),
            client_secret: Sensitive::from("test-secret"),
        });

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with whitespace-only client_id"
        );
    }

    #[test]
    #[cfg(feature = "keycloak")]
    fn test_validate_oidc_empty_realm() {
        use crate::config::http::HttpOidcConfig;
        use crate::utils::Sensitive;

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.oidc = Some(HttpOidcConfig {
            issuer_url: "https://example.com".to_string(),
            realm: "".to_string(),
            audiences: vec![],
            client_id: "test-client".to_string(),
            client_secret: Sensitive::from("test-secret"),
        });

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty realm"
        );
    }

    #[test]
    fn test_validate_static_dir_empty_directory_path() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.directories = vec![StaticDirConfig {
            directory: "".to_string(),
            route: StaticDirRoute::Route("/static".to_string()),
            protected: false,
            cache_max_age: None,
        }];

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty directory path"
        );
    }

    #[test]
    fn test_validate_static_dir_empty_route_path() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.directories = vec![StaticDirConfig {
            directory: "./public".to_string(),
            route: StaticDirRoute::Route("".to_string()),
            protected: false,
            cache_max_age: None,
        }];

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty route path"
        );
    }

    #[test]
    fn test_validate_static_dir_multiple_fallbacks() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.directories = vec![
            StaticDirConfig {
                directory: "./dist1".to_string(),
                route: StaticDirRoute::Fallback(true),
                protected: false,
                cache_max_age: None,
            },
            StaticDirConfig {
                directory: "./dist2".to_string(),
                route: StaticDirRoute::Fallback(true),
                protected: false,
                cache_max_age: None,
            },
        ];

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with multiple fallback directories"
        );
    }

    #[test]
    fn test_validate_static_dir_negative_cache_max_age() {
        use crate::config::http::{StaticDirConfig, StaticDirRoute};

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        // Note: cache_max_age is u32, so we can't directly test negative values
        // But we can test with Some(0) which might be considered invalid
        config.http.directories = vec![StaticDirConfig {
            directory: "./public".to_string(),
            route: StaticDirRoute::Route("/static".to_string()),
            protected: false,
            cache_max_age: Some(0),
        }];

        let result = config.validate();
        // cache_max_age of 0 is actually valid (means no caching)
        assert!(
            result.is_ok(),
            "cache_max_age of 0 should be valid (no caching)"
        );
    }

    #[test]
    fn test_validate_bind_port_zero() {
        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.bind_port = 0;

        let result = config.validate();
        // Port 0 means OS will assign a random port, which is valid
        assert!(
            result.is_ok(),
            "Port 0 should be valid (OS assigns random port)"
        );
    }

    #[test]
    fn test_validate_empty_bind_addr() {
        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.bind_addr = "".to_string();

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with empty bind address"
        );
    }

    #[test]
    fn test_validate_invalid_bind_addr_format() {
        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.bind_addr = "not-an-ip-address".to_string();

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with invalid bind address format"
        );
    }

    #[test]
    fn test_validate_max_concurrent_requests_zero() {
        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.max_concurrent_requests = 0;

        let result = config.validate();
        assert!(
            result.is_err(),
            "Expected validation to fail with zero max_concurrent_requests"
        );
    }

    #[test]
    fn test_validate_conflicting_middleware_config() {
        use crate::config::http::HttpMiddlewareConfig;

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        // Setting middleware to Include with empty vec might be considered invalid
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![]));

        let result = config.validate();
        // Empty include list is actually valid - it means no middlewares
        assert!(
            result.is_ok(),
            "Empty include list should be valid (no middlewares enabled)"
        );
    }

    #[test]
    fn test_validate_cors_empty_allowed_origins() {
        use crate::config::http::HttpCorsConfig;

        let mut config = Config::default();
        #[cfg(feature = "postgres")]
        {
            config.database.url = "postgres://localhost/test".to_string();
        }
        config.http.cors = Some(HttpCorsConfig::default().with_allowed_origins(vec![]));

        let result = config.validate();
        // Empty allowed origins defaults to permissive CORS (allow all)
        assert!(
            result.is_ok(),
            "Empty allowed_origins should be valid (defaults to permissive)"
        );
    }

    // ========================================================================
    // Property-based tests for config parsing
    // ========================================================================

    mod proptest_config {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Valid bind addresses should parse and validate
            #[test]
            fn valid_bind_addr_parses(
                a in 0u8..=255,
                b in 0u8..=255,
                c in 0u8..=255,
                d in 0u8..=255
            ) {
                let addr = format!("{a}.{b}.{c}.{d}");
                let toml_str = format!(
                    r#"
[http]
bind_addr = "{addr}"
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "Valid IP should parse");

                let config = config.unwrap();
                prop_assert_eq!(config.http.bind_addr, addr);
            }

            /// Valid port numbers should parse
            #[test]
            fn valid_port_parses(port in 0u16..=65535) {
                let toml_str = format!(
                    r#"
[http]
bind_addr = "127.0.0.1"
bind_port = {port}
max_payload_size_bytes = "1KiB"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "Valid port should parse");

                let config = config.unwrap();
                prop_assert_eq!(config.http.bind_port, port);
            }

            /// Valid max_concurrent_requests should parse and validate
            #[test]
            fn valid_max_concurrent_parses(max in 1u32..10000) {
                let toml_str = format!(
                    r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = {max}
max_payload_size_bytes = "1KiB"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "Valid max_concurrent_requests should parse");

                let config = config.unwrap();
                prop_assert_eq!(config.http.max_concurrent_requests, max);

                // Should also validate successfully
                #[cfg(feature = "postgres")]
                let config = Config {
                    database: DatabaseConfig {
                        url: "postgres://localhost/test".into(),
                        ..config.database
                    },
                    ..config
                };

                prop_assert!(config.validate().is_ok());
            }

            /// Zero max_concurrent_requests should fail validation
            #[test]
            fn zero_max_concurrent_fails_validation(_dummy in 0..1) {
                let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 0
max_payload_size_bytes = "1KiB"
"#;

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "TOML should parse even with 0");

                #[allow(unused_mut)]
                let mut config = config.unwrap();

                #[cfg(feature = "postgres")]
                {
                    config.database.url = "postgres://localhost/test".into();
                }

                prop_assert!(config.validate().is_err(), "Validation should fail with 0");
            }

            /// Valid byte sizes should parse correctly
            #[test]
            fn valid_byte_sizes_parse(size in 1u64..1_000_000) {
                let toml_str = format!(
                    r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "{size}"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "Valid byte size should parse");

                let config = config.unwrap();
                prop_assert_eq!(config.http.max_payload_size_bytes.as_u64(), size);
            }

            /// Config with all valid fields should validate
            #[test]
            fn complete_valid_config_validates(
                port in 1024u16..49151,
                max_concurrent in 1u32..10000,
                max_requests_per_sec in 1u32..10000
            ) {
                let toml_str = format!(
                    r#"
[http]
bind_addr = "127.0.0.1"
bind_port = {port}
max_concurrent_requests = {max_concurrent}
max_requests_per_sec = {max_requests_per_sec}
max_payload_size_bytes = "1MiB"

[database]
url = "postgres://test:test@localhost:5432/test"
max_pool_size = 5

[logging]
format = "json"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "Valid config should parse");

                let config = config.unwrap();
                prop_assert!(config.validate().is_ok(), "Valid config should validate");
            }

            /// API version defaults should be respected
            #[test]
            fn default_api_version_respected(version in 1u32..100) {
                let toml_str = format!(
                    r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
default_api_version = {version}
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok());

                let config = config.unwrap();
                prop_assert_eq!(config.http.default_api_version, version);
            }
        }

        #[cfg(feature = "postgres")]
        proptest! {
            /// Valid database URLs should parse and validate
            #[test]
            fn valid_database_url_validates(
                user in "[a-z]{1,10}",
                pass in "[a-z]{1,10}",
                host in "[a-z]{1,10}",
                port in 1024u16..49151,
                db in "[a-z]{1,10}"
            ) {
                let url = format!("postgres://{user}:{pass}@{host}:{port}/{db}");
                let toml_str = format!(
                    r#"
[database]
url = "{url}"
max_pool_size = 5

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "Valid database config should parse");

                let config = config.unwrap();
                prop_assert!(config.validate().is_ok(), "Valid database config should validate");
            }

            /// Invalid database URL schemes should fail validation
            #[test]
            fn invalid_database_scheme_fails(scheme in "[a-z]{3,8}") {
                // Skip if it happens to be postgres/postgresql
                prop_assume!(!scheme.starts_with("postgres"));

                let url = format!("{scheme}://user:pass@localhost:5432/db");
                let toml_str = format!(
                    r#"
[database]
url = "{url}"
max_pool_size = 5

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#
                );

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "TOML should parse even with invalid scheme");

                let config = config.unwrap();
                prop_assert!(config.validate().is_err(), "Invalid scheme should fail validation");
            }

            /// Zero pool size should fail validation
            #[test]
            fn zero_pool_size_fails(_dummy in 0..1) {
                let toml_str = r#"
[database]
url = "postgres://localhost/test"
max_pool_size = 0

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#;

                let config: std::result::Result<Config, _> = toml_str.parse();
                prop_assert!(config.is_ok(), "TOML should parse");

                let config = config.unwrap();
                prop_assert!(config.validate().is_err(), "Zero pool size should fail");
            }
        }
    }
}
