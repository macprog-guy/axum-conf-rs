//! Tests for liveness and readiness probe middleware setup

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{body::Body, http::{Request, StatusCode}};
use tower::Service;

#[tokio::test]
async fn test_liveness_readiness_individual_control() {
    // Test that liveness and readiness can be independently controlled
    let config = Config::default()
        .with_included_middlewares(vec![HttpMiddleware::Liveness]);    // Readiness is NOT included

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .setup_liveness_readiness();

    let mut app = fluent_router.into_inner();

    // Liveness should work
    let response = app
        .call(Request::builder().uri("/live").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
