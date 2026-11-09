#![cfg(feature = "session")]

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
async fn test_memory_session_basic() {
    // Create config with memory session (default)
    let config = Config::new().with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]); // Disable rate limiting for tests (requires ConnectInfo which isn't available in oneshot tests)
    // Disable rate limiting for tests (oneshot() doesn't provide ConnectInfo<SocketAddr>)

    // Create router with session handling
    let app = FluentRouter::without_state(config)
        .unwrap()
        .route("/counter", get(session_handler))
        .setup_session_handling()
        .await
        .unwrap()
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

async fn rotate_handler(
    session: tower_sessions::Session,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    session
        .cycle_id()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok("rotated")
}

/// Sends `GET <uri>` with an optional cookie; returns (set-cookie pair, body).
async fn request(app: &axum::Router, uri: &str, cookie: Option<&str>) -> (Option<String>, String) {
    let mut builder = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (set_cookie, String::from_utf8_lossy(&body).into_owned())
}

/// `Session::cycle_id()` (as called by the OIDC callback) issues a new id, keeps
/// the session data, and invalidates the old id.
#[tokio::test]
async fn test_memory_session_cycle_id_rotates_and_keeps_data() {
    let config = Config::new().with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);
    let app = FluentRouter::without_state(config)
        .unwrap()
        .route("/counter", get(session_handler))
        .route("/rotate", get(rotate_handler))
        .setup_session_handling()
        .await
        .unwrap()
        .into_inner();

    let (cookie, body) = request(&app, "/counter", None).await;
    assert_eq!(body, "Counter: 1");
    let old_cookie = cookie.expect("session cookie should be set");

    let (new_cookie, body) = request(&app, "/rotate", Some(&old_cookie)).await;
    assert_eq!(body, "rotated");
    let new_cookie = new_cookie.expect("rotation should issue a new cookie");
    assert_ne!(old_cookie, new_cookie, "session id must change");

    // Data carried over to the new id.
    let (_, body) = request(&app, "/counter", Some(&new_cookie)).await;
    assert_eq!(body, "Counter: 2");

    // The old id no longer resolves to the session (a fresh one starts at 1).
    let (_, body) = request(&app, "/counter", Some(&old_cookie)).await;
    assert_eq!(body, "Counter: 1", "the old session id must be invalidated");
}
