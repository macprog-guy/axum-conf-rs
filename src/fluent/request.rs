//! Request handling middleware: payload limits, concurrency, deduplication, request ID, sensitive headers.

use super::router::FluentRouter;
use crate::HttpMiddleware;

use {
    crate::utils::RequestIdGenerator,
    http::HeaderName,
    tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer},
};

#[cfg(feature = "payload-limit")]
use {axum::extract::DefaultBodyLimit, tower_http::limit::RequestBodyLimitLayer};

#[cfg(feature = "concurrency-limit")]
use tower::limit::ConcurrencyLimitLayer;

#[cfg(feature = "sensitive-headers")]
use {http::header::AUTHORIZATION, std::iter::once, tower_http::sensitive_headers::SetSensitiveHeadersLayer};

#[cfg(feature = "deduplication")]
use {
    super::dedup::{self, DeduplicationLayer},
    std::time::Duration,
    tokio_util::task::AbortOnDropHandle,
};

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up maximum request payload size limits.
    ///
    /// Rejects requests with bodies larger than the configured limit with
    /// a `413 Payload Too Large` response.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// max_payload_size_bytes = "1MiB"  # Supports KiB, MiB, GiB
    /// ```
    ///
    /// # Default
    ///
    /// 32 KiB if not configured.
    #[cfg(feature = "payload-limit")]
    #[must_use]
    pub fn setup_max_payload_size(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::MaxPayloadSize) {
            return self;
        }

        self.inner =
            self.inner
                .layer(DefaultBodyLimit::disable())
                .layer(RequestBodyLimitLayer::new(
                    self.config.http.max_payload_size_bytes.as_u64() as usize,
                ));
        self
    }

    /// No-op when `payload-limit` feature is disabled.
    #[cfg(not(feature = "payload-limit"))]
    #[must_use]
    pub fn setup_max_payload_size(self) -> Self {
        self
    }

    /// Sets up concurrent request limits.
    ///
    /// Limits the number of requests being processed simultaneously. When the
    /// limit is reached, new requests receive a `503 Service Unavailable` response.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// max_concurrent_requests = 4096  # Default
    /// ```
    ///
    /// # Use Cases
    ///
    /// - Prevent resource exhaustion under heavy load
    /// - Maintain stable response times
    /// - Protect downstream services
    #[cfg(feature = "concurrency-limit")]
    #[must_use]
    pub fn setup_concurrency_limit(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::ConcurrencyLimit) {
            return self;
        }

        self.inner = self.inner.layer(ConcurrencyLimitLayer::new(
            self.config.http.max_concurrent_requests as usize,
        ));
        self
    }

    /// No-op when `concurrency-limit` feature is disabled.
    #[cfg(not(feature = "concurrency-limit"))]
    #[must_use]
    pub fn setup_concurrency_limit(self) -> Self {
        self
    }

    /// Sets up request ID generation and propagation.
    ///
    /// Adds two middleware layers:
    /// 1. Generates or preserves `x-request-id` headers
    /// 2. Propagates the request ID to response headers
    ///
    /// Request IDs are UUIDv7 values that enable:
    /// - Distributed tracing across services
    /// - Log correlation
    /// - Request debugging
    ///
    /// If a request already has an `x-request-id` header, it is preserved.
    #[must_use]
    pub fn setup_request_id(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::RequestId) {
            return self;
        }

        let x_request_id = HeaderName::from_static("x-request-id");
        self.inner = self
            .inner
            .layer(SetRequestIdLayer::new(
                x_request_id.clone(),
                RequestIdGenerator,
            ))
            .layer(PropagateRequestIdLayer::new(x_request_id));
        self
    }

    /// Sets up request deduplication middleware using axum-idempotent.
    ///
    /// When enabled in configuration, this prevents duplicate requests (identified
    /// by the same `x-request-id` header) from being processed simultaneously.
    /// Instead, duplicate requests receive the cached response from the original request.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.deduplication]
    /// enabled = true
    /// ttl = "5m"              # Keep responses cached for 5 minutes
    /// max_entries = 10000     # Maximum cache size (not used by axum-idempotent)
    /// ```
    ///
    /// # Behavior
    ///
    /// - If a request with the same ID is being processed, waits and returns the same response
    /// - If a request with the same ID completed within TTL, returns the cached response
    /// - Uses the `x-request-id` header as the idempotency key
    /// - After TTL expires, the same request ID triggers a new request
    ///
    /// # Performance
    ///
    /// Uses an in-memory cache with automatic expiration based on TTL.
    ///
    /// This middleware should be added **after** `setup_request_id()` to ensure
    /// all requests have an `x-request-id` header before deduplication checking.
    /// That's because axum handles layers from the outside in.
    ///
    /// # Implementation
    ///
    /// Uses a custom session-free implementation that caches responses in memory.
    /// Only successful responses (2xx, 3xx) are cached. Error responses are not cached
    /// to avoid caching transient errors.
    #[cfg(feature = "deduplication")]
    #[must_use]
    pub fn setup_deduplication(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::RequestDeduplication) {
            return self;
        }

        if !self.is_middleware_enabled(HttpMiddleware::RequestId) {
            eprintln!(
                "RequestId middleware must be enabled and added after deduplication because axum handles layers from the outside in."
            );
            return self;
        }

        if let Some(dedup_config) = &self.config.http.deduplication {
            let layer =
                DeduplicationLayer::new(dedup_config.ttl, dedup_config.max_entries, "x-request-id");

            // Spawn cleanup task to remove expired entries every minute
            let tracker = layer.tracker();
            let cleanup_interval = Duration::from_secs(60);
            let handle = tokio::spawn(dedup::cleanup_task(tracker, cleanup_interval));
            self.dedup_cleanup_handle = Some(AbortOnDropHandle::new(handle));

            self.inner = self.inner.layer(layer);
        }
        self
    }

    /// No-op when `deduplication` feature is disabled.
    #[cfg(not(feature = "deduplication"))]
    #[must_use]
    pub fn setup_deduplication(self) -> Self {
        self
    }

    /// Marks sensitive headers to prevent them from appearing in logs.
    ///
    /// Protects the `Authorization` header (and any other configured headers)
    /// from being logged by middleware, preventing credential leaks in logs.
    ///
    /// # Protected Headers
    ///
    /// - `Authorization` - Bearer tokens, Basic auth, etc.
    ///
    /// Additional headers can be protected by modifying this method.
    #[cfg(feature = "sensitive-headers")]
    #[must_use]
    pub fn setup_sensitive_headers(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::SensitiveHeaders) {
            return self;
        }

        self.inner = self
            .inner
            .layer(SetSensitiveHeadersLayer::new(once(AUTHORIZATION)));
        self
    }

    /// No-op when `sensitive-headers` feature is disabled.
    #[cfg(not(feature = "sensitive-headers"))]
    #[must_use]
    pub fn setup_sensitive_headers(self) -> Self {
        self
    }
}
