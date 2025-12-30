# Getting Started

This guide walks you through building your first production-ready web service with axum-conf. By the end, you'll have a service with health checks, metrics, structured logging, and proper error handling.

## What You'll Learn

- Creating a new project with axum-conf
- Adding routes with JSON responses
- Using application state
- Configuring the service with TOML
- Testing your endpoints

## Prerequisites

- Rust 1.75 or later
- Basic familiarity with async Rust

## Step 1: Create a New Project

```bash
cargo new my-service
cd my-service
```

Add dependencies to `Cargo.toml`:

```toml
[package]
name = "my-service"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.8"
axum-conf = "0.3"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## Step 2: Create Configuration

Create the configuration directory and file:

```bash
mkdir config
```

Create `config/dev.toml`:

```toml
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1MiB"

[logging]
format = "pretty"
```

## Step 3: Write Your First Handler

Replace `src/main.rs` with:

```rust
use axum::{Json, Router, routing::get};
use axum_conf::{Config, FluentRouter, Result};
use serde::Serialize;

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

async fn hello() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/", get(hello))
        .route("/health", get(health))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Step 4: Run and Test

Start the server:

```bash
RUST_ENV=dev cargo run
```

You should see output like:

```
2024-01-15T10:30:00.000Z  INFO axum_conf: Starting axum-conf version 0.3.0...
2024-01-15T10:30:00.001Z  INFO axum_conf: Bound to 127.0.0.1:3000
2024-01-15T10:30:00.001Z  INFO axum_conf: Waiting for connections
2024-01-15T10:30:00.001Z  INFO axum_conf: Max req/s: 100
```

Test your endpoints:

```bash
# Your custom handler
curl http://localhost:3000/
# Output: Hello, World!

# JSON health endpoint
curl http://localhost:3000/health
# Output: {"status":"healthy","version":"0.1.0"}

# Built-in liveness probe
curl http://localhost:3000/live
# Output: OK

# Built-in readiness probe
curl http://localhost:3000/ready
# Output: OK

# Prometheus metrics
curl http://localhost:3000/metrics
# Output: # HELP axum_conf_http_requests_total...
```

## Step 5: Add Application State

Most applications need shared state. Here's how to add a request counter:

```rust
use axum::{Json, Router, extract::State, routing::get};
use axum_conf::{Config, FluentRouter, Result};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

// Define your application state
#[derive(Clone)]
struct AppState {
    request_count: Arc<AtomicU64>,
}

#[derive(Serialize)]
struct Stats {
    total_requests: u64,
}

async fn get_stats(State(state): State<AppState>) -> Json<Stats> {
    let count = state.request_count.fetch_add(1, Ordering::SeqCst);
    Json(Stats {
        total_requests: count + 1,
    })
}

async fn hello() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    let state = AppState {
        request_count: Arc::new(AtomicU64::new(0)),
    };

    FluentRouter::with_state(config, state)?
        .route("/", get(hello))
        .route("/stats", get(get_stats))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Test the counter:

```bash
curl http://localhost:3000/stats
# Output: {"total_requests":1}

curl http://localhost:3000/stats
# Output: {"total_requests":2}

curl http://localhost:3000/stats
# Output: {"total_requests":3}
```

## Step 6: Add POST Handlers with JSON Bodies

```rust
use axum::{Json, Router, extract::State, routing::{get, post}};
use axum_conf::{Config, FluentRouter, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
struct AppState {
    items: Arc<RwLock<Vec<Item>>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Item {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct CreateItem {
    name: String,
}

async fn list_items(State(state): State<AppState>) -> Json<Vec<Item>> {
    let items = state.items.read().unwrap();
    Json(items.clone())
}

async fn create_item(
    State(state): State<AppState>,
    Json(input): Json<CreateItem>,
) -> Json<Item> {
    let mut items = state.items.write().unwrap();
    let item = Item {
        id: items.len() as u64 + 1,
        name: input.name,
    };
    items.push(item.clone());
    Json(item)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    let state = AppState {
        items: Arc::new(RwLock::new(vec![])),
    };

    FluentRouter::with_state(config, state)?
        .route("/items", get(list_items).post(create_item))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Test CRUD operations:

```bash
# List items (empty)
curl http://localhost:3000/items
# Output: []

# Create an item
curl -X POST http://localhost:3000/items \
  -H "Content-Type: application/json" \
  -d '{"name": "First Item"}'
# Output: {"id":1,"name":"First Item"}

# Create another
curl -X POST http://localhost:3000/items \
  -H "Content-Type: application/json" \
  -d '{"name": "Second Item"}'
# Output: {"id":2,"name":"Second Item"}

# List all items
curl http://localhost:3000/items
# Output: [{"id":1,"name":"First Item"},{"id":2,"name":"Second Item"}]
```

## Step 7: Production Configuration

Create `config/prod.toml` for production:

```toml
[http]
bind_addr = "0.0.0.0"
bind_port = 8080
max_payload_size_bytes = "32KiB"
request_timeout = "30s"
shutdown_timeout = "30s"
max_requests_per_sec = 1000
max_concurrent_requests = 4096

[logging]
format = "json"
```

Run in production mode:

```bash
RUST_ENV=prod cargo run --release
```

## Next Steps

Now that you have a working service, explore these topics:

- [Architecture](architecture.md) - Understand how axum-conf works
- [Configuration Reference](configuration/toml-reference.md) - All configuration options
- [PostgreSQL](features/postgres.md) - Add database support
- [Kubernetes Deployment](kubernetes/deployment.md) - Deploy to production
