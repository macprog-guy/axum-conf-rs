//! FluentRouter and middleware configuration.
//!
//! This module provides the main router builder and all middleware setup methods.
//! The functionality is split across submodules for maintainability:
//!
//! - [`router`] - Core `FluentRouter` struct and initialization
//! - [`auth`] - Authentication (OIDC, Basic Auth, user span)
//! - [`observability`] - Logging, metrics, OpenTelemetry
//! - [`request`] - Request handling (payload, concurrency, dedup, request ID)
//! - [`features`] - Features (routing, compression, CORS, Helmet, sessions, health)
//! - [`control`] - Traffic control (rate limiting, panic catching)
//! - [`builder`] - Orchestration (setup_middleware, start, router delegation)

// Internal submodules (not part of the old public API, stay private)
#[cfg(feature = "deduplication")]
mod dedup;
#[cfg(feature = "basic-auth")]
mod basic_auth;
mod user_span;

// New submodules containing split implementation
mod router;
mod auth;
mod observability;
mod request;
mod features;
mod control;
mod builder;

// Re-export dedup types for backward compatibility
#[cfg(feature = "deduplication")]
pub use dedup::*;

// Re-export FluentRouter - the main public type
pub use router::FluentRouter;

#[cfg(test)]
mod tests;
