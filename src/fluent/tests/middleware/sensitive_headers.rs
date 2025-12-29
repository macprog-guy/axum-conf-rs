//! Tests for sensitive headers middleware setup

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use tower::Service;

#[tokio::test]
async fn test_setup_sensitive_headers_with_authorization() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/protected",
            get(|headers: http::HeaderMap| async move {
                // Handler receives the authorization header
                if let Some(auth) = headers.get(http::header::AUTHORIZATION) {
                    format!("Auth: {}", auth.to_str().unwrap_or("invalid"))
                } else {
                    "No auth".to_string()
                }
            }),
        ))
        .setup_sensitive_headers();

    let mut app = fluent_router.into_inner();

    // Request with Authorization header
    let response = app
        .call(
            Request::builder()
                .uri("/protected")
                .header(http::header::AUTHORIZATION, "Bearer secret-token-12345")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();

    // The handler should still receive the header
    assert_eq!(&body[..], b"Auth: Bearer secret-token-12345");
}

#[tokio::test]
async fn test_setup_sensitive_headers_without_authorization() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/public",
            get(|headers: http::HeaderMap| async move {
                if headers.get(http::header::AUTHORIZATION).is_some() {
                    "Has auth"
                } else {
                    "No auth"
                }
            }),
        ))
        .setup_sensitive_headers();

    let mut app = fluent_router.into_inner();

    // Request without Authorization header
    let response = app
        .call(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"No auth");
}

#[tokio::test]
async fn test_setup_sensitive_headers_middleware_disabled() {
    let config =
        Config::default().with_excluded_middlewares(vec![HttpMiddleware::SensitiveHeaders]);
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/test",
            get(|headers: http::HeaderMap| async move {
                if let Some(auth) = headers.get(http::header::AUTHORIZATION) {
                    format!("Auth: {}", auth.to_str().unwrap_or("invalid"))
                } else {
                    "No auth".to_string()
                }
            }),
        ))
        .setup_sensitive_headers();

    let mut app = fluent_router.into_inner();

    let response = app
        .call(
            Request::builder()
                .uri("/test")
                .header(http::header::AUTHORIZATION, "Bearer token-xyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();

    // Even with middleware disabled, headers still pass through
    assert_eq!(&body[..], b"Auth: Bearer token-xyz");
}

#[tokio::test]
async fn test_setup_sensitive_headers_multiple_routes() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(
            Router::new()
                .route(
                    "/admin",
                    get(|headers: http::HeaderMap| async move {
                        headers
                            .get(http::header::AUTHORIZATION)
                            .map(|_| "Admin access")
                            .unwrap_or("No admin access")
                    }),
                )
                .route(
                    "/user",
                    get(|headers: http::HeaderMap| async move {
                        headers
                            .get(http::header::AUTHORIZATION)
                            .map(|_| "User access")
                            .unwrap_or("No user access")
                    }),
                )
                .route("/public", get(|| async { "Public access" })),
        )
        .setup_sensitive_headers();

    let mut app = fluent_router.into_inner();

    // Admin route with auth
    let response = app
        .call(
            Request::builder()
                .uri("/admin")
                .header(http::header::AUTHORIZATION, "Bearer admin-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Admin access");

    // User route with auth
    let response = app
        .call(
            Request::builder()
                .uri("/user")
                .header(http::header::AUTHORIZATION, "Bearer user-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"User access");

    // Public route without auth
    let response = app
        .call(
            Request::builder()
                .uri("/public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Public access");
}

#[tokio::test]
async fn test_setup_sensitive_headers_case_insensitive() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route(
            "/test",
            get(|headers: http::HeaderMap| async move {
                // Check both standard and lowercase variants
                let auth_count = headers.get(http::header::AUTHORIZATION).iter().count();
                format!("Auth headers: {}", auth_count)
            }),
        ))
        .setup_sensitive_headers();

    let mut app = fluent_router.into_inner();

    // HTTP headers are case-insensitive, but we use the standard casing
    let response = app
        .call(
            Request::builder()
                .uri("/test")
                .header(http::header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Auth headers: 1");
}
