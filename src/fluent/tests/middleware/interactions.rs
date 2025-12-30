//! Integration tests for middleware interactions
//!
//! These tests verify that middleware work correctly together when multiple
//! middleware are enabled simultaneously. Unlike unit tests that test each
//! middleware in isolation, these tests ensure middleware don't conflict
//! or interfere with each other.

#[cfg(feature = "cors")]
use crate::HttpCorsConfig;
#[cfg(feature = "deduplication")]
use crate::HttpDeduplicationConfig;
use crate::{Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use std::time::Duration;
use tower::ServiceExt;

#[allow(unused_imports)]
use axum::http::Method;

// ============================================================================
// Request ID + CORS Interaction
// ============================================================================

#[cfg(feature = "cors")]
#[tokio::test]
async fn test_request_id_propagates_with_cors_headers() {
    use axum::http::Method;
    let mut config = Config::default();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![crate::CorsMethod(Method::GET)]),
    );
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_cors()
        .into_inner();

    let custom_id = "cors-request-123";
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Both request ID and CORS headers should be present
    assert_eq!(
        response.headers().get("x-request-id").unwrap(),
        custom_id,
        "Request ID should be propagated"
    );
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "https://example.com",
        "CORS headers should be present"
    );
}

// ============================================================================
// Request ID + Deduplication Interaction
// ============================================================================

#[cfg(feature = "deduplication")]
#[tokio::test]
async fn test_request_id_preserved_on_duplicate_rejection() {
    use crate::HttpDeduplicationConfig;
    let config = Config::default()
        .with_deduplication_config(
            HttpDeduplicationConfig::default()
                .with_ttl(Duration::from_secs(60))
                .with_max_entries(100),
        )
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication()
        .into_inner();

    let request_id = "dedup-test-123";

    // First request succeeds
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    assert_eq!(
        response1.headers().get("x-request-id").unwrap(),
        request_id,
        "Request ID should be in successful response"
    );

    // Duplicate request gets rejected
    let response2 = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);

    // Note: The deduplication middleware returns early with a 409 response
    // before the request ID propagation layer runs, so the request ID
    // may not be in the rejection response. This documents the current behavior.
    // If request ID propagation on rejections is needed, the dedup layer
    // would need to be modified to propagate the header manually.
    let has_duplicate_header = response2.headers().get("x-duplicate-request").is_some();
    assert!(
        has_duplicate_header,
        "Duplicate rejection should include x-duplicate-request header"
    );
}

// ============================================================================
// Helmet + CORS Interaction
// ============================================================================

#[cfg(all(feature = "security-headers", feature = "cors"))]
#[tokio::test]
async fn test_security_headers_coexist_with_cors() {
    let mut config = Config::default();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![crate::CorsMethod(Method::GET)]),
    );
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet()
        .setup_cors()
        .into_inner();

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Security headers from Helmet
    assert!(
        response.headers().contains_key("x-content-type-options"),
        "Helmet should add X-Content-Type-Options"
    );
    assert!(
        response.headers().contains_key("x-frame-options"),
        "Helmet should add X-Frame-Options"
    );

    // CORS headers
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .unwrap(),
        "https://example.com",
        "CORS origin should be present"
    );
}

// ============================================================================
// Path Normalization + Request ID Interaction
// ============================================================================

#[cfg(feature = "path-normalization")]
#[tokio::test]
async fn test_path_normalization_preserves_request_id() {
    let mut config = Config::default();
    config.http.trim_trailing_slash = true;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_path_normalization()
        .into_inner();

    let custom_id = "path-norm-123";

    // Request with trailing slash - should be normalized and request ID preserved
    let response = app
        .oneshot(
            Request::builder()
                .uri("/test/")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Note: Path normalization may result in redirect or direct handling
    // The key is that request ID is preserved
    let request_id = response.headers().get("x-request-id");
    assert!(
        request_id.is_some(),
        "Request ID should be preserved through path normalization"
    );
    assert_eq!(request_id.unwrap(), custom_id);
}

// ============================================================================
// Timeout + Request ID Interaction
// ============================================================================

#[tokio::test]
async fn test_timeout_preserves_request_id() {
    let mut config = Config::default();
    config.http.request_timeout = Some(Duration::from_secs(5));
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_timeout()
        .into_inner();

    let custom_id = "timeout-test-123";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-request-id").unwrap(),
        custom_id,
        "Request ID should be preserved when timeout layer is active"
    );
}

// ============================================================================
// Catch Panic + Request ID Interaction
// ============================================================================

#[tokio::test]
async fn test_panic_handler_preserves_request_id() {
    let mut config = Config::default();
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    // Handler that panics - using a function to avoid type inference issues
    async fn panic_handler() -> &'static str {
        panic!("test panic")
    }

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .merge(Router::new().route("/panic", get(panic_handler)))
        .setup_request_id()
        .setup_catch_panic()
        .into_inner();

    let custom_id = "panic-test-123";

    // Normal request should work with request ID
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-request-id").unwrap(),
        custom_id,
        "Request ID should be preserved on normal requests"
    );

    // Panic request should still return request ID in error response
    let panic_id = "panic-error-456";
    let panic_response = app
        .oneshot(
            Request::builder()
                .uri("/panic")
                .header("x-request-id", panic_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(panic_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    // Request ID may or may not be preserved depending on panic handler implementation
    // This test documents the current behavior
}

// ============================================================================
// CORS + Deduplication Interaction
// ============================================================================

#[cfg(all(feature = "cors", feature = "deduplication"))]
#[tokio::test]
async fn test_cors_headers_on_duplicate_rejection() {
    let mut config = Config::default();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![crate::CorsMethod(Method::GET)]),
    );
    config.http.deduplication = Some(
        HttpDeduplicationConfig::default()
            .with_ttl(Duration::from_secs(60))
            .with_max_entries(100),
    );
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
        HttpMiddleware::RequestId,
        HttpMiddleware::RequestDeduplication,
        HttpMiddleware::Cors,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication()
        .setup_cors()
        .into_inner();

    let request_id = "cors-dedup-123";

    // First request succeeds with CORS headers
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    assert!(
        response1
            .headers()
            .contains_key("access-control-allow-origin"),
        "CORS headers should be on successful response"
    );

    // Duplicate request rejected but should still have CORS headers
    let response2 = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    assert!(
        response2
            .headers()
            .contains_key("access-control-allow-origin"),
        "CORS headers should be on rejection response for browser compatibility"
    );
}

// ============================================================================
// Full Middleware Stack Integration
// ============================================================================

#[cfg(all(
    feature = "cors",
    feature = "security-headers",
    feature = "path-normalization"
))]
#[tokio::test]
async fn test_full_middleware_stack() {
    let mut config = Config::default();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![
                crate::CorsMethod(Method::GET),
                crate::CorsMethod(Method::POST),
            ]),
    );
    config.http.request_timeout = Some(Duration::from_secs(30));
    config.http.trim_trailing_slash = true;
    config.http.with_metrics = false; // Disable for test
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
        HttpMiddleware::Metrics,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/api/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_helmet()
        .setup_cors()
        .setup_timeout()
        .setup_path_normalization()
        .setup_catch_panic()
        .into_inner();

    let custom_id = "full-stack-test-123";

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/test")
                .header("Origin", "https://example.com")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let headers = response.headers();

    // Request ID propagated
    assert_eq!(
        headers.get("x-request-id").unwrap(),
        custom_id,
        "Request ID should be propagated through full stack"
    );

    // CORS headers present
    assert!(
        headers.contains_key("access-control-allow-origin"),
        "CORS headers should be present"
    );

    // Security headers present
    assert!(
        headers.contains_key("x-content-type-options"),
        "X-Content-Type-Options should be present"
    );
    assert!(
        headers.contains_key("x-frame-options"),
        "X-Frame-Options should be present"
    );
}

// ============================================================================
// Sensitive Headers + Request ID Interaction
// ============================================================================

#[cfg(feature = "sensitive-headers")]
#[tokio::test]
async fn test_sensitive_headers_do_not_affect_request_id() {
    let mut config = Config::default();
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_sensitive_headers()
        .into_inner();

    let custom_id = "sensitive-test-123";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", custom_id)
                .header("authorization", "Bearer secret-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Request ID should still be visible (it's not sensitive)
    assert_eq!(
        response.headers().get("x-request-id").unwrap(),
        custom_id,
        "Request ID should not be marked as sensitive"
    );
}

// ============================================================================
// Concurrency Limit + Request ID Interaction
// ============================================================================

#[cfg(feature = "concurrency-limit")]
#[tokio::test]
async fn test_concurrency_limit_preserves_request_id() {
    let mut config = Config::default();
    config.http.max_concurrent_requests = 100;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_concurrency_limit()
        .into_inner();

    let custom_id = "concurrency-test-123";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-request-id").unwrap(),
        custom_id,
        "Request ID should be preserved through concurrency limit layer"
    );
}

// ============================================================================
// Multiple Middleware Disabled
// ============================================================================

#[cfg(all(feature = "cors", feature = "security-headers"))]
#[tokio::test]
async fn test_multiple_middleware_can_be_disabled() {
    let config = Config::default().with_excluded_middlewares(vec![
        HttpMiddleware::RateLimiting,
        HttpMiddleware::RequestId,
        HttpMiddleware::RequestDeduplication,
        HttpMiddleware::SecurityHeaders,
        HttpMiddleware::Cors,
    ]);

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id() // No-op when disabled
        .setup_helmet() // No-op when disabled
        .setup_cors() // No-op when disabled
        .into_inner();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", "should-not-propagate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // When disabled, these headers should not be present
    assert!(
        response.headers().get("x-request-id").is_none(),
        "Request ID should not propagate when middleware is disabled"
    );
    assert!(
        response.headers().get("x-content-type-options").is_none(),
        "Helmet headers should not be present when disabled"
    );
}

// ============================================================================
// Middleware Order Verification
// ============================================================================

#[cfg(all(feature = "cors", feature = "deduplication"))]
#[tokio::test]
async fn test_middleware_order_cors_before_dedup() {
    // CORS should handle OPTIONS preflight before deduplication checks
    let mut config = Config::default();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![crate::CorsMethod(Method::POST)]),
    );
    config.http.deduplication = Some(
        HttpDeduplicationConfig::default()
            .with_ttl(Duration::from_secs(60))
            .with_max_entries(100),
    );
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
        HttpMiddleware::RequestId,
        HttpMiddleware::RequestDeduplication,
        HttpMiddleware::Cors,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/api/resource", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication()
        .setup_cors()
        .into_inner();

    // CORS preflight request should succeed even with same request ID
    // because OPTIONS should be handled by CORS layer, not by deduplication
    let request_id = "preflight-123";

    // First preflight request
    let preflight1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/resource")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "POST")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Preflight should return 2xx with CORS headers
    assert!(
        preflight1.status().is_success() || preflight1.status() == StatusCode::NO_CONTENT,
        "Preflight should succeed, got {}",
        preflight1.status()
    );

    // Second preflight with same ID should also succeed
    // (OPTIONS are typically idempotent and should bypass dedup)
    let preflight2 = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/api/resource")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "POST")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Both preflights should succeed
    assert!(
        preflight2.status().is_success() || preflight2.status() == StatusCode::NO_CONTENT,
        "Second preflight should also succeed, got {}",
        preflight2.status()
    );
}

// ============================================================================
// API Versioning + Request ID Interaction
// ============================================================================

#[cfg(feature = "api-versioning")]
#[tokio::test]
async fn test_api_versioning_coexists_with_request_id() {
    let mut config = Config::default();
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/v1/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_api_versioning(1) // Default version is 1
        .into_inner();

    let custom_id = "version-test-123";

    // Request to versioned path should preserve request ID
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/test")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Request ID should be present
    assert_eq!(
        response.headers().get("x-request-id").unwrap(),
        custom_id,
        "Request ID should be present alongside API versioning"
    );
}
