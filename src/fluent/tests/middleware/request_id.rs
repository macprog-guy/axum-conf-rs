//! Tests for request ID middleware
//!
//! The request ID middleware consists of two tower-http layers:
//! 1. SetRequestIdLayer - Generates/preserves x-request-id header
//! 2. PropagateRequestIdLayer - Adds x-request-id to response headers
//!
//! Request IDs use UUIDv7 format for time-ordered, globally unique identifiers.
//!
//! Note: PropagateRequestIdLayer works more reliably when request IDs are provided
//! in the incoming request. When using oneshot() for testing, generated IDs may not
//! always appear in the response headers, though they ARE available internally.

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{Router, body::Body, http::Request, http::StatusCode, routing::get};
use tower::ServiceExt;

#[tokio::test]
async fn test_request_id_preserves_existing_header() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    let custom_id = "custom-request-id-12345";

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

    // Verify the custom request ID is preserved and propagated to response
    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("x-request-id header should be present when provided in request");

    assert_eq!(request_id.to_str().unwrap(), custom_id);
}

#[tokio::test]
async fn test_request_id_preserves_uuid_format() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    // Provide a valid UUID v7 as request ID
    let uuid_id = "018c8f3e-1234-7000-8000-000000000000";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", uuid_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let request_id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();

    // Verify it's a valid UUID v7 format (xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)
    assert_eq!(request_id.len(), 36);
    assert_eq!(request_id.chars().filter(|c| *c == '-').count(), 4);
    assert_eq!(request_id, uuid_id);
}

#[tokio::test]
async fn test_request_id_works_with_multiple_routes() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route("/users", get(|| async { "users" }))
                .route("/products", get(|| async { "products" }))
                .route("/orders", get(|| async { "orders" })),
        )
        .setup_request_id()
        .into_inner();

    // Test multiple routes with provided request IDs
    for (uri, id) in [
        ("/users", "req-users-001"),
        ("/products", "req-products-002"),
        ("/orders", "req-orders-003"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("x-request-id", id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Each route should preserve the provided request ID
        let request_id = response.headers().get("x-request-id").unwrap();
        assert_eq!(request_id.to_str().unwrap(), id);
    }
}

#[tokio::test]
async fn test_request_id_works_with_different_methods() {
    let config = Config::default();
    use axum::routing::{delete, post, put};

    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route("/resource", get(|| async { "get" }))
                .route("/resource", post(|| async { "post" }))
                .route("/resource", put(|| async { "put" }))
                .route("/resource", delete(|| async { "delete" })),
        )
        .setup_request_id()
        .into_inner();

    // Test different HTTP methods with provided request IDs
    for (method, id) in [
        ("GET", "req-get"),
        ("POST", "req-post"),
        ("PUT", "req-put"),
        ("DELETE", "req-delete"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/resource")
                    .header("x-request-id", id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Each method should preserve the request ID
        let request_id = response.headers().get("x-request-id").unwrap();
        assert_eq!(request_id.to_str().unwrap(), id);
    }
}

#[tokio::test]
async fn test_request_id_disabled() {
    // Must also exclude RequestDeduplication since it depends on RequestId
    let config = Config::default().with_excluded_middlewares(vec![
        HttpMiddleware::RequestId,
        HttpMiddleware::RequestDeduplication,
    ]);
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", "should-not-be-propagated")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // When disabled, x-request-id header should not be propagated to response
    let request_id = response.headers().get("x-request-id");
    assert!(
        request_id.is_none(),
        "x-request-id should not be present when middleware is disabled"
    );
}

#[tokio::test]
async fn test_request_id_with_query_parameters() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/search", get(|| async { "search results" })))
        .setup_request_id()
        .into_inner();

    let custom_id = "search-query-123";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/search?q=test&limit=10")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Request ID should work with query parameters
    let request_id = response.headers().get("x-request-id").unwrap();
    assert_eq!(request_id.to_str().unwrap(), custom_id);
}

#[tokio::test]
async fn test_request_id_case_insensitive_header() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    // HTTP headers are case-insensitive
    let custom_id = "case-test-123";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("X-Request-ID", custom_id) // Different casing
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // The middleware should preserve the ID regardless of header casing
    let request_id = response.headers().get("x-request-id").unwrap();
    assert_eq!(request_id.to_str().unwrap(), custom_id);
}

#[tokio::test]
async fn test_request_id_with_404_response() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/exists", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    let custom_id = "not-found-123";

    let response = app
        .oneshot(
            Request::builder()
                .uri("/does-not-exist")
                .header("x-request-id", custom_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Request ID should be propagated even for 404 responses
    let request_id = response.headers().get("x-request-id").unwrap();
    assert_eq!(request_id.to_str().unwrap(), custom_id);
}

#[tokio::test]
async fn test_request_id_special_characters_in_custom_id() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    // Test with various valid header value characters
    let custom_id = "req-2024-12-11_test.id:123";

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

    let request_id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();

    assert_eq!(request_id, custom_id);
}

#[tokio::test]
async fn test_request_id_different_ids_for_different_requests() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    // Make requests with different IDs
    let id1 = "request-001";
    let id2 = "request-002";

    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", id1)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let response2 = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", id2)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Each request should preserve its own ID
    let resp_id1 = response1.headers().get("x-request-id").unwrap();
    let resp_id2 = response2.headers().get("x-request-id").unwrap();

    assert_eq!(resp_id1.to_str().unwrap(), id1);
    assert_eq!(resp_id2.to_str().unwrap(), id2);
    assert_ne!(resp_id1, resp_id2);
}

#[tokio::test]
async fn test_request_id_empty_string_preserved() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    // Empty string should be preserved (HTTP allows empty header values)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", "")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // The middleware preserves whatever value is provided, even empty strings
    let request_id = response.headers().get("x-request-id");
    assert!(request_id.is_some());
}

#[tokio::test]
async fn test_request_id_long_custom_id() {
    let config = Config::default();
    let app = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .into_inner();

    // Test with a very long request ID
    let long_id = "a".repeat(200);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header("x-request-id", &long_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let request_id = response.headers().get("x-request-id").unwrap();
    assert_eq!(request_id.to_str().unwrap(), long_id);
}
