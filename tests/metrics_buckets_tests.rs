//! Integration test for configurable Prometheus histogram buckets.
//!
//! This is a **single** test on purpose: the global Prometheus recorder installs
//! once per process, and `with_prefix` may set its prefix only once. As a
//! separate integration-test binary it has its own process, so it does not
//! collide with the in-crate `metrics_endpoint_serves_prometheus_text` test.
#![cfg(feature = "metrics")]

use axum::{body::Body, http::Request, routing::get};
use axum_conf::{Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig};
use tower::ServiceExt;

#[tokio::test]
async fn custom_buckets_render_as_histogram_with_global_label() {
    let mut config = Config::new()
        .with_bind_addr("127.0.0.1")
        .with_metric_buckets(
            "test_widget_seconds",
            [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0],
        )
        .with_metrics_global_label("service", "metrics-buckets-test");
    config.http.with_metrics = true;
    config.http.metrics_route = "/metrics".to_string();
    // Rate limiting needs ConnectInfo, which is unavailable under oneshot().
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .expect("router builds")
        .route(
            "/hit",
            get(|| async {
                metrics::histogram!("test_widget_seconds").record(0.03);
                "ok"
            }),
        )
        .setup_middleware()
        .await
        .expect("middleware sets up")
        .into_inner();

    // Drive one request so the custom histogram and the built-in series record.
    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/hit").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // Drain the response body so the body-size callback fires and records the
    // built-in `axum_conf_http_response_body_size` histogram.
    let _ = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();

    // Scrape /metrics.
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

    // The custom metric renders as a bucketed histogram, not a summary.
    assert!(
        body.contains("test_widget_seconds_bucket{"),
        "expected histogram buckets for test_widget_seconds, got:\n{body}"
    );
    assert!(
        body.contains("le="),
        "expected `le=` bucket upper bounds, got:\n{body}"
    );
    assert!(
        body.contains("service=\"metrics-buckets-test\""),
        "expected the global constant label on series, got:\n{body}"
    );
    assert!(
        !body.contains("test_widget_seconds{quantile="),
        "test_widget_seconds must be a histogram, not a summary, got:\n{body}"
    );

    // The built-in response-body-size histogram is still present. By design only
    // the duration and the named metric get bucket matchers, so it is NOT
    // seconds-bucketed.
    assert!(
        body.contains("axum_conf_http_response_body_size"),
        "expected the built-in response body size metric, got:\n{body}"
    );
}
