//! Hello World Example
//!
//! The simplest possible axum-conf application.
//!
//! Run with:
//! ```bash
//! RUST_ENV=dev cargo run --example hello_world
//! ```
//!
//! Then test:
//! ```bash
//! curl http://localhost:3000/
//! curl http://localhost:3000/live
//! curl http://localhost:3000/ready
//! ```

use axum::{Json, routing::get};
use axum_conf::{Config, FluentRouter, Result};
use serde::Serialize;

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

async fn hello() -> Json<Message> {
    Json(Message {
        message: "Hello, World!",
    })
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "healthy",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from inline string
    // In production, use Config::default() to load from config/{RUST_ENV}.toml
    let config: Config = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1MiB"
request_timeout = "30s"

[logging]
format = "default"
"#
    .parse()?;

    // Setup logging based on config
    config.setup_tracing();

    println!("Starting server on http://127.0.0.1:3000");

    // Build and start the server
    FluentRouter::without_state(config)?
        .route("/", get(hello))
        .route("/health", get(health))
        .setup_middleware()
        .await?
        .start()
        .await
}
