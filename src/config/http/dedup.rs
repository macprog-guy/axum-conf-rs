use serde::Deserialize;
use std::time::Duration;

/// Configuration for request deduplication using a custom middleware.
///
/// When enabled, the middleware uses the `x-request-id` header as an idempotency key.
/// If a request with the same ID arrives while another is being processed, the duplicate
/// will wait and receive the same cached response. If a request arrives after completion
/// but within the TTL window, it receives the cached response immediately.
///
/// This prevents duplicate processing of requests due to client retries, network issues,
/// or other race conditions, while ensuring consistent responses.
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// [http.deduplication]
/// enabled = true
/// ttl = "5m"              # Keep cached responses for 5 minutes
/// max_entries = 10000     # Not used by axum-idempotent (for future compatibility)
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct HttpDeduplicationConfig {
    /// Time-to-live for completed request IDs in the cache.
    /// After a request completes, its ID remains in the cache for this duration
    /// to reject late duplicates. Longer TTLs provide better protection but use
    /// more memory. Default: 60 seconds
    #[serde(
        default = "HttpDeduplicationConfig::default_ttl",
        with = "humantime_serde"
    )]
    pub ttl: Duration,

    /// Maximum number of request IDs to keep in the cache.
    /// When the cache reaches this size, older entries are evicted using LRU.
    /// Default: 10000
    #[serde(default = "HttpDeduplicationConfig::default_max_entries")]
    pub max_entries: usize,
}

impl HttpDeduplicationConfig {
    fn default_ttl() -> Duration {
        Duration::from_secs(60)
    }

    fn default_max_entries() -> usize {
        10000
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }
    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }
}

impl Default for HttpDeduplicationConfig {
    fn default() -> Self {
        Self {
            ttl: Self::default_ttl(),
            max_entries: Self::default_max_entries(),
        }
    }
}
