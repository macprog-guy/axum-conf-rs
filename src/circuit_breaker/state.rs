//! Circuit breaker state machine implementation.
//!
//! Provides thread-safe state management for circuit breakers with atomic
//! state transitions between Closed, Open, and HalfOpen states.

use crate::config::CircuitBreakerTargetConfig;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// Circuit breaker state: Closed (normal), Open (failing), HalfOpen (probing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation. Requests pass through, failures are counted.
    Closed,
    /// Circuit is tripped. Requests fail fast without calling the target.
    Open,
    /// Testing recovery. Limited probe requests allowed.
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Thread-safe circuit breaker with atomic state transitions.
///
/// # State Transitions
/// - Closed → Open: after `failure_threshold` consecutive failures
/// - Open → HalfOpen: after `reset_timeout` expires
/// - HalfOpen → Closed: after `success_threshold` successes
/// - HalfOpen → Open: on any failure
pub struct CircuitBreakerState {
    config: CircuitBreakerTargetConfig,
    state: RwLock<CircuitState>,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    half_open_calls: AtomicU32,
    last_failure_time: RwLock<Option<Instant>>,
}

impl CircuitBreakerState {
    /// Create a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerTargetConfig) -> Self {
        Self {
            config,
            state: RwLock::new(CircuitState::Closed),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            half_open_calls: AtomicU32::new(0),
            last_failure_time: RwLock::new(None),
        }
    }

    /// Check if a request should be allowed through the circuit breaker.
    ///
    /// Returns `true` if the request should proceed, `false` if it should be rejected.
    pub fn should_allow(&self) -> bool {
        let state = *self.state.read().unwrap();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                let last_failure = self.last_failure_time.read().unwrap();
                if let Some(time) = *last_failure
                    && time.elapsed() >= self.config.reset_timeout
                {
                    // Transition to half-open
                    self.transition_to_half_open();
                    // Allow this request as a probe
                    self.half_open_calls.fetch_add(1, Ordering::SeqCst);
                    return true;
                }
                false
            }
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                let current_calls = self.half_open_calls.fetch_add(1, Ordering::SeqCst);
                current_calls < self.config.half_open_max_calls
            }
        }
    }

    /// Record a successful call. May close the circuit if in half-open state.
    pub fn record_success(&self) {
        let state = *self.state.read().unwrap();

        match state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CircuitState::HalfOpen => {
                let count = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if count >= self.config.success_threshold {
                    self.transition_to_closed();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but if it does, ignore
            }
        }
    }

    /// Record a failed call. May open the circuit.
    pub fn record_failure(&self) {
        let state = *self.state.read().unwrap();

        match state {
            CircuitState::Closed => {
                let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if count >= self.config.failure_threshold {
                    self.transition_to_open();
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open immediately opens the circuit
                self.transition_to_open();
            }
            CircuitState::Open => {
                // Update last failure time
                *self.last_failure_time.write().unwrap() = Some(Instant::now());
            }
        }
    }

    /// Get the current circuit state.
    pub fn current_state(&self) -> CircuitState {
        *self.state.read().unwrap()
    }

    /// Get the current failure count.
    pub fn failure_count(&self) -> u32 {
        self.failure_count.load(Ordering::SeqCst)
    }

    /// Get the current success count (relevant in half-open state).
    pub fn success_count(&self) -> u32 {
        self.success_count.load(Ordering::SeqCst)
    }

    /// Get the configured call timeout, if any.
    pub fn call_timeout(&self) -> Option<std::time::Duration> {
        self.config.call_timeout
    }

    fn transition_to_open(&self) {
        let mut state = self.state.write().unwrap();
        *state = CircuitState::Open;
        *self.last_failure_time.write().unwrap() = Some(Instant::now());
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        self.half_open_calls.store(0, Ordering::SeqCst);

        tracing::warn!(
            state = %CircuitState::Open,
            "Circuit breaker opened"
        );
    }

    fn transition_to_half_open(&self) {
        let mut state = self.state.write().unwrap();
        *state = CircuitState::HalfOpen;
        self.success_count.store(0, Ordering::SeqCst);
        self.half_open_calls.store(0, Ordering::SeqCst);

        tracing::info!(
            state = %CircuitState::HalfOpen,
            "Circuit breaker transitioned to half-open"
        );
    }

    fn transition_to_closed(&self) {
        let mut state = self.state.write().unwrap();
        *state = CircuitState::Closed;
        self.failure_count.store(0, Ordering::SeqCst);
        self.success_count.store(0, Ordering::SeqCst);
        self.half_open_calls.store(0, Ordering::SeqCst);

        tracing::info!(
            state = %CircuitState::Closed,
            "Circuit breaker closed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_config() -> CircuitBreakerTargetConfig {
        CircuitBreakerTargetConfig {
            failure_threshold: 3,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(100),
            half_open_max_calls: 2,
            call_timeout: None,
        }
    }

    #[test]
    fn test_circuit_starts_closed() {
        let cb = CircuitBreakerState::new(test_config());
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert!(cb.should_allow());
    }

    #[test]
    fn test_circuit_opens_after_threshold() {
        let cb = CircuitBreakerState::new(test_config());

        // Record failures up to threshold
        for _ in 0..3 {
            assert!(cb.should_allow());
            cb.record_failure();
        }

        assert_eq!(cb.current_state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_rejects_when_open() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip the circuit
        for _ in 0..3 {
            cb.record_failure();
        }

        assert!(!cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::Open);
    }

    #[test]
    fn test_success_resets_failure_count() {
        let cb = CircuitBreakerState::new(test_config());

        // Record some failures (but not enough to trip)
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);

        // Success resets the count
        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_transitions_to_half_open() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip the circuit
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.current_state(), CircuitState::Open);

        // Wait for reset timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should allow probe request and transition to half-open
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_closes_after_success_threshold() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip and wait for half-open
        for _ in 0..3 {
            cb.record_failure();
        }
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Trigger transition to half-open
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        // Record successes in half-open
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_failure_in_half_open_reopens_circuit() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip and wait for half-open
        for _ in 0..3 {
            cb.record_failure();
        }
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Trigger transition to half-open
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        // Failure reopens immediately
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);
    }

    #[test]
    fn test_half_open_limits_concurrent_calls() {
        let config = CircuitBreakerTargetConfig {
            failure_threshold: 1,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(0), // Immediate transition
            half_open_max_calls: 2,
            call_timeout: None,
        };
        let cb = CircuitBreakerState::new(config);

        // Trip the circuit
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);

        // First call triggers half-open and is allowed
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        // Second call is allowed (under limit)
        assert!(cb.should_allow());

        // Third call exceeds limit
        assert!(!cb.should_allow());
    }

    #[test]
    fn test_circuit_state_display() {
        assert_eq!(format!("{}", CircuitState::Closed), "closed");
        assert_eq!(format!("{}", CircuitState::Open), "open");
        assert_eq!(format!("{}", CircuitState::HalfOpen), "half-open");
    }
}
