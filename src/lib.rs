//! # axum-conf
//!
//! A batteries-included library for building production-ready web services with Axum,
//! designed specifically for Kubernetes deployments.
//!
//! Get health probes, metrics, security headers, rate limiting, and more — all
//! configured through simple TOML.
//!
//! **📖 For detailed guides, see the [documentation](../docs/):**
//! - [Getting Started](../docs/getting-started.md) - Build your first service
//! - [Architecture](../docs/architecture.md) - How axum-conf works
//! - [Configuration Reference](../docs/configuration/toml-reference.md) - All options
//! - [Troubleshooting](../docs/troubleshooting.md) - Common issues
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use axum::{Router, routing::get};
//! use axum_conf::{Config, Result, FluentRouter};
//!
//! async fn hello() -> &'static str {
//!     "Hello, World!"
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let config = Config::default();  // Loads from config/{RUST_ENV}.toml
//!     config.setup_tracing();
//!
//!     FluentRouter::without_state(config)?
//!         .route("/", get(hello))
//!         .setup_middleware()
//!         .await?
//!         .start()
//!         .await
//! }
//! ```
//!
//! With `config/dev.toml`:
//! ```toml
//! [http]
//! bind_port = 3000
//! max_payload_size_bytes = "1MiB"
//! ```
//!
//! Run with `RUST_ENV=dev cargo run`.
//!
//! # What You Get
//!
//! | Feature | Description | Default |
//! |---------|-------------|---------|
//! | Health probes | `/live` and `/ready` endpoints | Enabled |
//! | Prometheus metrics | Request counts, latencies at `/metrics` | Enabled |
//! | Request logging | Structured logs with UUIDv7 correlation IDs | Enabled |
//! | Rate limiting | Per-IP request throttling | 100 req/sec |
//! | Security headers | X-Frame-Options, X-Content-Type-Options | Enabled |
//! | Panic recovery | Catches panics, returns 500, keeps running | Enabled |
//! | Graceful shutdown | Handles SIGTERM, drains connections | 30s timeout |
//! | Compression | gzip, brotli, deflate, zstd | Available |
//!
//! # Cargo Features
//!
//! | Feature | Description |
//! |---------|-------------|
//! | `postgres` | PostgreSQL connection pooling (enables `rustls`) |
//! | `keycloak` | OIDC/JWT authentication (enables `session`) |
//! | `session` | Cookie-based session management |
//! | `opentelemetry` | Distributed tracing with OTLP export |
//! | `basic-auth` | HTTP Basic Auth and API key authentication |
//! | `rustls` | TLS support |
//!
//! # Module Organization
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`config`] | Configuration loading and validation ([`Config`]) |
//! | [`fluent`] | Router builder and middleware setup ([`FluentRouter`]) |
//! | [`error`] | Error types and handling ([`Error`]) |
//! | [`utils`] | Utilities ([`ApiVersion`], [`Sensitive`]) |
//!
//! # Configuration
//!
//! Configuration can be loaded from TOML files, strings, or built programmatically:
//!
//! ```rust
//! use axum_conf::Config;
//! use std::time::Duration;
//!
//! // From file (recommended for production)
//! let config = Config::default();  // Uses RUST_ENV
//!
//! // From string (useful for tests)
//! let config: Config = r#"
//!     [http]
//!     bind_port = 3000
//!     max_payload_size_bytes = "1KiB"
//! "#.parse().unwrap();
//!
//! // With builder methods
//! let config = Config::default()
//!     .with_bind_port(8080)
//!     .with_compression(true)
//!     .with_request_timeout(Duration::from_secs(30));
//! ```
//!
//! See [Configuration Reference](../docs/configuration/toml-reference.md) for all options.
//!
//! # Error Handling
//!
//! The library uses a custom [`Result`] type. Errors convert to structured JSON responses:
//!
//! ```json
//! {
//!   "error_code": "DATABASE_ERROR",
//!   "message": "Database error: connection refused"
//! }
//! ```
//!
//! # Examples
//!
//! ## With Application State
//!
//! ```rust,no_run
//! use axum::{routing::get, extract::State};
//! use axum_conf::{Config, Result, FluentRouter};
//! use std::sync::Arc;
//! use std::sync::atomic::{AtomicU64, Ordering};
//!
//! #[derive(Clone)]
//! struct AppState {
//!     counter: Arc<AtomicU64>,
//! }
//!
//! async fn count(State(state): State<AppState>) -> String {
//!     let n = state.counter.fetch_add(1, Ordering::SeqCst);
//!     format!("Count: {}", n)
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let config = Config::default();
//!     let state = AppState { counter: Arc::new(AtomicU64::new(0)) };
//!
//!     FluentRouter::<AppState>::with_state(config, state)?
//!         .route("/count", get(count))
//!         .setup_middleware()
//!         .await?
//!         .start()
//!         .await
//! }
//! ```
//!
//! ## With PostgreSQL
//!
//! Requires the `postgres` feature.
//!
//! ```toml
//! [database]
//! url = "{{ DATABASE_URL }}"
//! max_pool_size = 10
//! ```
//!
//! ```rust,ignore
//! use sqlx::PgPool;
//! use axum::extract::State;
//!
//! async fn get_users(State(pool): State<PgPool>) -> String {
//!     let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
//!         .fetch_one(&pool)
//!         .await
//!         .unwrap();
//!     format!("Users: {}", count.0)
//! }
//! ```
//!
//! See [PostgreSQL Guide](../docs/features/postgres.md) for details.
//!
//! ## Middleware Control
//!
//! Enable or disable specific middleware:
//!
//! ```toml
//! [http.middleware]
//! exclude = ["rate-limiting", "compression"]
//! ```
//!
//! See [Middleware Overview](../docs/middleware/overview.md) for the full stack.
mod config;
mod error;
mod fluent;
mod utils;

#[cfg(feature = "circuit-breaker")]
pub mod circuit_breaker;

#[cfg(feature = "openapi")]
pub mod openapi;

pub use config::*;
pub use error::*;
pub use fluent::*;
pub use utils::*;

#[cfg(feature = "keycloak")]
pub type Role = String;

pub type Result<T> = std::result::Result<T, Error>;
