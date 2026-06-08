//! Application Readiness Hook Example
//!
//! Demonstrates how to make `/ready` reflect application state — not just
//! database connectivity — using `with_readiness_check`. Here a bounded worker
//! pool is modeled with a `Semaphore`; when all permits are held the service
//! reports `503` on `/ready` so a load balancer stops routing to a saturated
//! instance. The hook is composed with the built-in checks: the endpoint is
//! ready only when the application check passes *and* the built-in
//! database/circuit-breaker check passes.
//!
//! Run with:
//! ```bash
//! RUST_ENV=dev cargo run --example readiness_check
//! ```
//!
//! Then, in another terminal:
//! ```bash
//! # Ready while permits are available
//! curl -i http://127.0.0.1:3000/ready
//!
//! # Hold all permits by starting more work than the pool allows...
//! curl -X POST http://127.0.0.1:3000/work &
//! curl -X POST http://127.0.0.1:3000/work &
//!
//! # ...and /ready now reports 503 Service Unavailable
//! curl -i http://127.0.0.1:3000/ready
//! ```

use axum::{extract::State, routing::post};
use axum_conf::{Config, FluentRouter, Readiness, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Shared application state: a bounded pool of worker permits.
#[derive(Clone)]
struct AppState {
    permits: Arc<Semaphore>,
}

/// Simulates a CPU-bound unit of work that holds a permit while running.
async fn do_work(State(state): State<AppState>) -> &'static str {
    // Acquire a permit; if none are available the service is saturated and
    // sheds this request immediately.
    let Ok(_permit) = state.permits.clone().try_acquire_owned() else {
        return "busy\n";
    };

    // Hold the permit (kept alive across the await) to simulate work.
    tokio::time::sleep(Duration::from_secs(5)).await;
    "done\n"
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

    // Two permits: the pool is saturated once two requests are in flight.
    let state = AppState {
        permits: Arc::new(Semaphore::new(2)),
    };

    println!("Starting readiness demo on http://127.0.0.1:3000");

    FluentRouter::<AppState>::with_state(config, state)?
        .route("/work", post(do_work))
        .with_readiness_check(|s: AppState| async move {
            if s.permits.available_permits() == 0 {
                Readiness::not_ready("all worker permits held")
            } else {
                Readiness::ready()
            }
        })
        .setup_middleware()
        .await?
        .start()
        .await
}
