# Testing Guide

This guide covers how to write tests for applications built with axum-conf.

## Test Organization

| Location | Type | Method | Speed |
|----------|------|--------|-------|
| `src/**/tests.rs` | Unit tests | `oneshot()` | Fast |
| `tests/` | Integration tests | Real TCP + Docker | Slow |

## Unit Testing with oneshot()

For fast unit tests, use Axum's `oneshot()` method to test handlers without starting a real server:

```rust
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use axum_conf::{Config, FluentRouter};
use tower::ServiceExt;

async fn hello() -> &'static str {
    "Hello, World!"
}

#[tokio::test]
async fn test_hello_handler() {
    // Create a minimal config for testing
    let config_str = r#"
[http]
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#;
    let config: Config = config_str.parse().unwrap();

    // Build router with handler
    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/hello", get(hello))
        .into_router();

    // Test using oneshot (no real server needed)
    let response = router
        .oneshot(
            Request::builder()
                .uri("/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

## Testing with Application State

When your handlers need application state:

```rust
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::get,
};
use axum_conf::{Config, FluentRouter};
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    message: String,
}

async fn greeting(State(state): State<Arc<AppState>>) -> String {
    state.message.clone()
}

#[tokio::test]
async fn test_with_state() {
    let config_str = r#"
[http]
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#;
    let config: Config = config_str.parse().unwrap();
    let state = Arc::new(AppState {
        message: "Test greeting".into(),
    });

    let router = FluentRouter::with_state(config, state)
        .unwrap()
        .route("/greeting", get(greeting))
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/greeting")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

## Testing JSON Handlers

For handlers that return JSON:

```rust
use axum::{
    Json,
    body::Body,
    http::{Request, StatusCode, header},
    routing::post,
};
use axum_conf::{Config, FluentRouter};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

#[derive(Serialize, Deserialize)]
struct CreateUser {
    name: String,
}

#[derive(Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

async fn create_user(Json(payload): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: payload.name,
    })
}

#[tokio::test]
async fn test_json_handler() {
    let config_str = r#"
[http]
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#;
    let config: Config = config_str.parse().unwrap();

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/users", post(create_user))
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/users")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name": "Alice"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Parse response body
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let user: User = serde_json::from_slice(&body).unwrap();
    assert_eq!(user.name, "Alice");
}
```

## Testing Health Endpoints

Test the built-in health check endpoints:

```rust
#[tokio::test]
async fn test_liveness_probe() {
    let config_str = r#"
[http]
bind_port = 3000
max_payload_size_bytes = "1KiB"
"#;
    let config: Config = config_str.parse().unwrap();

    let router = FluentRouter::without_state(config)
        .unwrap()
        .setup_middleware()
        .await
        .unwrap()
        .into_router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

## Integration Testing

For full integration tests that test the complete server behavior:

```rust
// tests/integration_test.rs
use axum_conf::{Config, FluentRouter};
use reqwest::Client;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_full_server() {
    let config_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0  # Use port 0 for dynamic port assignment
max_payload_size_bytes = "1KiB"
"#;
    let config: Config = config_str.parse().unwrap();

    // Bind to a random available port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Spawn server in background
    tokio::spawn(async move {
        let router = FluentRouter::without_state(config)
            .unwrap()
            .setup_middleware()
            .await
            .unwrap();

        axum::serve(listener, router.into_router())
            .await
            .unwrap();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Make real HTTP requests
    let client = Client::new();
    let response = client
        .get(format!("http://{}/live", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}
```

## Testing with Database (testcontainers)

For tests requiring a real database:

```rust
// tests/database_test.rs
use axum_conf::{Config, FluentRouter};
use testcontainers::{GenericImage, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

#[tokio::test]
async fn test_with_postgres() {
    // Start PostgreSQL container
    let container = Postgres::default().start().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();

    let db_url = format!(
        "postgres://postgres:postgres@127.0.0.1:{}/postgres",
        port
    );

    let config_str = format!(r#"
[http]
bind_port = 3000
max_payload_size_bytes = "1KiB"

[database]
url = "{}"
max_pool_size = 5
"#, db_url);

    let config: Config = config_str.parse().unwrap();

    // Your database tests here...
}
```

## Feature-Gated Tests

When testing feature-specific functionality:

```rust
#[cfg(feature = "postgres")]
#[tokio::test]
async fn test_database_pool() {
    // Test database functionality
}

#[cfg(feature = "keycloak")]
#[tokio::test]
async fn test_oidc_authentication() {
    // Test OIDC functionality
}
```

## Configuration for Testing

Create test-specific configurations:

```rust
fn test_config() -> Config {
    r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_payload_size_bytes = "1KiB"
request_timeout = "5s"

[logging]
format = "compact"
"#
    .parse()
    .unwrap()
}
```

## Running Tests

```bash
# Run all tests (requires Docker for testcontainers)
cargo test --all-features

# Run unit tests only (fast, no Docker)
cargo test --all-features --lib

# Run integration tests only
cargo test --all-features --test '*'

# Run a specific test
cargo test --all-features test_hello_handler

# Run tests with output
cargo test --all-features -- --nocapture
```

## Best Practices

1. **Use `oneshot()` for unit tests** - Fast and doesn't require real network connections
2. **Use random ports** - Set `bind_port = 0` for integration tests to avoid port conflicts
3. **Clean up resources** - Use testcontainers for databases to ensure clean state
4. **Test error cases** - Verify proper error responses for invalid inputs
5. **Test middleware** - Ensure rate limiting, CORS, etc. work as expected
6. **Use feature flags** - Gate feature-specific tests with `#[cfg(feature = "...")]`
