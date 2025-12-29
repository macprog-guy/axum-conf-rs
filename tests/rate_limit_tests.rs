//! Integration tests for rate limiting middleware
//!
//! These tests actually start a server on a random port and make real HTTP requests
//! to verify that rate limiting works correctly with ConnectInfo<SocketAddr>.
//!
//! ## Test Coverage
//!
//! - `test_rate_limiting_blocks_excessive_requests`: Verifies that requests exceeding
//!   the configured rate limit receive 429 Too Many Requests status
//! - `test_rate_limiting_resets_after_time`: Confirms that rate limits reset after
//!   the time window expires
//! - `test_rate_limiting_per_ip`: Validates that rate limiting is applied per IP address
//! - `test_rate_limiting_does_not_affect_health_endpoints`: Ensures health/liveness
//!   endpoints bypass rate limiting
//! - `test_rate_limiting_preserves_request_processing`: Verifies that successful requests
//!   are processed correctly within rate limits
//! - `test_rate_limiting_with_concurrent_requests`: Tests rate limiting behavior with
//!   concurrent requests and burst capacity
//! - `test_rate_limiting_disabled_when_zero`: Confirms that setting max_requests_per_sec
//!   to 0 disables rate limiting entirely

use axum::{Router, routing::get};
use axum_conf::{Config, FluentRouter};
use reqwest::Client;
use std::time::Duration;
use tokio::net::TcpListener;

/// Helper to create a test configuration with rate limiting enabled
fn create_rate_limit_config(max_requests_per_sec: u32) -> Config {
    let toml_str = format!(
        r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
support_compression = false
trim_trailing_slash = true
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"
max_requests_per_sec = {}

[logging]
format = "json"
        "#,
        max_requests_per_sec
    );

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    // Disable metrics to avoid Prometheus registry conflicts in tests
    config.http.with_metrics = false;
    config
}

/// Start a server on a random port and return the port number and a shutdown handle
async fn start_test_server(config: Config) -> (u16, tokio::task::JoinHandle<()>) {
    // Bind to port 0 to get a random available port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");

    let port = listener.local_addr().unwrap().port();

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(Router::new().route("/test", get(|| async { "OK" })).route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                "Slow response"
            }),
        ))
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

    // Give the server a moment to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    (port, handle)
}

#[tokio::test]
async fn test_rate_limiting_blocks_excessive_requests() {
    // Configure rate limit of 5 requests per second
    let config = create_rate_limit_config(5);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Make 5 requests rapidly - these should all succeed
    for i in 0..5 {
        let response = client.get(&url).send().await.expect("Request failed");
        assert_eq!(
            response.status(),
            200,
            "Request {} should succeed within rate limit",
            i + 1
        );
    }

    // The 6th request should be rate limited (429 Too Many Requests)
    let response = client.get(&url).send().await.expect("Request failed");
    assert_eq!(
        response.status(),
        429,
        "Request should be rate limited with 429 status"
    );

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limiting_resets_after_time() {
    // Configure rate limit of 2 requests per second
    let config = create_rate_limit_config(2);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Make 2 requests - should succeed
    for i in 0..2 {
        let response = client.get(&url).send().await.expect("Request failed");
        assert_eq!(response.status(), 200, "Request {} should succeed", i + 1);
    }

    // 3rd request should be rate limited
    let response = client.get(&url).send().await.expect("Request failed");
    assert_eq!(response.status(), 429, "Should be rate limited");

    // Wait for the rate limit window to reset (1 second + buffer)
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // New requests should succeed again
    let response = client.get(&url).send().await.expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "Request should succeed after rate limit reset"
    );

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limiting_per_ip() {
    // Configure rate limit of 3 requests per second
    let config = create_rate_limit_config(3);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // All requests are from localhost (127.0.0.1), so they should share the same rate limit

    // Make 3 requests - should all succeed
    for i in 0..3 {
        let response = client.get(&url).send().await.expect("Request failed");
        assert_eq!(response.status(), 200, "Request {} should succeed", i + 1);
    }

    // 4th request should be rate limited
    let response = client.get(&url).send().await.expect("Request failed");
    assert_eq!(
        response.status(),
        429,
        "Request from same IP should be rate limited"
    );

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limiting_does_not_affect_health_endpoints() {
    // Configure very restrictive rate limit of 1 request per second
    let config = create_rate_limit_config(1);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::builder()
        .timeout(Duration::from_secs(5)) // Set a timeout to avoid hanging
        .build()
        .expect("Failed to create client");
    let health_url = format!("http://127.0.0.1:{}/health", port);

    // Make multiple health check requests - they should all succeed
    // regardless of rate limiting
    for i in 0..5 {
        let health_response = client
            .get(&health_url)
            .send()
            .await
            .expect("Request failed");
        assert_eq!(
            health_response.status(),
            200,
            "Health check {} should not be rate limited",
            i + 1
        );
    }

    // Note: We skip readiness checks in this test because when postgres feature
    // is enabled, readiness checks might timeout trying to connect to a database
    // that isn't available in tests

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limiting_preserves_request_processing() {
    // Configure rate limit of 10 requests per second
    let config = create_rate_limit_config(10);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Make requests within the limit and verify responses
    for i in 0..10 {
        let response = client.get(&url).send().await.expect("Request failed");
        assert_eq!(response.status(), 200, "Request {} should succeed", i + 1);

        let body = response.text().await.expect("Failed to read body");
        assert_eq!(body, "OK", "Response body should be correct");
    }

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limiting_with_concurrent_requests() {
    // Configure rate limit of 20 requests per second
    let config = create_rate_limit_config(20);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::builder().build().expect("Failed to create client");
    let url = format!("http://127.0.0.1:{}/test", port);

    // Launch 20 concurrent requests
    let mut handles = vec![];
    for _ in 0..20 {
        let client_clone = client.clone();
        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            client_clone
                .get(&url_clone)
                .send()
                .await
                .expect("Request failed")
                .status()
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut success_count = 0;
    for handle in handles {
        let status = handle.await.expect("Task failed");
        if status == 200 {
            success_count += 1;
        }
    }

    // All 20 should succeed (within burst capacity)
    assert_eq!(
        success_count, 20,
        "All concurrent requests within limit should succeed"
    );

    // Now the rate limiter should be exhausted, next request should fail
    let response = client.get(&url).send().await.expect("Request failed");
    assert_eq!(
        response.status(),
        429,
        "Request after burst should be rate limited"
    );

    // Cleanup
    server_handle.abort();
}

#[tokio::test]
async fn test_rate_limiting_disabled_when_zero() {
    // Create config with rate limiting disabled (0 requests per second)
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
support_compression = false
trim_trailing_slash = true
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"
max_requests_per_sec = 0

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;

    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/test", port);

    // Make many requests - all should succeed since rate limiting is disabled
    for i in 0..100 {
        let response = client.get(&url).send().await.expect("Request failed");
        assert_eq!(
            response.status(),
            200,
            "Request {} should succeed with rate limiting disabled",
            i + 1
        );
    }

    // Cleanup
    server_handle.abort();
}
