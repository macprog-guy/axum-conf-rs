//! Custom request deduplication middleware that doesn't require sessions.
//!
//! This module provides a session-free alternative to axum-idempotent for API usage.
//! It tracks in-flight requests and returns 409 Conflict for duplicate requests
//! within the TTL window.

use axum::{body::Body, extract::Request, response::Response};
use dashmap::DashMap;
use http::StatusCode;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tower::{Layer, Service};

/// Entry tracking an in-flight or recently completed request
#[derive(Clone)]
struct RequestEntry {
    expires_at: Instant,
}

/// In-memory tracker for request IDs using DashMap for concurrent access
#[derive(Clone)]
pub(crate) struct RequestTracker {
    tracker: Arc<DashMap<String, RequestEntry>>,
    ttl: Duration,
    max_entries: usize,
}

impl RequestTracker {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            tracker: Arc::new(DashMap::new()),
            ttl,
            max_entries,
        }
    }

    /// Atomically check if a request is duplicate and mark it if not.
    /// Returns true if the request is a duplicate (already exists and not expired).
    /// Returns false if this is a new request (and marks it).
    fn check_and_mark(&self, key: String) -> bool {
        let now = Instant::now();

        // Try to get existing entry
        if let Some(entry) = self.tracker.get(&key) {
            if now < entry.expires_at {
                // Entry exists and hasn't expired - this is a duplicate
                return true;
            }
            // Entry expired, drop the reference so we can remove it
            drop(entry);
            self.tracker.remove(&key);
        }

        // Not a duplicate - need to mark this request
        // Check if we need to evict entries to stay under max_entries
        if self.tracker.len() >= self.max_entries {
            // First try removing expired entries
            self.cleanup_expired();

            // If still at capacity, remove oldest entry
            if self.tracker.len() >= self.max_entries
                && let Some(oldest_key) = self
                    .tracker
                    .iter()
                    .min_by_key(|entry| entry.expires_at)
                    .map(|entry| entry.key().clone())
            {
                self.tracker.remove(&oldest_key);
            }
        }

        // Insert the new entry
        let entry = RequestEntry {
            expires_at: now + self.ttl,
        };
        self.tracker.insert(key, entry);

        // Not a duplicate
        false
    }

    fn cleanup_expired(&self) {
        let now = Instant::now();
        self.tracker.retain(|_, v| now < v.expires_at);
    }
}

/// Layer that applies the deduplication middleware
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

/// Service that handles request deduplication
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
            if tracker.check_and_mark(request_id.clone()) {
                tracing::warn!(
                    request_id = %request_id,
                    "Duplicate request detected, returning 409 Conflict"
                );

                // Return 409 Conflict for duplicate request
                return Ok(Response::builder()
                    .status(StatusCode::CONFLICT)
                    .header("x-duplicate-request", "true")
                    .body(Body::from("Duplicate request detected"))
                    .unwrap());
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
        assert!(!tracker.check_and_mark("test-key".to_string()));

        // Second request with same key - should be duplicate (returns true)
        assert!(tracker.check_and_mark("test-key".to_string()));
    }

    #[test]
    fn test_request_tracker_nonexistent() {
        let tracker = RequestTracker::new(Duration::from_secs(60), 1000);

        // Non-existent key should not be duplicate (and gets marked)
        assert!(!tracker.check_and_mark("nonexistent-key".to_string()));

        // Now it should be a duplicate
        assert!(tracker.check_and_mark("nonexistent-key".to_string()));
    }

    #[tokio::test]
    async fn test_request_tracker_expiration() {
        let tracker = RequestTracker::new(Duration::from_millis(50), 1000);

        // First check marks it
        assert!(!tracker.check_and_mark("expiring-key".to_string()));

        // Should be duplicate immediately
        assert!(tracker.check_and_mark("expiring-key".to_string()));

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should not be duplicate anymore (expired, and gets marked again)
        assert!(!tracker.check_and_mark("expiring-key".to_string()));
    }

    #[tokio::test]
    async fn test_request_tracker_cleanup_expired() {
        let tracker = RequestTracker::new(Duration::from_millis(50), 1000);

        // Mark multiple entries
        tracker.check_and_mark("key1".to_string());
        tracker.check_and_mark("key2".to_string());
        tracker.check_and_mark("key3".to_string());

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
        assert!(!tracker.check_and_mark("test".to_string()));

        // Should be accessible through the layer's tracker (now it's a duplicate)
        let layer_tracker = layer.tracker();
        assert!(layer_tracker.check_and_mark("test".to_string()));
    }

    #[test]
    fn test_request_tracker_max_entries_eviction() {
        let tracker = RequestTracker::new(Duration::from_secs(60), 3);

        // Mark 3 entries (at max capacity)
        assert!(!tracker.check_and_mark("key1".to_string()));
        assert!(!tracker.check_and_mark("key2".to_string()));
        assert!(!tracker.check_and_mark("key3".to_string()));

        assert_eq!(tracker.tracker.len(), 3);

        // Mark 4th entry - should trigger eviction
        assert!(!tracker.check_and_mark("key4".to_string()));

        // Should still be at max capacity
        assert_eq!(tracker.tracker.len(), 3);

        // The newest entry should exist (is now a duplicate)
        assert!(tracker.check_and_mark("key4".to_string()));

        // key1 should have been evicted (oldest entry)
        // We can verify by checking the tracker directly
        assert!(!tracker.tracker.contains_key("key1"));

        // key2 and key3 should still exist
        assert!(tracker.tracker.contains_key("key2"));
        assert!(tracker.tracker.contains_key("key3"));
    }
}
