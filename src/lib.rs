//! # axum-conf
//!
//! A batteries-included library for building production-ready web services with Axum,
//! designed specifically for Kubernetes deployments.
//!
#![cfg_attr(docsrs, feature(doc_cfg))]
//! Get health probes, metrics, security headers, rate limiting, and more — all
//! configured through simple TOML.
//!
//! **📖 For detailed guides, see the [documentation](https://github.com/macprog-guy/axum-conf-rs/tree/main/docs):**
//! - [Getting Started](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/getting-started.md) - Build your first service
//! - [Architecture](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/architecture.md) - How axum-conf works
//! - [Configuration Reference](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/configuration/toml-reference.md) - All options
//! - [Troubleshooting](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/troubleshooting.md) - Common issues
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
//!     let config: Config = Config::default();  // Loads from config/{RUST_ENV}.toml
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
//! | Readiness hook | App-supplied `/ready` checks via [`FluentRouter::with_readiness_check`] | Available |
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
//! | `keycloak` | OIDC/JWT authentication with auth code flow (enables `session`) |
//! | `session` | Cookie-based session management |
//! | `opentelemetry` | Distributed tracing with OTLP export |
//! | `basic-auth` | HTTP Basic Auth and API key authentication |
//! | `rustls` | TLS support |
//!
//! # Authentication
//!
//! All authentication methods produce an [`AuthenticatedIdentity`] available as an Axum extractor:
//!
//! ```rust,ignore
//! use axum_conf::AuthenticatedIdentity;
//!
//! // Required — returns 401 if not authenticated
//! async fn protected(identity: AuthenticatedIdentity) -> String {
//!     format!("Hello, {}!", identity.user)
//! }
//!
//! // Optional — returns None if not authenticated
//! async fn public(identity: Option<AuthenticatedIdentity>) -> String {
//!     match identity {
//!         Some(id) => format!("Hello, {}!", id.user),
//!         None => "Hello, anonymous!".to_string(),
//!     }
//! }
//! ```
//!
//! Supported methods: OIDC (`keycloak` feature), HTTP Basic Auth (`basic-auth` feature),
//! and Proxy OIDC (no feature flag, configured via `[http.proxy_oidc]`).
//!
//! # Module Organization
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`config`] | Configuration loading and validation ([`Config`]) |
//! | [`fluent`] | Router builder and middleware setup ([`FluentRouter`]) |
//! | `error` | Error types and handling ([`Error`]) |
//! | `utils` | Utilities ([`ApiVersion`], [`Sensitive`]) |
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
//! let config: Config = Config::default();  // Uses RUST_ENV
//!
//! // From string (useful for tests)
//! let config: Config = r#"
//!     [http]
//!     bind_port = 3000
//!     max_payload_size_bytes = "1KiB"
//! "#.parse().unwrap();
//!
//! // With builder methods
//! let config: Config = Config::default()
//!     .with_bind_port(8080)
//!     .with_compression(true)
//!     .with_request_timeout(Duration::from_secs(30));
//! ```
//!
//! See [Configuration Reference](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/configuration/toml-reference.md) for all options.
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
//!     let config: Config = Config::default();
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
//! ## Application Readiness
//!
//! Make `/ready` reflect application state — not just database connectivity — by
//! registering a check with [`FluentRouter::with_readiness_check`]. The check is
//! composed with the built-in database / circuit-breaker checks: the endpoint is
//! ready iff the application check returns [`Readiness::Ready`] *and* the built-in
//! check passes. Returning [`Readiness::not_ready`] yields `503 Service
//! Unavailable` with the message in the body — useful for shedding load when a
//! bounded worker pool is saturated, so a load balancer stops routing to the
//! instance.
//!
//! ```rust,no_run
//! use axum::routing::post;
//! use axum_conf::{Config, FluentRouter, Readiness, Result};
//! use std::sync::Arc;
//! use tokio::sync::Semaphore;
//!
//! #[derive(Clone)]
//! struct AppState {
//!     permits: Arc<Semaphore>,
//! }
//!
//! # async fn convert() {}
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let config: Config = Config::default();
//!     let state = AppState { permits: Arc::new(Semaphore::new(8)) };
//!
//!     FluentRouter::<AppState>::with_state(config, state)?
//!         .route("/convert", post(convert))
//!         .with_readiness_check(|s: AppState| async move {
//!             if s.permits.available_permits() == 0 {
//!                 Readiness::not_ready("all conversion permits held")
//!             } else {
//!                 Readiness::ready()
//!             }
//!         })
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
//! See [PostgreSQL Guide](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/features/postgres.md) for details.
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
//! See [Middleware Overview](https://github.com/macprog-guy/axum-conf-rs/blob/main/docs/middleware/overview.md) for the full stack.
#![warn(missing_docs)]
// Hold library code to explicit error handling: a panic via unwrap/expect must be
// a deliberate, justified `#[allow(...)]`. Unit tests are exempted via clippy.toml
// (allow-unwrap-in-tests / allow-expect-in-tests); examples and integration tests
// are separate crates and unaffected.
#![deny(clippy::unwrap_used, clippy::expect_used)]
pub mod config;
mod error;
pub mod fluent;
pub mod resilience;
mod utils;

#[cfg(feature = "circuit-breaker")]
#[cfg_attr(docsrs, doc(cfg(feature = "circuit-breaker")))]
pub mod circuit_breaker;

#[cfg(feature = "openapi")]
#[cfg_attr(docsrs, doc(cfg(feature = "openapi")))]
pub mod openapi;

pub use config::*;
pub use error::*;
pub use fluent::*;
pub use utils::*;

/// Convenience alias for results returned by this crate, fixing the error type
/// to [`Error`]. Use it as `axum_conf::Result<T>`.
pub type Result<T> = std::result::Result<T, Error>;
