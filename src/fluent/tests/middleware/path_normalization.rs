//! Tests for path normalization middleware
//!
//! These tests verify that the NormalizePathLayer from tower-http works correctly
//! with Axum routers. The key insight is that NormalizePathLayer must be applied
//! using `tower::Layer::layer()` to wrap the router, not using `Router::layer()`.

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{Router, body::Body, http::Request, http::StatusCode, routing::get};
use tower::{Layer, ServiceExt};
use tower_http::normalize_path::NormalizePathLayer;

#[tokio::test]
async fn test_path_normalization_removes_trailing_slash() {
    let config = Config::default();
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/users", get(|| async { "users" })))
        .into_inner();

    // Apply normalization using tower::Layer::layer() to wrap the router
    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/users/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"users");
}

#[tokio::test]
async fn test_path_normalization_without_trailing_slash_unchanged() {
    let config = Config::default();
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/products", get(|| async { "products" })))
        .into_inner();

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/products")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_path_normalization_nested_paths() {
    let config = Config::default();
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/api/v1/users", get(|| async { "nested" })))
        .into_inner();

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_path_normalization_with_query_string() {
    let config = Config::default();
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/search", get(|| async { "search" })))
        .into_inner();

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/search/?q=test&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_path_normalization_with_path_parameters() {
    let config = Config::default();
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/users/{id}", get(|| async { "user detail" })))
        .into_inner();

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/users/123/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_path_normalization_disabled() {
    let config =
        Config::default().with_excluded_middlewares(vec![HttpMiddleware::PathNormalization]);
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .into_inner();

    // When disabled, don't apply the layer at all
    let response = router
        .oneshot(
            Request::builder()
                .uri("/test/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should return 404 because path normalization is disabled
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_path_normalization_preserves_multiple_routes() {
    let config = Config::default();
    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route("/api/users", get(|| async { "users" }))
                .route("/api/products", get(|| async { "products" }))
                .route("/api/orders", get(|| async { "orders" })),
        )
        .into_inner();

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    // Test multiple routes with trailing slashes
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/users/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/products/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_path_normalization_different_http_methods() {
    let config = Config::default();
    use axum::routing::{delete, post};

    let router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route("/resource", get(|| async { "get" }))
                .route("/resource", post(|| async { "post" }))
                .route("/resource", delete(|| async { "delete" })),
        )
        .into_inner();

    let app = NormalizePathLayer::trim_trailing_slash().layer(router);

    // Test GET
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/resource/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Test POST
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/resource/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
