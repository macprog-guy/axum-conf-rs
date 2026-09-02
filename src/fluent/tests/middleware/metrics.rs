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

/// `secure_with_basic_auth` is a general-purpose primitive, not a metrics-only
/// special case: it works for an arbitrary route, and doesn't affect routes
/// registered outside its closure. Unlike the test above, this doesn't touch
/// `setup_metrics()`/the Prometheus recorder at all, so it's safe to run
/// alongside `metrics_endpoint_serves_prometheus_text` in this same binary.
///
/// The `[http.metrics_basic_auth]`-wired behavior itself (scoping the real
/// `/metrics` endpoint) is covered by `tests/metrics_basic_auth_tests.rs`
/// instead of here: `axum-prometheus`'s `with_prefix()` sets a process-global
/// `OnceLock` and panics if called a second time in the same process, so only
/// ONE test in this whole unit-test binary may ever call `setup_metrics()`
/// with `with_metrics = true` — that slot is already spent by the test above.
/// A separate `tests/*.rs` file compiles to its own binary/process, so it gets
/// its own fresh `OnceLock` and can safely exercise the real endpoint.
#[cfg(feature = "basic-auth")]
#[tokio::test]
async fn secure_with_basic_auth_works_for_arbitrary_routes() {
    use crate::HttpBasicAuthConfig;
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

    fn basic_auth_header(user: &str, pass: &str) -> String {
        format!("Basic {}", BASE64.encode(format!("{user}:{pass}")))
    }

    let basic_auth_cfg = HttpBasicAuthConfig {
        mode: crate::BasicAuthMode::Basic,
        users: vec![crate::BasicAuthUser {
            username: "admin".to_string(),
            password: "admin-secret".into(),
            email: None,
            groups: Vec::new(),
            roles: Vec::new(),
            preferred_username: None,
        }],
        ..Default::default()
    };

    let app = FluentRouter::without_state(create_test_config())
        .expect("router builds")
        .secure_with_basic_auth(&basic_auth_cfg, |r| {
            r.route("/admin/debug", get(|| async { "secret" }))
        })
        .route("/public", get(|| async { "hello" }))
        .into_inner();

    let secured = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/debug")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(secured.status(), 401);

    let secured_with_creds = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/debug")
                .header("Authorization", basic_auth_header("admin", "admin-secret"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(secured_with_creds.status(), 200);

    // A route registered outside the closure is unaffected.
    let unrelated = app
        .oneshot(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unrelated.status(), 200);
}
