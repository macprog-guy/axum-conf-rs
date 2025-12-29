//! Test helpers and utilities for FluentRouter tests
//!
//! This module provides shared test infrastructure for all FluentRouter **unit tests**.
//! These tests use `oneshot()` for fast, in-process testing without network I/O.
//!
//! ## Test Organization
//!
//! - **Unit tests** (`src/fluent/tests/`): Fast, isolated tests using `oneshot()`
//! - **Integration tests** (`tests/`): Real server + network tests (rate limiting, OIDC)
//!
//! See `tests/README.md` for details on why some tests require external infrastructure.
//!
//! ## Available Helpers
//!
//! - Configuration builders: `create_base_config()`, `create_test_config()`, `create_config_with_toml()`
//! - Router builders: `create_test_router()`, `TestRouterBuilder`
//! - Request helpers: `get_request()`, `post_request()`, `options_request()`
//! - Response helpers: `get_body_string()`

use crate::{Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig};
use axum::{Router, body::Body, http::Request, response::Response, routing::get};

// Re-export test modules
#[cfg(test)]
pub(crate) mod basic;
#[cfg(test)]
pub(crate) mod integration;
#[cfg(test)]
pub(crate) mod middleware;

// ============================================================================
// Configuration Helpers
// ============================================================================

/// Base TOML configuration template for tests.
/// Use `create_config_with_toml()` to inject additional sections.
const BASE_CONFIG_TOML: &str = r#"
[database]
url = "postgres://test:test@localhost:5432/test"
max_pool_size = 5

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
support_compression = false
trim_trailing_slash = true
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[logging]
format = "json"
"#;

/// Creates a base test configuration by parsing TOML.
/// This config has sensible defaults for most tests.
pub(crate) fn create_base_config() -> Config {
    BASE_CONFIG_TOML
        .parse()
        .expect("Failed to parse test config TOML")
}

/// Creates a simpler test configuration programmatically using defaults.
/// Useful when you only need minimal configuration.
#[allow(dead_code)]
pub(crate) fn create_test_config() -> Config {
    #[cfg(not(feature = "postgres"))]
    let config = Config::default();

    #[cfg(feature = "postgres")]
    let config = Config::default().with_pg_url("postgres://test:test@localhost:5432/test");

    config
}

/// Creates a test configuration with additional TOML sections injected.
///
/// # Example
/// ```ignore
/// let config = create_config_with_toml(r#"
/// [[http.directories]]
/// directory = "tests/test_static_files"
/// route = "/static"
/// "#);
/// ```
pub(crate) fn create_config_with_toml(additional_toml: &str) -> Config {
    let toml_str = format!(
        r#"
[database]
url = "postgres://test:test@localhost:5432/test"
max_pool_size = 5

[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
support_compression = false
trim_trailing_slash = true
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

{additional_toml}

[logging]
format = "json"
        "#
    );

    toml_str.parse().expect("Failed to parse test config TOML")
}

// ============================================================================
// Router Helpers
// ============================================================================

/// Prepares a config for test usage by disabling metrics, OIDC, and rate limiting.
/// These features are incompatible with `oneshot()` testing.
pub(crate) fn prepare_config_for_test(mut config: Config) -> Config {
    config.http.with_metrics = false;

    #[cfg(feature = "keycloak")]
    {
        config.http.oidc = None;
    }

    // Disable rate limiting (oneshot() doesn't provide ConnectInfo<SocketAddr>)
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    config
}

/// Creates a test router with common test configuration.
/// Automatically disables metrics, OIDC, and rate limiting for `oneshot()` compatibility.
pub(crate) async fn create_test_router(config: Option<Config>) -> Router {
    let config = prepare_config_for_test(config.unwrap_or_else(create_base_config));

    FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(Router::new().route("/noop", get(|| async { "OK\n" }).post(|| async { "OK\n" })))
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner()
}

/// Creates a test router with static file support.
/// Use this when testing static file serving.
pub(crate) async fn create_test_router_with_static_files(config: Config) -> Router {
    let config = prepare_config_for_test(config);

    FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .setup_public_files()
        .expect("Failed to setup public files")
        .merge(Router::new().route("/test", get(|| async { "test response" })))
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner()
}

// ============================================================================
// Request Helpers
// ============================================================================

/// Creates a GET request to the specified URI.
#[allow(dead_code)]
pub(crate) fn get_request(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Creates a POST request to the specified URI with an empty body.
#[allow(dead_code)]
pub(crate) fn post_request(uri: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

/// Creates an OPTIONS preflight request for CORS testing.
#[allow(dead_code)]
pub(crate) fn options_request(uri: &str, origin: &str, method: &str) -> Request<Body> {
    Request::builder()
        .method("OPTIONS")
        .uri(uri)
        .header("Origin", origin)
        .header("Access-Control-Request-Method", method)
        .body(Body::empty())
        .unwrap()
}

/// Creates a request with a custom request ID header.
#[allow(dead_code)]
pub(crate) fn request_with_id(method: &str, uri: &str, request_id: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("x-request-id", request_id)
        .body(Body::empty())
        .unwrap()
}

// ============================================================================
// Response Helpers
// ============================================================================

/// Extracts the body from a response as a String.
#[allow(dead_code)]
pub(crate) async fn get_body_string(response: Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&body).to_string()
}

// ============================================================================
// Test Handlers
// ============================================================================

/// Handler for nested route tests.
pub(crate) async fn nested_handler() -> &'static str {
    "nested response"
}
