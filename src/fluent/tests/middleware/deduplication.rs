//! Tests for request deduplication middleware setup
use crate::{Config, FluentRouter, HttpDeduplicationConfig, HttpMiddleware};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::{Arc, atomic::AtomicU32, atomic::Ordering};
use tower::{Service, ServiceExt};

/// Application state containing a counter
#[derive(Clone)]
struct AppState {
    counter: Arc<AtomicU32>,
}

impl AppState {
    fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU32::new(0)),
        }
    }

    fn get(&self) -> u32 {
        self.counter.load(Ordering::SeqCst)
    }
}

/// Handler that increments a counter on each call
async fn counter_handler(State(state): State<AppState>) -> impl IntoResponse {
    let count = state.counter.fetch_add(1, Ordering::SeqCst) + 1;
    format!("Count: {}", count)
}

#[tokio::test]
async fn test_deduplication_disabled_by_default() {
    let config = Config::default();
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication();

    let mut app = fluent_router.into_inner();

    // Make two requests with the same request ID
    let request_id = "test-request-id-123";

    let response1 = app
        .call(
            Request::builder()
                .uri("/test")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Second request with same ID should also succeed when deduplication is disabled
    let response2 = app
        .call(
            Request::builder()
                .uri("/test")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_deduplication_returns_conflict_response() {
    let config = Config::default().with_deduplication_config(
        HttpDeduplicationConfig::default()
            .with_ttl(std::time::Duration::from_secs(5))
            .with_max_entries(100),
    );
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication();

    let mut app = fluent_router.into_inner();

    let request_id = "test-duplicate-123";

    // First request should succeed
    let response1 = app
        .call(
            Request::builder()
                .uri("/test")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Second request with same ID should return 409 Conflict with error message
    let response2 = app
        .call(
            Request::builder()
                .uri("/test")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);

    let body = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], b"Duplicate request detected");
}

#[tokio::test]
async fn test_deduplication_allows_different_request_ids() {
    let config = Config::default().with_deduplication_config(
        HttpDeduplicationConfig::default()
            .with_ttl(std::time::Duration::from_secs(5))
            .with_max_entries(100),
    );
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication();

    let mut app = fluent_router.into_inner();

    // First request
    let response1 = app
        .call(
            Request::builder()
                .uri("/test")
                .header("x-request-id", "request-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Second request with different ID should succeed
    let response2 = app
        .call(
            Request::builder()
                .uri("/test")
                .header("x-request-id", "request-2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_deduplication_allows_request_without_id() {
    let config = Config::default().with_deduplication_config(
        HttpDeduplicationConfig::default()
            .with_ttl(std::time::Duration::from_secs(5))
            .with_max_entries(100),
    );
    let fluent_router = FluentRouter::without_state(config)
        .unwrap()
        .merge(Router::new().route("/test", get(|| async { "OK" })))
        .setup_request_id()
        .setup_deduplication();

    let mut app = fluent_router.into_inner();

    // Request without x-request-id header should succeed
    let response = app
        .call(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_deduplication_basic_idempotency() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(HttpDeduplicationConfig {
            ttl: std::time::Duration::from_secs(60),
            max_entries: 1000,
        })
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    config.setup_tracing();

    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", post(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    let request_id = "test-dedup-001";

    // First request - should increment counter to 1
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    // Second request with same ID - should return 409 Conflict
    // Counter should NOT increment
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    assert!(response2.headers().get("x-duplicate-request").is_some());

    // Verify counter only incremented once
    assert_eq!(state.get(), 1);
}

#[tokio::test]
async fn test_deduplication_different_request_ids() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(HttpDeduplicationConfig {
            ttl: std::time::Duration::from_secs(60),
            max_entries: 1000,
        })
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", get(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    // Request 1
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", "request-001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    // Request 2 with different ID - should execute and increment
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", "request-002")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body2, "Count: 2");

    // Request 3 with different ID - should execute and increment
    let response3 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", "request-003")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body3 = axum::body::to_bytes(response3.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body3, "Count: 3");

    // Verify all requests executed
    assert_eq!(state.get(), 3);
}

#[tokio::test]
async fn test_deduplication_ttl_expiration2() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(
            HttpDeduplicationConfig::default()
                .with_ttl(std::time::Duration::from_millis(100))
                .with_max_entries(1000),
        )
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", get(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    let request_id = "test-ttl-001";

    // First request
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    // Wait for TTL to expire
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Second request with same ID after TTL - should execute again
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body2, "Count: 2"); // New execution

    // Verify counter incremented twice
    assert_eq!(state.get(), 2);
}

#[tokio::test]
async fn test_deduplication_without_request_id() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(
            HttpDeduplicationConfig::default()
                .with_ttl(std::time::Duration::from_secs(60))
                .with_max_entries(1000),
        )
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", get(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    // Requests without x-request-id should each execute
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

    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body2, "Count: 2");

    // Verify both requests executed
    assert_eq!(state.get(), 2);
}

#[tokio::test]
async fn test_deduplication_with_post_requests() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(
            HttpDeduplicationConfig::default()
                .with_ttl(std::time::Duration::from_secs(60))
                .with_max_entries(1000),
        )
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", post(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    let request_id = "test-post-001";

    // First POST request
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);
    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    // Second POST request with same ID - should return 409 Conflict
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    assert!(response2.headers().get("x-duplicate-request").is_some());

    // Verify counter only incremented once
    assert_eq!(state.get(), 1);
}

#[tokio::test]
async fn test_deduplication_different_paths_same_id() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(
            HttpDeduplicationConfig::default()
                .with_ttl(std::time::Duration::from_secs(60))
                .with_max_entries(1000),
        )
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", get(counter_handler))
        .route("/other", get(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    let request_id = "test-path-001";

    // Request to /counter
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    // Request to /other with same ID - should return 409 Conflict
    // since deduplication uses request-id only (not path-specific)
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/other")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CONFLICT);
    assert!(response2.headers().get("x-duplicate-request").is_some());
    assert_eq!(state.get(), 1); // Counter only incremented once
}

#[tokio::test]
async fn test_deduplication_disabled() {
    let state = AppState::new();

    // Deduplication disabled by default (config.http.deduplication = None)
    let config = Config::default().with_excluded_middlewares(vec![HttpMiddleware::RateLimiting]);
    let app = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", get(counter_handler))
        .setup_deduplication() // Should be no-op when disabled
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    let request_id = "test-disabled-001";

    // First request
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body1, "Count: 1");

    // Second request with same ID - should execute again (no deduplication)
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/counter")
                .header("x-request-id", request_id)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body2, "Count: 2"); // New execution

    // Verify counter incremented twice
    assert_eq!(state.get(), 2);
}

#[tokio::test]
async fn test_deduplication_concurrent_requests() {
    let state = AppState::new();

    let config = Config::default()
        .with_deduplication_config(
            HttpDeduplicationConfig::default()
                .with_ttl(std::time::Duration::from_secs(60))
                .with_max_entries(1000),
        )
        .with_included_middlewares(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::RequestDeduplication,
        ]);
    let router = FluentRouter::<AppState>::with_state(config, state.clone())
        .unwrap()
        .route("/counter", get(counter_handler))
        .setup_deduplication()
        .setup_request_id()
        .into_inner()
        .with_state(state.clone());

    // Simulate concurrent requests with the same request ID
    let request_id = "test-concurrent-001";

    let app1 = router.clone();
    let app2 = router.clone();

    let req1 = Request::builder()
        .uri("/counter")
        .header("x-request-id", request_id)
        .body(Body::empty())
        .unwrap();

    let req2 = Request::builder()
        .uri("/counter")
        .header("x-request-id", request_id)
        .body(Body::empty())
        .unwrap();

    // Execute both requests concurrently
    let (response1, response2) = tokio::join!(app1.oneshot(req1), app2.oneshot(req2));

    let response1 = response1.unwrap();
    let response2 = response2.unwrap();

    // One should succeed, one should get 409 Conflict (or both might succeed due to race)
    let status1 = response1.status();
    let status2 = response2.status();

    // At least one should be marked as duplicate
    let conflicts = [status1, status2]
        .iter()
        .filter(|&&s| s == StatusCode::CONFLICT)
        .count();

    assert!(
        conflicts >= 1 || state.get() == 1,
        "Expected at least one 409 Conflict or counter == 1, got statuses: {:?}, {:?}, counter: {}",
        status1,
        status2,
        state.get()
    );

    // Counter should only increment once (or twice if race condition exists)
    let final_count = state.get();
    assert!(
        final_count <= 2,
        "Counter should be 1 or 2, got {}",
        final_count
    );
}
