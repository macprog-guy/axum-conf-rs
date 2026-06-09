//! Circuit breaker pattern for external service resilience.
//!
//! Provides fail-fast behavior when downstream dependencies are degraded,
//! preventing cascading failures across microservices.
//!
//! # Architecture
//!
//! Circuit breakers are organized **per target** (not per route):
//! - One circuit breaker for database calls
//! - One circuit breaker per external HTTP service
//!
//! # Example
//!
//! ```rust,no_run
//! use axum_conf::circuit_breaker::{CircuitBreakerRegistry, CircuitBreakerError};
//!
//! async fn call_with_circuit_breaker(registry: &CircuitBreakerRegistry) {
//!     let breaker = registry.get_or_default("payment-api");
//!
//!     if !breaker.should_allow() {
//!         // Circuit is open, fail fast
//!         return;
//!     }
//!
//!     // Make the call
//!     let result = make_external_call().await;
//!
//!     match result {
//!         Ok(_) => breaker.record_success(),
//!         Err(_) => breaker.record_failure(),
//!     }
//! }
//!
//! async fn make_external_call() -> Result<(), std::io::Error> {
//!     Ok(())
//! }
//! ```
//!
//! # State Machine
//!
//! ```text
//! CLOSED ──(failures >= threshold)──► OPEN
//!    ▲                                  │
//!    │                                  │ (reset_timeout expires)
//!    │                                  ▼
//!    └──(successes >= threshold)── HALF-OPEN
//! ```
//!
//! - **CLOSED**: Normal operation. Requests pass through, failures are counted.
//! - **OPEN**: Circuit is tripped. Requests fail fast without calling the target.
//! - **HALF-OPEN**: Testing recovery. Limited probe requests allowed.

mod error;
mod registry;
mod state;

#[cfg(all(feature = "circuit-breaker", feature = "postgres"))]
mod database;

#[cfg(feature = "circuit-breaker")]
mod http_client;

pub use error::CircuitBreakerError;
pub use registry::CircuitBreakerRegistry;
pub use state::{CircuitBreakerState, CircuitState};

#[cfg(all(feature = "circuit-breaker", feature = "postgres"))]
pub use database::GuardedPool;

#[cfg(feature = "circuit-breaker")]
pub use http_client::GuardedHttpClient;

use std::future::Future;
use std::sync::Arc;

/// Execute a call through a circuit breaker.
///
/// This is a convenience function that handles the circuit breaker protocol:
/// 1. Check if the circuit allows the request
/// 2. Execute the call if allowed
/// 3. Record success or failure
///
/// # Example
///
/// ```rust,no_run
/// use axum_conf::circuit_breaker::{CircuitBreakerRegistry, guarded_call};
///
/// async fn example(registry: &CircuitBreakerRegistry) {
///     let breaker = registry.get_or_default("api");
///
///     let result = guarded_call(&breaker, "api", async {
///         // Your async operation here
///         Ok::<_, std::io::Error>(42)
///     }).await;
/// }
/// ```
pub async fn guarded_call<F, T, E>(
    breaker: &Arc<CircuitBreakerState>,
    target: &str,
    f: F,
) -> Result<T, CircuitBreakerError<E>>
where
    F: Future<Output = Result<T, E>>,
{
    // Default classifier: every error counts as a failure.
    guarded_call_with(breaker, target, f, |_| true).await
}

/// Execute a call through a circuit breaker, classifying which errors count as
/// failures.
///
/// Identical to [`guarded_call`], but `is_failure` decides whether a given error
/// indicates the target is **degraded** (and should trip the breaker) or is a
/// normal application outcome (and should not). For example, a database query
/// returning no rows is not a sign the database is down, so it should not count
/// toward opening the circuit. A non-failure error is recorded as a *neutral*
/// outcome (see [`CircuitBreakerState::record_neutral`]): it clears the
/// consecutive-failure streak when closed, but does **not** advance a half-open
/// circuit toward closing — only a genuine success does that.
///
/// A call timeout always counts as a failure regardless of `is_failure`.
pub async fn guarded_call_with<F, T, E>(
    breaker: &Arc<CircuitBreakerState>,
    target: &str,
    f: F,
    is_failure: impl Fn(&E) -> bool,
) -> Result<T, CircuitBreakerError<E>>
where
    F: Future<Output = Result<T, E>>,
{
    if !breaker.should_allow() {
        return Err(CircuitBreakerError::circuit_open(target));
    }

    let record = |breaker: &CircuitBreakerState, e: &E| {
        if is_failure(e) {
            breaker.record_failure();
        } else {
            // The target responded; the error is an application-level outcome,
            // not degradation. Record it as neutral (clears the failure streak
            // when closed, but does not count as half-open recovery progress).
            breaker.record_neutral();
        }
    };

    // Check for call timeout
    if let Some(timeout_duration) = breaker.call_timeout() {
        match tokio::time::timeout(timeout_duration, f).await {
            Ok(Ok(result)) => {
                breaker.record_success();
                Ok(result)
            }
            Ok(Err(e)) => {
                record(breaker, &e);
                Err(CircuitBreakerError::call_failed(e))
            }
            Err(_) => {
                breaker.record_failure();
                Err(CircuitBreakerError::timeout(timeout_duration))
            }
        }
    } else {
        match f.await {
            Ok(result) => {
                breaker.record_success();
                Ok(result)
            }
            Err(e) => {
                record(breaker, &e);
                Err(CircuitBreakerError::call_failed(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CircuitBreakerTargetConfig;
    use std::time::Duration;

    #[tokio::test]
    async fn test_guarded_call_success() {
        let config = CircuitBreakerTargetConfig::default();
        let breaker = Arc::new(CircuitBreakerState::new(config));

        let result: Result<i32, CircuitBreakerError<&str>> =
            guarded_call(&breaker, "test", async { Ok(42) }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(breaker.failure_count(), 0);
    }

    #[tokio::test]
    async fn test_guarded_call_failure() {
        let config = CircuitBreakerTargetConfig::default();
        let breaker = Arc::new(CircuitBreakerState::new(config));

        let result: Result<i32, CircuitBreakerError<&str>> =
            guarded_call(&breaker, "test", async { Err("error") }).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_call_failed());
        assert_eq!(breaker.failure_count(), 1);
    }

    #[tokio::test]
    async fn classifier_excludes_non_failures() {
        let config = CircuitBreakerTargetConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let breaker = Arc::new(CircuitBreakerState::new(config));

        // An error classified as NOT degradation (e.g. "no rows") must not trip
        // the circuit — it is recorded as a success instead.
        let result: Result<i32, CircuitBreakerError<&str>> =
            guarded_call_with(&breaker, "db", async { Err("no rows") }, |_| false).await;
        assert!(result.is_err());
        assert_eq!(breaker.current_state(), CircuitState::Closed);
        assert_eq!(breaker.failure_count(), 0);

        // A degradation error (classified true) opens the circuit at threshold 1.
        let result: Result<i32, CircuitBreakerError<&str>> =
            guarded_call_with(&breaker, "db", async { Err("connection refused") }, |_| {
                true
            })
            .await;
        assert!(result.is_err());
        assert_eq!(breaker.current_state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_guarded_call_circuit_open() {
        let config = CircuitBreakerTargetConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        let breaker = Arc::new(CircuitBreakerState::new(config));

        // Trip the circuit
        breaker.record_failure();
        assert_eq!(breaker.current_state(), CircuitState::Open);

        let result: Result<i32, CircuitBreakerError<&str>> =
            guarded_call(&breaker, "test", async { Ok(42) }).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.is_circuit_open());
        assert_eq!(err.target(), Some("test"));
    }

    // `start_paused` drives tokio's virtual clock: the call_timeout (which uses
    // `tokio::time::timeout`) and the inner sleep share the paused timer, so the
    // timeout fires deterministically and instantly with no real wall-clock wait.
    #[tokio::test(start_paused = true)]
    async fn test_guarded_call_timeout() {
        let config = CircuitBreakerTargetConfig {
            call_timeout: Some(Duration::from_millis(10)),
            ..Default::default()
        };
        let breaker = Arc::new(CircuitBreakerState::new(config));

        let result: Result<i32, CircuitBreakerError<&str>> =
            guarded_call(&breaker, "test", async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(42)
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_timeout());
        assert_eq!(breaker.failure_count(), 1);
    }
}
