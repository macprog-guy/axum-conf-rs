//! Tests for max payload size middleware setup

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use tower::ServiceExt;

#[tokio::test]
async fn test_setup_max_payload_size_accepts_small_body() {
    let config = Config::default(); // Default is 1KiB in test config

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|body: String| async move { format!("Received: {} bytes", body.len()) }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send a small body (500 bytes, well under 1KiB limit)
    let small_body = "x".repeat(500);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(small_body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Received: 500 bytes");
}

#[tokio::test]
async fn test_setup_max_payload_size_rejects_large_body() {
    let config = Config::default().with_max_payload_size_bytes(1024); // 1KiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|_body: String| async move { "Should not reach here" }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send a body larger than 1KiB limit
    let large_body = "x".repeat(2048); // 2KiB
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(large_body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 413 Payload Too Large
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_setup_max_payload_size_exactly_at_limit() {
    let config = Config::default().with_max_payload_size_bytes(1024); // 1KiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|body: String| async move { format!("Received: {} bytes", body.len()) }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send exactly 1KiB
    let exact_body = "x".repeat(1024);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(exact_body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should accept exactly at the limit
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Received: 1024 bytes");
}

#[tokio::test]
async fn test_setup_max_payload_size_one_byte_over_limit() {
    let config = Config::default().with_max_payload_size_bytes(1024); // 1KiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|_body: String| async move { "Should not reach here" }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send 1 byte over the limit
    let over_body = "x".repeat(1025);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(over_body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should reject
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_setup_max_payload_size_with_custom_size() {
    let config = Config::default().with_max_payload_size_bytes(5 * 1024); // 5KiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|body: String| async move { format!("Received: {} bytes", body.len()) }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send 4KiB (under limit)
    let body_4k = "x".repeat(4096);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(body_4k))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Received: 4096 bytes");
}

#[tokio::test]
async fn test_setup_max_payload_size_middleware_disabled() {
    let config = Config::default()
        .with_max_payload_size_bytes(100) // Very small limit
        .with_excluded_middlewares(vec![HttpMiddleware::MaxPayloadSize]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|body: String| async move { format!("Received: {} bytes", body.len()) }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send a body larger than the configured limit
    // But since middleware is disabled, it should accept it
    let large_body = "x".repeat(1000);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(large_body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should accept since middleware is disabled
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_max_payload_size_empty_body() {
    let config = Config::default().with_max_payload_size_bytes(1024);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|body: String| async move { format!("Received: {} bytes", body.len()) }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send an empty body
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Received: 0 bytes");
}

#[tokio::test]
async fn test_setup_max_payload_size_multiple_routes() {
    let config = Config::default().with_max_payload_size_bytes(1024); // 1KiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route(
                    "/upload1",
                    post(|body: String| async move { format!("Route1: {} bytes", body.len()) }),
                )
                .route(
                    "/upload2",
                    post(|body: String| async move { format!("Route2: {} bytes", body.len()) }),
                )
                .route(
                    "/upload3",
                    post(|body: String| async move { format!("Route3: {} bytes", body.len()) }),
                ),
        )
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Test route 1 with small body
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload1")
                .body(Body::from("x".repeat(500)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response1.status(), StatusCode::OK);

    // Test route 2 with large body
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload2")
                .body(Body::from("x".repeat(2000)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response2.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // Test route 3 with body at limit
    let response3 = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload3")
                .body(Body::from("x".repeat(1024)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response3.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_setup_max_payload_size_large_limit() {
    let config = Config::default().with_max_payload_size_bytes(10 * 1024 * 1024); // 10MiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(|body: String| async move { format!("Received: {} bytes", body.len()) }),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send 1MiB (well under 10MiB limit)
    let body_1m = "x".repeat(1024 * 1024);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .body(Body::from(body_1m))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Received: 1048576 bytes");
}

#[tokio::test]
async fn test_setup_max_payload_size_binary_data() {
    let config = Config::default().with_max_payload_size_bytes(2048); // 2KiB
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/upload",
            post(
                |body: axum::body::Bytes| async move { format!("Received: {} bytes", body.len()) },
            ),
        ))
        .setup_max_payload_size();

    let app = fluent_router.into_inner();

    // Send binary data (1KiB of random-ish bytes)
    let binary_data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/upload")
                .header("content-type", "application/octet-stream")
                .body(Body::from(binary_data))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Received: 1024 bytes");
}
