//! Tests for the Prometheus `/metrics` endpoint.
//!
//! Metrics use a process-global recorder, so the rest of the test suite sets
//! `with_metrics = false`. This module is the single place that enables it and
//! asserts the endpoint actually serves Prometheus text.

use crate::fluent::tests::create_test_config;
use crate::{FluentRouter, HttpMiddleware, HttpMiddlewareConfig};
use axum::{body::Body, http::Request, routing::get};
use tower::ServiceExt;

#[tokio::test]
async fn metrics_endpoint_serves_prometheus_text() {
    let mut config = create_test_config();
    config.http.with_metrics = true;
    config.http.metrics_route = "/metrics".to_string();
    // Rate limiting needs ConnectInfo, unavailable under oneshot().
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .expect("router builds")
        .route("/hello", get(|| async { "hi" }))
        .setup_metrics()
        .into_inner();

    // Generate at least one request so a counter is recorded.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // The /metrics endpoint serves Prometheus exposition text.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&bytes);

    // Prometheus exposition format carries `# TYPE`/`# HELP` lines and our
    // package-prefixed metric names.
    assert!(
        body.contains("# TYPE") || body.contains("# HELP"),
        "expected Prometheus exposition text, got:\n{body}"
    );
    assert!(
        body.contains("axum_conf"),
        "expected package-prefixed metrics, got:\n{body}"
    );
}
