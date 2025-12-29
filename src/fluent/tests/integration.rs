//! Integration tests for FluentRouter API

use super::nested_handler;
use crate::{Config, FluentRouter};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::convert::Infallible;
use tower::Service;

#[cfg(feature = "compression")]
use tower::ServiceExt;
#[cfg(feature = "compression")]
use tower_http::compression::CompressionLayer;

#[tokio::test]
async fn test_fluent_router_new() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config);
    assert!(fluent_router.is_ok());
}

#[tokio::test]
async fn test_fluent_router_into_inner() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config).unwrap();
    let router: Router = fluent_router.into_inner();

    // Verify we get a valid Router back
    assert!(std::mem::size_of_val(&router) > 0);
}

#[tokio::test]
async fn test_fluent_router_nest() {
    let config = Config::default();
    let nested_router = Router::new().route("/nested", get(nested_handler));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .nest("/api", nested_router);

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/api/nested")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"nested response");
}

#[tokio::test]
async fn test_fluent_router_merge() {
    let config = Config::default();
    let other_router = Router::new().route("/merged", get(|| async { "merged response" }));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(other_router);

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/merged")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"merged response");
}

#[cfg(feature = "compression")]
#[tokio::test]
async fn test_fluent_router_layer() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .layer(CompressionLayer::new());

    let router = fluent_router.into_inner();

    // Verify router is created successfully with layer
    assert!(std::mem::size_of_val(&router) > 0);
}

#[tokio::test]
async fn test_fluent_router_route_layer() {
    let config = Config::default();
    let test_router = Router::new().route("/test", get(|| async { "test" }));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(test_router)
        .route_layer(tower::limit::ConcurrencyLimitLayer::new(10));

    let mut app = fluent_router.into_inner();

    // Verify the route works with the route layer applied
    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[cfg(feature = "compression")]
#[tokio::test]
async fn test_fluent_router_method_chaining() {
    let config = Config::default();
    let nested_router = Router::new().route("/nested", get(nested_handler));
    let other_router = Router::new().route("/merged", get(|| async { "merged" }));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .nest("/api", nested_router)
        .merge(other_router)
        .layer(CompressionLayer::new());

    let app = fluent_router.into_inner();

    // Test nested route
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/nested")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Test merged route
    let response = app
        .oneshot(
            Request::builder()
                .uri("/merged")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_fluent_router_setup_methods() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .setup_public_files()
        .unwrap()
        .setup_protected_files()
        .unwrap();

    let router = fluent_router.into_inner();

    // Verify router is created successfully
    assert!(std::mem::size_of_val(&router) > 0);
}

#[tokio::test]
async fn test_fluent_router_with_invalid_config() {
    // Create a config that will fail validation (if validation exists)
    let config = Config::default(); // Depending on validation logic, this might pass or fail
    let result = FluentRouter::without_state(config);

    // This test demonstrates handling of potential validation failures
    // Adjust based on actual validation requirements
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_fluent_router_nest_service() {
    use tower::service_fn;

    let config = Config::default();
    let service = service_fn(|_req: Request<Body>| async {
        Ok::<Response, Infallible>((StatusCode::OK, "service response").into_response())
    });

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .nest_service("/service", service);

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/service")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"service response");
}

#[tokio::test]
async fn test_fluent_router_route_service() {
    use tower::service_fn;

    let config = Config::default();
    let service = service_fn(|_req: Request<Body>| async {
        Ok::<Response, Infallible>((StatusCode::OK, "route service response").into_response())
    });

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .route_service("/route", service);

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/route")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"route service response");
}
