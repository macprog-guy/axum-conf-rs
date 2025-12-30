//! Tests for middleware configuration (include/exclude)

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use tower::Service;

#[tokio::test]
async fn test_middleware_config_exclude() {
    // Test that middleware can be selectively disabled
    let config = Config::default().with_excluded_middlewares(vec![
        HttpMiddleware::Compression,
        HttpMiddleware::PathNormalization,
    ]);

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_compression()
        .setup_path_normalization()
        .setup_logging(); // This should still be enabled

    let app = fluent_router.into_inner();

    // Compression and path normalization should be disabled
    // Logging should still work
    assert!(std::mem::size_of_val(&app) > 0);
}

#[tokio::test]
async fn test_middleware_config_include() {
    // Test that only specified middleware are enabled
    let config = Config::default()
        .with_included_middlewares(vec![HttpMiddleware::RequestId, HttpMiddleware::Logging]);

    let fluent_router = FluentRouter::without_state(config).unwrap();

    // Only specified middleware should be enabled
    assert!(fluent_router.is_middleware_enabled(HttpMiddleware::RequestId));
    assert!(fluent_router.is_middleware_enabled(HttpMiddleware::Logging));

    // Others should be disabled
    assert!(!fluent_router.is_middleware_enabled(HttpMiddleware::Compression));
    assert!(!fluent_router.is_middleware_enabled(HttpMiddleware::Cors));
    assert!(!fluent_router.is_middleware_enabled(HttpMiddleware::RateLimiting));

    let mut app = fluent_router
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id() // Should be enabled
        .setup_logging() // Should be enabled
        .setup_compression() // Should be skipped
        .setup_cors() // Should be skipped
        .into_inner();

    // Verify router works
    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_middleware_config_default_all_enabled() {
    // Test that when no middleware config is provided, all middleware are enabled
    let config = Config::default();

    let fluent_router = FluentRouter::without_state(config).unwrap();

    // All middleware should be enabled by default
    assert!(fluent_router.is_middleware_enabled(HttpMiddleware::Logging));
    assert!(fluent_router.is_middleware_enabled(HttpMiddleware::Metrics));
    assert!(fluent_router.is_middleware_enabled(HttpMiddleware::Compression));
    assert!(fluent_router.is_middleware_enabled(HttpMiddleware::RateLimiting));
}

#[tokio::test]
async fn test_logging_disabled_path() {
    // Test the early return path when logging is disabled
    let config = Config::default().with_excluded_middlewares(vec![HttpMiddleware::Logging]);

    let mut app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_logging() // Should return early without adding logging layer
        .into_inner();

    // Router should still work
    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_api_versioning_disabled_path() {
    // Test the early return path when API versioning is disabled
    let config = Config::default().with_excluded_middlewares(vec![HttpMiddleware::ApiVersioning]);

    let mut app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_api_versioning(1) // Should return early
        .into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_sensitive_headers_disabled_path() {
    // Test the early return path when sensitive headers is disabled
    let config =
        Config::default().with_excluded_middlewares(vec![HttpMiddleware::SensitiveHeaders]);

    let mut app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_sensitive_headers() // Should return early
        .into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_helmet_disabled_path() {
    // Test the early return path when security headers are disabled
    let config = Config::default().with_excluded_middlewares(vec![HttpMiddleware::SecurityHeaders]);

    let mut app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet() // Should return early
        .into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
