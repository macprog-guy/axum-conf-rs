//! Integration tests for HTTP Basic Auth and API Key authentication middleware.
//!
//! These tests start a server on a random port and make real HTTP requests
//! to verify that basic authentication works correctly.
//!
//! ## Test Coverage
//!
//! - `test_basic_auth_valid_credentials`: Verifies that valid Basic Auth credentials succeed
//! - `test_basic_auth_invalid_credentials`: Verifies that invalid credentials return 401
//! - `test_api_key_valid`: Verifies that valid API key authentication succeeds
//! - `test_api_key_invalid`: Verifies that invalid API keys return 401
//! - `test_no_auth_returns_401`: Verifies that missing credentials return 401
//! - `test_health_endpoints_bypass_auth`: Ensures health endpoints don't require auth
//! - `test_either_mode_accepts_both`: Verifies both methods work in "either" mode
//! - `test_basic_mode_rejects_api_key`: Verifies API keys are rejected in "basic" mode
//! - `test_api_key_mode_rejects_basic`: Verifies Basic Auth is rejected in "api_key" mode

#![cfg(feature = "basic-auth")]

use axum::{Extension, Router, routing::get};
use axum_conf::{
    AuthenticatedIdentity, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use std::time::Duration;
use tokio::net::TcpListener;

/// Creates a test config with basic auth enabled
fn create_basic_auth_config() -> Config {
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.basic_auth]
mode = "either"
api_key_header = "X-API-Key"

[[http.basic_auth.users]]
username = "testuser"
password = "testpass"

[[http.basic_auth.api_keys]]
key = "test-api-key-12345"
name = "test-key"

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;
    // Exclude rate limiting for tests
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

/// Creates a config with basic auth mode set to "basic" only
fn create_basic_only_config() -> Config {
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.basic_auth]
mode = "basic"

[[http.basic_auth.users]]
username = "testuser"
password = "testpass"

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

/// Creates a config with basic auth mode set to "api_key" only
fn create_api_key_only_config() -> Config {
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.basic_auth]
mode = "api_key"
api_key_header = "X-API-Key"

[[http.basic_auth.api_keys]]
key = "test-api-key-12345"
name = "test-key"

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

/// Handler that returns the authenticated identity name
async fn whoami_handler(Extension(identity): Extension<AuthenticatedIdentity>) -> String {
    format!("Hello, {}!", identity.name)
}

/// Start a test server with the given config
async fn start_test_server(config: Config) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");

    let port = listener.local_addr().unwrap().port();

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(
            Router::new()
                .route("/test", get(|| async { "OK" }))
                .route("/whoami", get(whoami_handler)),
        )
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();

    let handle = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .expect("Server failed to run");
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    (port, handle)
}

/// Create Basic Auth header value
fn basic_auth_header(username: &str, password: &str) -> String {
    let credentials = BASE64.encode(format!("{}:{}", username, password));
    format!("Basic {}", credentials)
}

#[tokio::test]
async fn test_basic_auth_valid_credentials() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("testuser", "testpass"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200, "Valid credentials should succeed");
    assert_eq!(response.text().await.unwrap(), "OK");

    server_handle.abort();
}

#[tokio::test]
async fn test_basic_auth_invalid_credentials() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Wrong password
    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("testuser", "wrongpass"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(
        response.status(),
        401,
        "Invalid credentials should return 401"
    );
    assert!(
        response.headers().get("www-authenticate").is_some(),
        "Should include WWW-Authenticate header"
    );

    // Wrong username
    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("wronguser", "testpass"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 401, "Invalid username should return 401");

    server_handle.abort();
}

#[tokio::test]
async fn test_api_key_valid() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    let response = client
        .get(&url)
        .header("X-API-Key", "test-api-key-12345")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200, "Valid API key should succeed");
    assert_eq!(response.text().await.unwrap(), "OK");

    server_handle.abort();
}

#[tokio::test]
async fn test_api_key_invalid() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    let response = client
        .get(&url)
        .header("X-API-Key", "wrong-api-key")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 401, "Invalid API key should return 401");

    server_handle.abort();
}

#[tokio::test]
async fn test_no_auth_returns_401() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    let response = client.get(&url).send().await.expect("Request failed");

    assert_eq!(
        response.status(),
        401,
        "Missing credentials should return 401"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_health_endpoints_bypass_auth() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create client");

    // Liveness should be accessible without auth
    let health_url = format!("http://127.0.0.1:{}/health", port);
    let response = client
        .get(&health_url)
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "Health endpoint should not require auth"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_either_mode_accepts_both() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Basic Auth should work
    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("testuser", "testpass"))
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "Basic Auth should work in either mode"
    );

    // API Key should also work
    let response = client
        .get(&url)
        .header("X-API-Key", "test-api-key-12345")
        .send()
        .await
        .expect("Request failed");
    assert_eq!(response.status(), 200, "API Key should work in either mode");

    server_handle.abort();
}

#[tokio::test]
async fn test_basic_mode_rejects_api_key() {
    let config = create_basic_only_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // API Key should be rejected in basic mode
    let response = client
        .get(&url)
        .header("X-API-Key", "test-api-key-12345")
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        401,
        "API Key should be rejected in basic mode"
    );

    // Basic Auth should still work
    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("testuser", "testpass"))
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "Basic Auth should work in basic mode"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_api_key_mode_rejects_basic() {
    let config = create_api_key_only_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Basic Auth should be rejected in api_key mode
    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("testuser", "testpass"))
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        401,
        "Basic Auth should be rejected in api_key mode"
    );

    // API Key should still work
    let response = client
        .get(&url)
        .header("X-API-Key", "test-api-key-12345")
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "API Key should work in api_key mode"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_identity_extraction_basic_auth() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client
        .get(&url)
        .header("Authorization", basic_auth_header("testuser", "testpass"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "Hello, testuser!", "Should extract correct username");

    server_handle.abort();
}

#[tokio::test]
async fn test_identity_extraction_api_key() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client
        .get(&url)
        .header("X-API-Key", "test-api-key-12345")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(
        body, "Hello, test-key!",
        "Should extract correct API key name"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_malformed_basic_auth_header() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Invalid base64
    let response = client
        .get(&url)
        .header("Authorization", "Basic not-valid-base64!!!")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(
        response.status(),
        400,
        "Malformed Basic Auth should return 400"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_basic_auth_missing_colon() {
    let config = create_basic_auth_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Base64 encoded "usernameonly" (no colon separator)
    let credentials = BASE64.encode("usernameonly");
    let response = client
        .get(&url)
        .header("Authorization", format!("Basic {}", credentials))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(
        response.status(),
        400,
        "Basic Auth without colon should return 400"
    );

    server_handle.abort();
}
