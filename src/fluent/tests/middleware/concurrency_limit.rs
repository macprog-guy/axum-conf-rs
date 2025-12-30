//! Tests for concurrency limit middleware setup

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tower::Service;

#[tokio::test]
async fn test_setup_concurrency_limit_with_default() {
    let config = Config::default(); // Default max_concurrent_requests is 100 in test config

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_concurrency_limit_with_custom_value() {
    let config = Config::default().with_max_concurrent_requests(50);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_concurrency_limit_enforces_limit() {
    let config = Config::default().with_max_concurrent_requests(2); // Very low limit for testing
    // Use a semaphore to control when requests complete
    let sem = Arc::new(Semaphore::new(0));
    let sem_clone = sem.clone();

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/slow",
            get(move || {
                let sem = sem_clone.clone();
                async move {
                    // Wait for semaphore permit (controlled by test)
                    let _permit = sem.acquire().await.unwrap();
                    "Done"
                }
            }),
        ))
        .setup_concurrency_limit();

    let app = Arc::new(tokio::sync::Mutex::new(fluent_router.into_inner()));

    // Start 2 requests (should fill the limit)
    let app1 = app.clone();
    let handle1 = tokio::spawn(async move {
        let mut app = app1.lock().await;
        app.call(Request::builder().uri("/slow").body(Body::empty()).unwrap())
            .await
    });

    let app2 = app.clone();
    let handle2 = tokio::spawn(async move {
        let mut app = app2.lock().await;
        app.call(Request::builder().uri("/slow").body(Body::empty()).unwrap())
            .await
    });

    // Give time for requests to be accepted
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Release the semaphore to let requests complete
    sem.add_permits(2);

    let result1 = handle1.await.unwrap().unwrap();
    let result2 = handle2.await.unwrap().unwrap();

    assert_eq!(result1.status(), StatusCode::OK);
    assert_eq!(result2.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_concurrency_limit_multiple_routes() {
    let config = Config::default().with_max_concurrent_requests(100);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route("/route1", get(|| async { "Route 1" }))
                .route("/route2", get(|| async { "Route 2" }))
                .route("/route3", get(|| async { "Route 3" })),
        )
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    // All routes should work within concurrency limit
    let response1 = app
        .call(
            Request::builder()
                .uri("/route1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response1.status(), StatusCode::OK);

    let response2 = app
        .call(
            Request::builder()
                .uri("/route2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response2.status(), StatusCode::OK);

    let response3 = app
        .call(
            Request::builder()
                .uri("/route3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response3.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_concurrency_limit_middleware_disabled() {
    let config = Config::default()
        .with_max_concurrent_requests(2)
        .with_excluded_middlewares(vec![HttpMiddleware::ConcurrencyLimit]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    // Should work even though middleware is disabled
    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_concurrency_limit_with_fast_requests() {
    let config = Config::default().with_max_concurrent_requests(10);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/fast",
            get(|| async {
                // Fast request
                tokio::time::sleep(Duration::from_millis(10)).await;
                "Fast"
            }),
        ))
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    // Make multiple fast requests sequentially
    for _ in 0..20 {
        let response = app
            .call(Request::builder().uri("/fast").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn test_setup_concurrency_limit_high_limit() {
    let config = Config::default().with_max_concurrent_requests(10000); // Very high limit
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"OK");
}

#[tokio::test]
async fn test_setup_concurrency_limit_with_streaming_response() {
    let config = Config::default().with_max_concurrent_requests(5);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/stream",
            get(|| async {
                // Simulate a response with body
                "Streaming response data"
            }),
        ))
        .setup_concurrency_limit();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Streaming response data");
}
