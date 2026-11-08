//! `Config::install_metrics()` installs the global Prometheus recorder once and
//! `setup_metrics()` reuses it.
//!
//! A single test on purpose: the recorder is process-global, and each integration
//! test file is its own process.
#![cfg(feature = "metrics")]

use axum::{body::Body, http::Request};
use axum_conf::{Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig};
use tower::ServiceExt;

fn metrics_config() -> Config {
    let mut config = Config::new().with_bind_addr("127.0.0.1");
    config.http.with_metrics = true;
    config.http.metrics_route = "/metrics".to_string();
    // Rate limiting needs ConnectInfo, which is unavailable under oneshot().
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

async fn scrape(app: axum::Router) -> String {
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
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn build_app(config: Config) -> axum::Router {
    FluentRouter::without_state(config)
        .expect("router builds")
        .setup_middleware()
        .await
        .expect("middleware sets up")
        .into_inner()
}

#[tokio::test]
async fn setup_metrics_reuses_installed_recorder() {
    // Disabled: nothing is installed.
    let mut disabled = metrics_config();
    disabled.http.with_metrics = false;
    assert!(disabled.install_metrics().is_none());

    // Install up front and record a metric before any router exists.
    let config = metrics_config();
    let (_layer, handle) = config.install_metrics().expect("metrics enabled");
    metrics::counter!("install_test_total").increment(7);
    assert!(handle.render().contains("install_test_total 7"));

    // A second install reuses the same recorder instead of failing.
    let (_layer2, handle2) = config.install_metrics().expect("metrics enabled");
    assert!(handle2.render().contains("install_test_total 7"));

    // Two routers built afterwards (with different recorder settings, which only
    // logs a warning) both serve the one shared registry; neither panics.
    let app1 = build_app(metrics_config()).await;
    let app2 = build_app(metrics_config().with_metrics_global_label("service", "other")).await;
    assert!(scrape(app1).await.contains("install_test_total 7"));
    assert!(scrape(app2).await.contains("install_test_total 7"));
}
