//! Application State Example
//!
//! Demonstrates how to use shared application state with axum-conf.
//!
//! Run with:
//! ```bash
//! RUST_ENV=dev cargo run --example with_state
//! ```
//!
//! Then test:
//! ```bash
//! curl http://localhost:3000/counter
//! curl -X POST http://localhost:3000/counter
//! curl http://localhost:3000/counter
//! ```

use axum::{
    extract::State,
    routing::get,
    Json,
};
use axum_conf::{Config, FluentRouter, Result};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Shared application state
#[derive(Default)]
struct AppState {
    /// A thread-safe counter
    counter: AtomicU64,
    /// Application name from config
    app_name: String,
}

#[derive(Serialize)]
struct CounterResponse {
    app: String,
    count: u64,
}

/// Get the current counter value
async fn get_counter(State(state): State<Arc<AppState>>) -> Json<CounterResponse> {
    Json(CounterResponse {
        app: state.app_name.clone(),
        count: state.counter.load(Ordering::Relaxed),
    })
}

/// Increment the counter
async fn increment_counter(State(state): State<Arc<AppState>>) -> Json<CounterResponse> {
    let new_count = state.counter.fetch_add(1, Ordering::Relaxed) + 1;
    Json(CounterResponse {
        app: state.app_name.clone(),
        count: new_count,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let config: Config = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1MiB"

[logging]
format = "default"
"#
    .parse()?;

    config.setup_tracing();

    // Create shared state
    let state = Arc::new(AppState {
        counter: AtomicU64::new(0),
        app_name: "Counter Service".into(),
    });

    println!("Starting counter service on http://127.0.0.1:3000");

    FluentRouter::<Arc<AppState>>::with_state(config, state)?
        .route("/counter", get(get_counter).post(increment_counter))
        .setup_middleware()
        .await?
        .start()
        .await
}
