//! Tests for panic catching middleware setup

use crate::{Config, FluentRouter};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use tower::Service;

#[tokio::test]
async fn test_setup_catch_panic_with_panic() {
    let config = Config::default();
    // Create a route that panics
    let panic_router = Router::new().route(
        "/panic",
        get(|| async {
            panic!("Test panic!");
            #[allow(unreachable_code)]
            "This will never be reached"
        }),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(panic_router)
        .setup_catch_panic();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/panic")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 500 Internal Server Error
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // Should have the correct content type
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );

    // Check the body
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Internal Server Error");
}

#[tokio::test]
async fn test_setup_catch_panic_normal_request() {
    let config = Config::default();
    // Create a normal route
    let normal_router = Router::new().route("/normal", get(|| async { "OK" }));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(normal_router)
        .setup_catch_panic();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/normal")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 200 OK for normal requests
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"OK");
}

#[tokio::test]
async fn test_with_panic_notification_channel() {
    let config = Config::default();
    // Create a channel to receive panic notifications
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);

    // Create a route that panics
    let panic_router = Router::new().route(
        "/panic_notify",
        get(|| async {
            panic!("Notification test panic!");
            #[allow(unreachable_code)]
            "This will never be reached"
        }),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .with_panic_notification_channel(tx)
        .merge(panic_router)
        .setup_catch_panic();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/panic_notify")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 500 Internal Server Error
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // Check that we received a panic notification
    let notification = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;

    assert!(notification.is_ok());
    let msg = notification.unwrap().unwrap();
    assert!(msg.contains("Service panicked"));
    assert!(msg.contains("Notification test panic!"));
}

#[tokio::test]
async fn test_with_panic_notification_channel_no_panic() {
    let config = Config::default();
    // Create a channel to receive panic notifications
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);

    // Create a normal route
    let normal_router = Router::new().route("/no_panic", get(|| async { "All good" }));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .with_panic_notification_channel(tx)
        .merge(normal_router)
        .setup_catch_panic();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/no_panic")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 200 OK
    assert_eq!(response.status(), StatusCode::OK);

    // Check that no panic notification was sent
    let notification = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;

    // Should timeout since no panic occurred
    assert!(notification.is_err());
}

#[tokio::test]
async fn test_catch_panic_with_string_panic() {
    let config = Config::default();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);

    // Create a route that panics with a String
    let panic_router = Router::new().route(
        "/string_panic",
        get(|| async {
            panic!("String panic message");
            #[allow(unreachable_code)]
            "This will never be reached"
        }),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .with_panic_notification_channel(tx)
        .merge(panic_router)
        .setup_catch_panic();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/string_panic")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let notification = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;

    assert!(notification.is_ok());
    let msg = notification.unwrap().unwrap();
    assert!(msg.contains("String panic message"));
}
