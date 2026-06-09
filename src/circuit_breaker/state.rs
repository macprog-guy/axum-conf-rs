//! Circuit breaker state machine implementation.
//!
//! Provides thread-safe state management for circuit breakers with atomic
//! state transitions between Closed, Open, and HalfOpen states.

use crate::config::CircuitBreakerTargetConfig;
use std::sync::{Mutex, MutexGuard};
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

/// Mutable circuit breaker state, guarded by a single lock so that every
/// check-then-act sequence is atomic (no TOCTOU windows between threads).
struct Inner {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    half_open_calls: u32,
    last_failure_time: Option<Instant>,
}

/// Thread-safe circuit breaker with atomic state transitions.
///
/// All mutable state lives behind a single `Mutex`, so concurrent callers
/// cannot observe a partially-updated state machine. The critical sections are
/// tiny and never span an `.await`, so a blocking `std::sync::Mutex` is the
/// right choice here even though callers are async.
///
/// # State Transitions
/// - Closed → Open: after `failure_threshold` consecutive failures
/// - Open → HalfOpen: after `reset_timeout` expires
/// - HalfOpen → Closed: after `success_threshold` successes
/// - HalfOpen → Open: on any failure
pub struct CircuitBreakerState {
    config: CircuitBreakerTargetConfig,
    inner: Mutex<Inner>,
}

impl CircuitBreakerState {
    /// Create a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerTargetConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(Inner {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                half_open_calls: 0,
                last_failure_time: None,
            }),
        }
    }

    /// Acquire the inner lock, recovering from poisoning.
    ///
    /// A panic in another thread while holding the lock must not cascade into a
    /// service-wide panic storm, so we deliberately recover the inner value
    /// instead of propagating `PoisonError`.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Test-only clock seam: backdate `last_failure_time` so the reset timeout is
    /// considered elapsed. This lets the `Open → HalfOpen` transition be exercised
    /// deterministically and instantly, instead of sleeping on the wall clock
    /// (the breaker uses `std::time::Instant`, which `tokio::time::pause` cannot
    /// control). The circuit must be `Open` for this to be meaningful.
    #[cfg(test)]
    fn force_reset_timeout_elapsed(&self) {
        let mut g = self.lock();
        let backdated = std::time::Instant::now()
            .checked_sub(self.config.reset_timeout + std::time::Duration::from_millis(50))
            .expect("test clock backdate underflowed the monotonic epoch");
        g.last_failure_time = Some(backdated);
    }

    /// Check if a request should be allowed through the circuit breaker.
    ///
    /// Returns `true` if the request should proceed, `false` if it should be rejected.
    pub fn should_allow(&self) -> bool {
        let mut g = self.lock();

        match g.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has elapsed
                if let Some(time) = g.last_failure_time
                    && time.elapsed() >= self.config.reset_timeout
                {
                    // Transition to half-open and allow this request as a probe.
                    self.transition_to_half_open(&mut g);
                    g.half_open_calls += 1;
                    return true;
                }
                false
            }
            CircuitState::HalfOpen => {
                // Allow limited requests in half-open state
                let current_calls = g.half_open_calls;
                g.half_open_calls += 1;
                current_calls < self.config.half_open_max_calls
            }
        }
    }

    /// Record a successful call. May close the circuit if in half-open state.
    pub fn record_success(&self) {
        let mut g = self.lock();

        match g.state {
            CircuitState::Closed => {
                // Reset failure count on success
                g.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                g.success_count += 1;
                if g.success_count >= self.config.success_threshold {
                    self.transition_to_closed(&mut g);
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but if it does, ignore
            }
        }
    }

    /// Record a *neutral* outcome: the target responded, but the call was not a
    /// genuine success (e.g. a database query that returned no rows or hit a
    /// constraint — an application-level error, not degradation).
    ///
    /// In `Closed` this clears the consecutive-failure streak (the target is
    /// reachable). In `HalfOpen` it deliberately does **nothing**: only genuine
    /// successes should advance the recovery budget toward closing the circuit,
    /// so a burst of application-level errors can't close a breaker without a
    /// single working query.
    pub fn record_neutral(&self) {
        let mut g = self.lock();
        if matches!(g.state, CircuitState::Closed) {
            g.failure_count = 0;
        }
    }

    /// Record a failed call. May open the circuit.
    pub fn record_failure(&self) {
        let mut g = self.lock();

        match g.state {
            CircuitState::Closed => {
                g.failure_count += 1;
                if g.failure_count >= self.config.failure_threshold {
                    self.transition_to_open(&mut g);
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open immediately opens the circuit
                self.transition_to_open(&mut g);
            }
            CircuitState::Open => {
                // Already open. Do NOT refresh `last_failure_time` here: the reset
                // timeout is measured from when the circuit opened, and refreshing
                // it on every late/in-flight failure could push the Open→HalfOpen
                // transition back indefinitely and prevent recovery.
            }
        }
    }

    /// Get the current circuit state.
    pub fn current_state(&self) -> CircuitState {
        self.lock().state
    }

    /// Returns whether the circuit is currently in the `Open` state, **without
    /// any side effect**.
    ///
    /// Unlike [`should_allow`](Self::should_allow), this never transitions the
    /// state machine or consumes a half-open probe slot. Note it stays `true`
    /// even after the reset timeout has elapsed (the transition to half-open only
    /// happens on a real call); for readiness checks prefer
    /// [`is_fast_failing`](Self::is_fast_failing).
    pub fn is_open(&self) -> bool {
        matches!(self.lock().state, CircuitState::Open)
    }

    /// Returns whether the circuit is currently **fast-failing**: `Open` *and*
    /// still within its reset timeout. Side-effect-free.
    ///
    /// Designed for health probes: while fast-failing, a probe should report
    /// unavailable without attempting the call. Once the reset timeout has
    /// elapsed this returns `false` even though the state is still `Open`, so the
    /// probe can attempt the real call again (and recover) rather than reporting
    /// unavailable forever when no other traffic drives the breaker's recovery.
    pub fn is_fast_failing(&self) -> bool {
        let g = self.lock();
        match g.state {
            CircuitState::Open => g
                .last_failure_time
                .is_some_and(|t| t.elapsed() < self.config.reset_timeout),
            CircuitState::Closed | CircuitState::HalfOpen => false,
        }
    }

    /// Get the current failure count.
    pub fn failure_count(&self) -> u32 {
        self.lock().failure_count
    }

    /// Get the current success count (relevant in half-open state).
    pub fn success_count(&self) -> u32 {
        self.lock().success_count
    }

    /// Get the configured call timeout, if any.
    pub fn call_timeout(&self) -> Option<std::time::Duration> {
        self.config.call_timeout
    }

    fn transition_to_open(&self, g: &mut Inner) {
        g.state = CircuitState::Open;
        g.last_failure_time = Some(Instant::now());
        g.failure_count = 0;
        g.success_count = 0;
        g.half_open_calls = 0;

        tracing::warn!(
            state = %CircuitState::Open,
            "Circuit breaker opened"
        );
    }

    fn transition_to_half_open(&self, g: &mut Inner) {
        g.state = CircuitState::HalfOpen;
        g.success_count = 0;
        g.half_open_calls = 0;

        tracing::info!(
            state = %CircuitState::HalfOpen,
            "Circuit breaker transitioned to half-open"
        );
    }

    fn transition_to_closed(&self, g: &mut Inner) {
        g.state = CircuitState::Closed;
        g.failure_count = 0;
        g.success_count = 0;
        g.half_open_calls = 0;

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

    #[test]
    fn test_circuit_transitions_to_half_open() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip the circuit
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.current_state(), CircuitState::Open);

        // Before the reset timeout elapses, requests are still rejected.
        assert!(!cb.should_allow());

        // Deterministically elapse the reset timeout (no wall-clock sleep).
        cb.force_reset_timeout_elapsed();

        // Should allow probe request and transition to half-open
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_closes_after_success_threshold() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip and elapse the reset timeout
        for _ in 0..3 {
            cb.record_failure();
        }
        cb.force_reset_timeout_elapsed();

        // Trigger transition to half-open
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        // Record successes in half-open
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn test_failure_in_half_open_reopens_circuit() {
        let cb = CircuitBreakerState::new(test_config());

        // Trip and elapse the reset timeout
        for _ in 0..3 {
            cb.record_failure();
        }
        cb.force_reset_timeout_elapsed();

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

    #[test]
    fn test_concurrent_half_open_never_exceeds_limit() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let config = CircuitBreakerTargetConfig {
            failure_threshold: 1,
            success_threshold: 5,
            reset_timeout: Duration::from_millis(0), // immediate Open -> HalfOpen
            half_open_max_calls: 3,
            call_timeout: None,
        };
        let cb = Arc::new(CircuitBreakerState::new(config));

        // Trip the circuit so the first allowed call transitions to half-open.
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);

        // Hammer should_allow() from many threads. Even though every thread sees
        // the reset timeout as elapsed, the single-lock state machine must let at
        // most `half_open_max_calls` probes through.
        let allowed = Arc::new(AtomicU32::new(0));
        let mut handles = Vec::new();
        for _ in 0..32 {
            let cb = Arc::clone(&cb);
            let allowed = Arc::clone(&allowed);
            handles.push(std::thread::spawn(move || {
                if cb.should_allow() {
                    allowed.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
        assert_eq!(
            allowed.load(Ordering::SeqCst),
            3,
            "exactly half_open_max_calls probes should be allowed"
        );
    }

    #[test]
    fn is_open_is_side_effect_free() {
        let config = CircuitBreakerTargetConfig {
            failure_threshold: 1,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(0), // immediate Open -> HalfOpen
            half_open_max_calls: 2,
            call_timeout: None,
        };
        let cb = CircuitBreakerState::new(config);
        cb.record_failure();
        assert!(cb.is_open());

        // Polling is_open() many times must not transition the state or consume
        // any half-open probe budget.
        for _ in 0..10 {
            assert!(cb.is_open());
        }
        assert_eq!(cb.current_state(), CircuitState::Open);

        // The full half-open probe budget is still available to real traffic.
        assert!(cb.should_allow());
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
        assert!(cb.should_allow());
        assert!(!cb.should_allow(), "budget of 2 probes should be exhausted");
    }

    #[test]
    fn is_fast_failing_clears_after_reset_window() {
        let cb = CircuitBreakerState::new(test_config()); // reset 100ms
        for _ in 0..3 {
            cb.record_failure();
        }
        assert!(cb.is_open());
        assert!(cb.is_fast_failing(), "Open and within the reset window");

        cb.force_reset_timeout_elapsed();
        assert!(cb.is_open(), "still Open — a peek never transitions");
        assert!(
            !cb.is_fast_failing(),
            "past the reset window, a probe may proceed (recovery not blocked)"
        );
    }

    #[test]
    fn record_neutral_does_not_advance_half_open_recovery() {
        let config = CircuitBreakerTargetConfig {
            failure_threshold: 1,
            success_threshold: 2,
            reset_timeout: Duration::from_millis(0),
            half_open_max_calls: 10,
            call_timeout: None,
        };
        let cb = CircuitBreakerState::new(config);
        cb.record_failure(); // -> Open
        cb.force_reset_timeout_elapsed();
        assert!(cb.should_allow()); // -> HalfOpen
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);

        // Neutral outcomes (non-degradation errors) must NOT count toward closing.
        cb.record_neutral();
        cb.record_neutral();
        cb.record_neutral();
        assert_eq!(
            cb.current_state(),
            CircuitState::HalfOpen,
            "neutral outcomes must not close a half-open circuit"
        );

        // Genuine successes advance recovery; success_threshold = 2 closes it.
        cb.record_success();
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn record_failure_while_open_does_not_delay_recovery() {
        let cb = CircuitBreakerState::new(test_config()); // threshold 3, reset 100ms
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.current_state(), CircuitState::Open);

        // Elapse the reset window, then deliver a late failure while Open. The
        // guard must NOT push the window back, so recovery still proceeds.
        cb.force_reset_timeout_elapsed();
        cb.record_failure();

        assert!(
            cb.should_allow(),
            "a late failure while Open must not delay the Open->HalfOpen transition"
        );
        assert_eq!(cb.current_state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_concurrent_failures_open_once() {
        use std::sync::Arc;

        let cb = Arc::new(CircuitBreakerState::new(test_config())); // failure_threshold = 3

        let mut handles = Vec::new();
        for _ in 0..64 {
            let cb = Arc::clone(&cb);
            handles.push(std::thread::spawn(move || cb.record_failure()));
        }
        for h in handles {
            h.join().unwrap();
        }

        // Many concurrent failures must leave the breaker open with a consistent
        // (post-transition reset) failure count, never a torn intermediate value.
        assert_eq!(cb.current_state(), CircuitState::Open);
    }
}
