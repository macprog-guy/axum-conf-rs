#![cfg(feature = "postgres")]

use crate::{Config, FluentRouter, HttpMiddleware};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
// use testcontainers_modules::{postgres, testcontainers::runners::AsyncRunner};
use tower::ServiceExt;

async fn session_handler(
    session: tower_sessions::Session,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let counter: i32 = session.get("counter").await.unwrap().unwrap_or(0);
    let new_counter = counter + 1;
    session
        .insert("counter", new_counter)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(format!("Counter: {}", new_counter))
}

#[tokio::test]
#[cfg(feature = "session")]
async fn test_memory_session_basic() {
    // Create config with memory session (default)
    let config = Config::default()
        .with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);    // Disable rate limiting for tests (requires ConnectInfo which isn't available in oneshot tests)
    // Disable rate limiting for tests (oneshot() doesn't provide ConnectInfo<SocketAddr>)

    // Create router with session handling
    let app = FluentRouter::without_state(config)
        .unwrap()
        .route("/counter", get(session_handler))
        .setup_session_handling()
        .into_inner();

    // First request - should create a session and return counter=1
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    eprintln!("Response1: {:?} = {:?}", response1, response1.body());
    assert_eq!(response1.status(), StatusCode::OK);

    // Extract session cookie
    let session_cookie = response1
        .headers()
        .get(header::SET_COOKIE)
        .expect("Session cookie should be set")
        .to_str()
        .unwrap()
        .to_string();

    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Counter: 1");

    // Second request with the same cookie - should increment counter to 2
    let response2 = app
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header(header::COOKIE, session_cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body2, "Counter: 2");
}
