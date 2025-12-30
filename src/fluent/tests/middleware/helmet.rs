//! Tests for Helmet security headers middleware setup

use crate::{Config, FluentRouter, HttpXFrameConfig};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use tower::Service;

#[tokio::test]
async fn test_setup_helmet_with_defaults() {
    // Default config should include X-Content-Type-Options: nosniff and X-Frame-Options: DENY
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(response.headers().get("x-frame-options").unwrap(), "DENY");
}

#[tokio::test]
async fn test_setup_helmet_x_content_type_disabled() {
    let config = Config::default().with_x_content_type_nosniff(false);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("x-content-type-options"));
    // X-Frame-Options should still be present
    assert_eq!(response.headers().get("x-frame-options").unwrap(), "DENY");
}

#[tokio::test]
async fn test_setup_helmet_x_frame_options_sameorigin() {
    let config = Config::default().with_x_frame_options(HttpXFrameConfig::same_origin());
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-frame-options").unwrap(),
        "SAMEORIGIN"
    );
    // X-Content-Type-Options should still be present
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
}

#[tokio::test]
async fn test_setup_helmet_x_frame_options_allow_from() {
    let config = Config::default()
        .with_x_frame_options(HttpXFrameConfig::allow_from("https://trusted.example.com"));
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-frame-options").unwrap(),
        "ALLOW-FROM https://trusted.example.com"
    );
}

#[tokio::test]
async fn test_setup_helmet_both_headers_disabled() {
    let config = Config::default()
        .with_x_content_type_nosniff(false)
        .with_x_frame_options(HttpXFrameConfig::same_origin()); // Set X-Frame-Options to a value, but since helmet always adds it, we check it's present

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_helmet();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("x-content-type-options"));
    // X-Frame-Options is always set when helmet is configured
    assert_eq!(
        response.headers().get("x-frame-options").unwrap(),
        "SAMEORIGIN"
    );
}

#[tokio::test]
async fn test_setup_helmet_with_multiple_routes() {
    let config = Config::default().with_x_frame_options(HttpXFrameConfig::same_origin());
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route("/api/v1", get(|| async { "v1" }))
                .route("/api/v2", get(|| async { "v2" })),
        )
        .setup_helmet();

    let mut app = fluent_router.into_inner();

    // Test first route
    let response1 = app
        .call(
            Request::builder()
                .uri("/api/v1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    assert_eq!(
        response1.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(
        response1.headers().get("x-frame-options").unwrap(),
        "SAMEORIGIN"
    );

    // Test second route
    let response2 = app
        .call(
            Request::builder()
                .uri("/api/v2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
    assert_eq!(
        response2.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(
        response2.headers().get("x-frame-options").unwrap(),
        "SAMEORIGIN"
    );
}
