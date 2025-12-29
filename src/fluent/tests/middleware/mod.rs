//! Middleware-specific tests for FluentRouter
//!
//! Tests are organized by middleware type in separate modules.
//! The `interactions` module tests middleware working together.

// Feature-gated middleware tests
#[cfg(feature = "api-versioning")]
mod api_versioning;
#[cfg(feature = "concurrency-limit")]
mod concurrency_limit;
#[cfg(feature = "cors")]
mod cors;
#[cfg(feature = "deduplication")]
mod deduplication;
#[cfg(feature = "security-headers")]
mod helmet;
#[cfg(feature = "payload-limit")]
mod max_payload_size;
#[cfg(feature = "opentelemetry")]
mod opentelemetry;
#[cfg(feature = "path-normalization")]
mod path_normalization;
#[cfg(feature = "sensitive-headers")]
mod sensitive_headers;
#[cfg(feature = "session")]
mod sessions;

// Always-available middleware tests (no feature dependencies)
mod catch_panic;
mod config;
mod interactions;
mod liveness_readiness;
mod request_id;
mod static_files;
mod timeout;
