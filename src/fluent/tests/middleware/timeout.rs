//! Tests for request timeout middleware setup

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use std::time::Duration;
use tower::Service;

#[tokio::test]
async fn test_setup_timeout_with_slow_handler() {
    let config = Config::default()
        .with_request_timeout(Duration::from_millis(100))
        .with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                "This should timeout"
            }),
        ))
        .setup_timeout();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should return 408 Request Timeout
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_setup_timeout_with_fast_handler() {
    let config = Config::default()
        .with_request_timeout(Duration::from_millis(200))
        .with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/fast",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                "Fast response"
            }),
        ))
        .setup_timeout();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/fast").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should complete successfully
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Fast response");
}

#[tokio::test]
async fn test_setup_timeout_disabled_by_default() {
    let config = Config::default(); // No request_timeout configured

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                "Slow but no timeout"
            }),
        ))
        .setup_timeout();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should complete successfully since timeout is not configured
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Slow but no timeout");
}

#[tokio::test]
async fn test_setup_timeout_middleware_disabled() {
    let config = Config::default()
        .with_request_timeout(Duration::from_millis(100))
        .with_excluded_middlewares(vec![HttpMiddleware::Timeout, HttpMiddleware::RateLimiting]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                "Should not timeout"
            }),
        ))
        .setup_timeout();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should complete successfully since timeout middleware is disabled
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_timeout_with_multiple_routes() {
    let config = Config::default()
        .with_request_timeout(Duration::from_millis(150))
        .with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route(
                    "/fast",
                    get(|| async {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        "Fast"
                    }),
                )
                .route(
                    "/slow",
                    get(|| async {
                        tokio::time::sleep(Duration::from_millis(300)).await;
                        "Slow"
                    }),
                )
                .route("/instant", get(|| async { "Instant" })),
        )
        .setup_timeout();

    let mut app = fluent_router.into_inner();

    // Test instant route
    let response1 = app
        .call(
            Request::builder()
                .uri("/instant")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response1.status(), StatusCode::OK);

    // Test fast route
    let response2 = app
        .call(Request::builder().uri("/fast").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response2.status(), StatusCode::OK);

    // Test slow route - should timeout
    let response3 = app
        .call(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response3.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn test_setup_timeout_exactly_at_limit() {
    let config = Config::default()
        .with_request_timeout(Duration::from_millis(100))
        .with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/edge",
            get(|| async {
                // Sleep for slightly less than timeout
                tokio::time::sleep(Duration::from_millis(80)).await;
                "Just made it"
            }),
        ))
        .setup_timeout();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/edge").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should complete successfully
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Just made it");
}
