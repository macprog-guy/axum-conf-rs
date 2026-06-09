//! Custom request deduplication middleware that doesn't require sessions.
//!
//! This module provides a session-free alternative to axum-idempotent for API usage.
//! It tracks in-flight requests and returns 409 Conflict for duplicate requests
//! within the TTL window.

use axum::{
    extract::Request,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use http::StatusCode;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tower::{Layer, Service};

/// Entry tracking an in-flight or recently completed request
#[derive(Clone)]
struct RequestEntry {
    expires_at: Instant,
    /// Monotonic insertion sequence, used to detect stale entries in the
    /// insertion-order index (an entry re-inserted after expiry gets a new seq).
    seq: u64,
}

/// In-memory tracker for request IDs.
///
/// `tracker` (a `DashMap`) gives O(1) concurrent duplicate lookups. `order` is
/// an insertion-ordered index used purely to evict the oldest entry in O(1)
/// amortized time when at capacity — replacing a previous O(n) scan over the
/// whole map under shard locks. Because the TTL is constant, insertion order is
/// also expiry order, so the front of `order` is always the oldest entry.
///
/// The `max_entries` capacity is **approximate**: the size check and the eviction
/// touch the `tracker` and `order` indices under separate locks, so under heavy
/// concurrency the live count can transiently exceed `max_entries` before the
/// over-capacity inserts evict. This bounded overshoot is acceptable for a
/// best-effort replay cache and avoids serializing every request behind one lock.
#[derive(Clone)]
pub(crate) struct RequestTracker {
    tracker: Arc<DashMap<String, RequestEntry>>,
    order: Arc<Mutex<VecDeque<(u64, String)>>>,
    seq: Arc<AtomicU64>,
    ttl: Duration,
    max_entries: usize,
}

impl RequestTracker {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            tracker: Arc::new(DashMap::new()),
            order: Arc::new(Mutex::new(VecDeque::new())),
            seq: Arc::new(AtomicU64::new(0)),
            ttl,
            max_entries,
        }
    }

    /// Atomically check if a request is duplicate and mark it if not.
    /// Returns true if the request is a duplicate (already exists and not expired).
    /// Returns false if this is a new request (and marks it).
    ///
    /// Takes the key by reference so the common duplicate (replay) path performs
    /// only a borrowed lookup; an owned key is allocated solely when inserting a
    /// new request below.
    fn check_and_mark(&self, key: &str) -> bool {
        let now = Instant::now();

        // Try to get existing entry
        if let Some(entry) = self.tracker.get(key) {
            if now < entry.expires_at {
                // Entry exists and hasn't expired - this is a duplicate
                return true;
            }
            // Entry expired, drop the reference so we can remove it. The stale
            // `order` record is skipped lazily during eviction.
            drop(entry);
            self.tracker.remove(key);
        }

        // Not a duplicate - need to mark this request.
        // Evict to stay under max_entries when at capacity.
        if self.tracker.len() >= self.max_entries {
            // First drop all expired entries (cheap retain).
            self.cleanup_expired();
            // If still at capacity, evict the single oldest live entry.
            if self.tracker.len() >= self.max_entries {
                self.evict_oldest();
            }
        }

        // Insert the new entry and record its insertion order. Ownership is
        // needed by both the tracker map and the order index, so allocate here
        // (only on the new-request path).
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.tracker.insert(
            key.to_string(),
            RequestEntry {
                expires_at: now + self.ttl,
                seq,
            },
        );
        self.order_lock().push_back((seq, key.to_string()));

        // Not a duplicate
        false
    }

    /// Evicts the oldest live entry by walking the insertion-order index from
    /// the front, discarding stale records (already removed or re-inserted with
    /// a newer seq) until one live entry is removed.
    fn evict_oldest(&self) {
        let mut order = self.order_lock();
        while let Some((seq, key)) = order.pop_front() {
            let is_current = self
                .tracker
                .get(&key)
                .map(|e| e.seq == seq)
                .unwrap_or(false);
            if is_current {
                self.tracker.remove(&key);
                return;
            }
            // Otherwise the record is stale; keep popping.
        }
    }

    fn cleanup_expired(&self) {
        let now = Instant::now();
        self.tracker.retain(|_, v| now < v.expires_at);
    }

    fn order_lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(u64, String)>> {
        self.order.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Layer that applies the deduplication middleware.
///
/// Low-level Tower type wired internally by `setup_middleware`; hidden from the
/// rendered public API. Configure deduplication via `[http.deduplication]` in TOML.
#[doc(hidden)]
#[derive(Clone)]
pub struct DeduplicationLayer {
    tracker: RequestTracker,
    header_name: String,
}

impl DeduplicationLayer {
    /// Create a new deduplication layer with the given TTL and maximum entries
    ///
    /// # Arguments
    ///
    /// * `ttl` - How long to track request IDs
    /// * `max_entries` - Maximum number of request IDs to track
    /// * `header_name` - The header containing the request ID (e.g., "x-request-id")
    pub fn new(ttl: Duration, max_entries: usize, header_name: impl Into<String>) -> Self {
        Self {
            tracker: RequestTracker::new(ttl, max_entries),
            header_name: header_name.into(),
        }
    }

    /// Get a clone of the tracker for cleanup tasks
    pub(crate) fn tracker(&self) -> RequestTracker {
        self.tracker.clone()
    }
}

impl<S> Layer<S> for DeduplicationLayer {
    type Service = DeduplicationService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DeduplicationService {
            inner,
            tracker: self.tracker.clone(),
            header_name: self.header_name.clone(),
        }
    }
}

/// Service that handles request deduplication.
///
/// Low-level Tower type produced by [`DeduplicationLayer`]; hidden from the
/// rendered public API.
#[doc(hidden)]
#[derive(Clone)]
pub struct DeduplicationService<S> {
    inner: S,
    tracker: RequestTracker,
    header_name: String,
}

impl<S> Service<Request> for DeduplicationService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        // Extract the request ID from headers
        let request_id = req
            .headers()
            .get(&self.header_name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let tracker = self.tracker.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // If no request ID, just pass through
            let Some(request_id) = request_id else {
                return inner.call(req).await;
            };

            // Atomically check if duplicate and mark if not
            if tracker.check_and_mark(&request_id) {
                tracing::warn!(
                    request_id = %request_id,
                    "Duplicate request detected, returning 409 Conflict"
                );

                // Return 409 Conflict for duplicate request. Built via
                // `IntoResponse` (no fallible builder/unwrap); the header is static.
                return Ok((
                    StatusCode::CONFLICT,
                    [("x-duplicate-request", "true")],
                    "Duplicate request detected",
                )
                    .into_response());
            }

            tracing::debug!(
                request_id = %request_id,
                "Processing new request"
            );

            // Process the request
            inner.call(req).await
        })
    }
}

/// Background task to periodically clean up expired request entries
pub(crate) async fn cleanup_task(tracker: RequestTracker, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        tracker.cleanup_expired();
        tracing::debug!("Cleaned up expired deduplication tracker entries");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deduplication_layer_new() {
        let ttl = Duration::from_secs(60);
        let layer = DeduplicationLayer::new(ttl, 1000, "x-request-id");

        assert_eq!(layer.header_name, "x-request-id");
    }

    #[test]
    fn test_deduplication_layer_new_with_string() {
        let ttl = Duration::from_secs(120);
        let layer = DeduplicationLayer::new(ttl, 500, "custom-header".to_string());

        assert_eq!(layer.header_name, "custom-header");
    }

    #[test]
    fn test_request_tracker_new() {
        let ttl = Duration::from_secs(30);
        let max_entries = 100;
        let tracker = RequestTracker::new(ttl, max_entries);

        assert_eq!(tracker.ttl, ttl);
        assert_eq!(tracker.max_entries, max_entries);
        assert_eq!(tracker.tracker.len(), 0);
    }

    #[test]
    fn test_request_tracker_check_and_mark() {
        let tracker = RequestTracker::new(Duration::from_secs(60), 1000);

        // First request - should not be duplicate (returns false, marks it)
        assert!(!tracker.check_and_mark("test-key"));

        // Second request with same key - should be duplicate (returns true)
        assert!(tracker.check_and_mark("test-key"));
    }

    #[test]
    fn test_request_tracker_nonexistent() {
        let tracker = RequestTracker::new(Duration::from_secs(60), 1000);

        // Non-existent key should not be duplicate (and gets marked)
        assert!(!tracker.check_and_mark("nonexistent-key"));

        // Now it should be a duplicate
        assert!(tracker.check_and_mark("nonexistent-key"));
    }

    #[tokio::test]
    async fn test_request_tracker_expiration() {
        let tracker = RequestTracker::new(Duration::from_millis(50), 1000);

        // First check marks it
        assert!(!tracker.check_and_mark("expiring-key"));

        // Should be duplicate immediately
        assert!(tracker.check_and_mark("expiring-key"));

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should not be duplicate anymore (expired, and gets marked again)
        assert!(!tracker.check_and_mark("expiring-key"));
    }

    #[tokio::test]
    async fn test_request_tracker_cleanup_expired() {
        let tracker = RequestTracker::new(Duration::from_millis(50), 1000);

        // Mark multiple entries
        tracker.check_and_mark("key1");
        tracker.check_and_mark("key2");
        tracker.check_and_mark("key3");

        assert_eq!(tracker.tracker.len(), 3);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Clean up expired entries
        tracker.cleanup_expired();

        assert_eq!(tracker.tracker.len(), 0);
    }

    #[test]
    fn test_deduplication_layer_tracker_access() {
        let ttl = Duration::from_secs(60);
        let layer = DeduplicationLayer::new(ttl, 1000, "x-request-id");

        // Get tracker reference
        let tracker = layer.tracker();

        // Mark a request
        assert!(!tracker.check_and_mark("test"));

        // Should be accessible through the layer's tracker (now it's a duplicate)
        let layer_tracker = layer.tracker();
        assert!(layer_tracker.check_and_mark("test"));
    }

    #[test]
    fn test_request_tracker_max_entries_eviction() {
        let tracker = RequestTracker::new(Duration::from_secs(60), 3);

        // Mark 3 entries (at max capacity)
        assert!(!tracker.check_and_mark("key1"));
        assert!(!tracker.check_and_mark("key2"));
        assert!(!tracker.check_and_mark("key3"));

        assert_eq!(tracker.tracker.len(), 3);

        // Mark 4th entry - should trigger eviction
        assert!(!tracker.check_and_mark("key4"));

        // Should still be at max capacity
        assert_eq!(tracker.tracker.len(), 3);

        // The newest entry should exist (is now a duplicate)
        assert!(tracker.check_and_mark("key4"));

        // key1 should have been evicted (oldest entry)
        // We can verify by checking the tracker directly
        assert!(!tracker.tracker.contains_key("key1"));

        // key2 and key3 should still exist
        assert!(tracker.tracker.contains_key("key2"));
        assert!(tracker.tracker.contains_key("key3"));
    }
}
