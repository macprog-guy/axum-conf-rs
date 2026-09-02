//! Integration tests for `[http.metrics_basic_auth]` — scoping the Prometheus
//! `/metrics` endpoint to its own Basic Auth credential, independent of
//! whatever `[http.oidc]` / `[http.basic_auth]` govern the rest of the app.
//!
//! These run as a real bound-TCP server (not `oneshot()`), matching this
//! repo's `tests/` integration-test convention (see `CLAUDE.md`'s test
//! organization table). That's also a functional requirement here, not just
//! style: `axum-prometheus`'s `with_prefix()` sets a process-global `OnceLock`
//! and panics if it's ever set a second time in the same process, so exactly
//! one test in this whole file (in fact, only one across the whole binary this
//! file compiles to) may call `setup_middleware()` with `with_metrics = true`.
//! `src/fluent/tests/middleware/metrics.rs` already spends that budget for the
//! *unauthenticated-by-default* case in its own, separate unit-test binary —
//! this file, being a distinct `tests/*.rs` binary/process, gets its own,
//! independent budget for the `metrics_basic_auth`-scoped case.
//!
//! ## Test Coverage
//!
//! - `metrics_endpoint_scoped_to_its_own_basic_auth`: `/metrics` requires its
//!   own Basic Auth credential (no creds → 401; correct creds → 200 with real
//!   Prometheus body; a Bearer token does not satisfy it → 401 — no credential
//!   leakage); an unrelated route is unaffected by the scoping.

#![cfg(all(feature = "basic-auth", feature = "metrics"))]

use axum_conf::{Config, FluentRouter, HttpBasicAuthConfig, HttpMiddleware, HttpMiddlewareConfig};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use std::time::Duration;
use tokio::net::TcpListener;

/// Basic Auth credential that secures only `/metrics`.
fn metrics_basic_auth_config() -> HttpBasicAuthConfig {
    HttpBasicAuthConfig {
        mode: axum_conf::BasicAuthMode::Basic,
        users: vec![axum_conf::BasicAuthUser {
            username: "prometheus".to_string(),
            password: "scrape-secret".into(),
            email: None,
            groups: Vec::new(),
            roles: Vec::new(),
            preferred_username: None,
        }],
        ..Default::default()
    }
}

fn create_test_config() -> Config {
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = true;
    config.http.metrics_basic_auth = Some(metrics_basic_auth_config());
    // Exclude rate limiting for tests (needs ConnectInfo behavior we don't set up here).
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

fn basic_auth_header(username: &str, password: &str) -> String {
    format!("Basic {}", BASE64.encode(format!("{username}:{password}")))
}

/// Starts a test server with the given config plus a plain `/hello` route.
async fn start_test_server(config: Config) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");

    let port = listener.local_addr().unwrap().port();

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .route("/hello", axum::routing::get(|| async { "hi" }))
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

#[tokio::test]
async fn metrics_endpoint_scoped_to_its_own_basic_auth() {
    let config = create_test_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create client");
    let metrics_url = format!("http://127.0.0.1:{port}/metrics");

    // Generate at least one request so a counter is recorded once we're authenticated.
    let hello_resp = client
        .get(format!("http://127.0.0.1:{port}/hello"))
        .send()
        .await
        .expect("Request failed");
    assert_eq!(hello_resp.status(), 200);

    // No credentials -> 401.
    let no_creds_resp = client
        .get(&metrics_url)
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        no_creds_resp.status(),
        401,
        "metrics should require its own Basic Auth credential"
    );

    // A Bearer token does not satisfy /metrics's Basic Auth requirement — no
    // credential leakage between auth methods. This needs no real JWT/OIDC
    // setup: `try_authenticate()` classifies anything not starting with
    // `"Basic "` as `NoCredentials`, and `secure_with_basic_auth` never offers
    // passthrough.
    let bearer_resp = client
        .get(&metrics_url)
        .header("Authorization", "Bearer some-arbitrary-token")
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        bearer_resp.status(),
        401,
        "a Bearer token must not satisfy metrics' Basic Auth requirement"
    );

    // Correct credentials -> 200 with real Prometheus exposition text.
    let ok_resp = client
        .get(&metrics_url)
        .header(
            "Authorization",
            basic_auth_header("prometheus", "scrape-secret"),
        )
        .send()
        .await
        .expect("Request failed");
    assert_eq!(ok_resp.status(), 200);

    let body = ok_resp.text().await.expect("Failed to read response body");
    assert!(
        body.contains("# TYPE") || body.contains("# HELP"),
        "expected Prometheus exposition text, got:\n{body}"
    );
    assert!(
        body.contains("axum_conf"),
        "expected package-prefixed metrics, got:\n{body}"
    );

    // An unrelated route is unaffected by the metrics-specific scoping.
    let hello_resp = client
        .get(format!("http://127.0.0.1:{port}/hello"))
        .send()
        .await
        .expect("Request failed");
    assert_eq!(hello_resp.status(), 200);

    server_handle.abort();
}
