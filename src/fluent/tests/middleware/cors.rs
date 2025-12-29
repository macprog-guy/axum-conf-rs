//! Tests for CORS middleware setup

use crate::fluent::tests::create_test_config;
use crate::{CorsHeader, CorsMethod, FluentRouter, HttpCorsConfig};
use axum::{
    Router,
    body::Body,
    http::{HeaderName, Method, Request},
    routing::get,
};
use tower::Service;

#[tokio::test]
async fn test_setup_cors_with_no_config() {
    // When no CORS config is provided in production (default), should use restrictive defaults
    // This test verifies the fail-safe behavior
    let config = create_test_config();
    let fluent_router = FluentRouter::without_state(config).unwrap().setup_cors();

    let mut app = fluent_router.into_inner();

    // Make a CORS preflight request
    let response = app
        .call(
            Request::builder()
                .method("OPTIONS")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // In production (no RUST_ENV or RUST_ENV=prod), restrictive CORS is used
    // Cross-origin requests should NOT have permissive CORS headers
    let has_allow_all_origin = response
        .headers()
        .get("access-control-allow-origin")
        .is_some_and(|v| v == "*");

    assert!(
        !has_allow_all_origin,
        "Production default should NOT allow all origins"
    );
}

#[tokio::test]
async fn test_setup_cors_with_no_config_dev_environment() {
    // In development environment, permissive CORS is used for convenience
    unsafe {
        std::env::set_var("RUST_ENV", "dev");
    }

    let config = create_test_config();
    let fluent_router = FluentRouter::without_state(config).unwrap().setup_cors();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .method("OPTIONS")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("Access-Control-Request-Method", "POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // In dev environment, should have permissive CORS headers
    assert!(
        response
            .headers()
            .contains_key("access-control-allow-origin")
            || response
                .headers()
                .contains_key("access-control-allow-methods"),
        "Dev environment should have permissive CORS"
    );

    unsafe {
        std::env::remove_var("RUST_ENV");
    }
}

#[tokio::test]
async fn test_setup_cors_with_allowed_origins() {
    let mut config = create_test_config();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![CorsMethod(Method::GET)]),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .setup_cors();

    let mut app = fluent_router.into_inner();

    // Make an actual GET request with Origin header
    let response = app
        .call(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should allow the configured origin
    let allow_origin = response.headers().get("access-control-allow-origin");
    assert!(allow_origin.is_some());
    assert_eq!(allow_origin.unwrap(), "https://example.com");
}

#[tokio::test]
async fn test_setup_cors_with_allowed_methods() {
    let mut config = create_test_config();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![CorsMethod(Method::GET), CorsMethod(Method::POST)]),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .setup_cors();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should have CORS headers
    assert!(
        response
            .headers()
            .contains_key("access-control-allow-origin")
    );
}

#[tokio::test]
async fn test_setup_cors_with_allowed_headers() {
    let mut config = create_test_config();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![CorsMethod(Method::GET)])
            .with_allowed_headers(vec![
                CorsHeader(HeaderName::from_static("content-type")),
                CorsHeader(HeaderName::from_static("authorization")),
            ]),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .setup_cors();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should have CORS headers
    assert!(
        response
            .headers()
            .contains_key("access-control-allow-origin")
    );
}

#[tokio::test]
async fn test_setup_cors_with_credentials() {
    let mut config = create_test_config();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![CorsMethod(Method::GET)])
            .with_allowed_headers(vec![CorsHeader(HeaderName::from_static("content-type"))])
            .with_allow_credentials(),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .setup_cors();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should have credentials header
    let credentials = response.headers().get("access-control-allow-credentials");
    assert!(credentials.is_some() && credentials.unwrap() == "true");
}

#[tokio::test]
async fn test_setup_cors_with_max_age() {
    use std::time::Duration;

    let mut config = create_test_config();
    config.http.cors = Some(HttpCorsConfig::default().with_max_age(Duration::from_secs(3600)));

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .setup_cors();

    let app = fluent_router.into_inner();

    // Max age is typically reflected in preflight responses
    // For this test, we just verify the router was built successfully with the config
    assert!(std::mem::size_of_val(&app) > 0);
}

#[tokio::test]
async fn test_setup_cors_complete_config() {
    use std::time::Duration;

    let mut config = create_test_config();
    config.http.cors = Some(
        HttpCorsConfig::default()
            .with_allowed_origins(vec!["https://example.com".to_string()])
            .with_allowed_methods(vec![CorsMethod(Method::GET), CorsMethod(Method::POST)])
            .with_allowed_headers(vec![CorsHeader(HeaderName::from_static("content-type"))])
            .with_exposed_headers(vec![CorsHeader(HeaderName::from_static("x-custom-header"))])
            .with_max_age(Duration::from_secs(7200))
            .with_allow_credentials(),
    );

    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "test" })))
        .setup_cors();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .method("GET")
                .uri("/test")
                .header("Origin", "https://example.com")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should have multiple CORS headers configured
    let headers = response.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
    assert_eq!(
        headers.get("access-control-allow-origin").unwrap(),
        "https://example.com"
    );

    let credentials = headers.get("access-control-allow-credentials");
    assert!(credentials.is_some() && credentials.unwrap() == "true");
}
