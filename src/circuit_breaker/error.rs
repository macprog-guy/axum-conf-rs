//! Circuit breaker error types.

use std::time::Duration;
use thiserror::Error;

/// Error type for circuit breaker operations.
///
/// This is a generic error type that wraps the underlying call error,
/// allowing circuit breakers to be used with any fallible operation.
#[derive(Debug, Error)]
pub enum CircuitBreakerError<E> {
    /// The circuit breaker is open and rejecting requests.
    #[error("Circuit breaker is open for target: {target}")]
    CircuitOpen {
        /// The name of the target that is circuit-broken.
        target: String,
    },

    /// The underlying call failed.
    #[error("Call failed: {0}")]
    CallFailed(#[source] E),

    /// The call timed out.
    #[error("Call timed out after {duration:?}")]
    Timeout {
        /// The duration after which the call timed out.
        duration: Duration,
    },
}

impl<E> CircuitBreakerError<E> {
    /// Create a new `CircuitOpen` error.
    pub fn circuit_open(target: impl Into<String>) -> Self {
        Self::CircuitOpen {
            target: target.into(),
        }
    }

    /// Create a new `CallFailed` error.
    pub fn call_failed(error: E) -> Self {
        Self::CallFailed(error)
    }

    /// Create a new `Timeout` error.
    pub fn timeout(duration: Duration) -> Self {
        Self::Timeout { duration }
    }

    /// Returns `true` if the circuit is open.
    pub fn is_circuit_open(&self) -> bool {
        matches!(self, Self::CircuitOpen { .. })
    }

    /// Returns `true` if the call failed.
    pub fn is_call_failed(&self) -> bool {
        matches!(self, Self::CallFailed(_))
    }

    /// Returns `true` if the call timed out.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Get the target name if this is a `CircuitOpen` error.
    pub fn target(&self) -> Option<&str> {
        match self {
            Self::CircuitOpen { target } => Some(target),
            _ => None,
        }
    }

    /// Map the inner error type to a different type.
    pub fn map<F, E2>(self, f: F) -> CircuitBreakerError<E2>
    where
        F: FnOnce(E) -> E2,
    {
        match self {
            Self::CircuitOpen { target } => CircuitBreakerError::CircuitOpen { target },
            Self::CallFailed(e) => CircuitBreakerError::CallFailed(f(e)),
            Self::Timeout { duration } => CircuitBreakerError::Timeout { duration },
        }
    }

    /// Returns whether this error is likely transient and worth retrying after a
    /// delay.
    ///
    /// A timeout is transient, and a failed downstream call is conservatively
    /// treated as transient too (the breaker only surfaces it after the call
    /// actually ran) — though note the wrapped inner error may itself be
    /// deterministic (e.g. a "row not found"), so callers retrying a guarded
    /// result should inspect it rather than retry blindly. An **open circuit** is
    /// *not* transient: it is a deliberate fast-fail that only clears after the
    /// reset timeout, so a short-backoff retry cannot help. Consistent with
    /// [`crate::Error::is_transient`] via the [`From`] bridge below.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::CallFailed(_))
    }
}

/// Bridges the generic circuit-breaker error into the crate's unified [`Error`]
/// taxonomy, preserving the original error as the source so the chain and
/// [`is_transient`](crate::Error::is_transient) classification compose.
impl<E> From<CircuitBreakerError<E>> for crate::Error
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn from(err: CircuitBreakerError<E>) -> Self {
        let kind = match &err {
            CircuitBreakerError::CircuitOpen { .. } => crate::ErrorKind::CircuitBreakerOpen,
            CircuitBreakerError::CallFailed(_) | CircuitBreakerError::Timeout { .. } => {
                crate::ErrorKind::CircuitBreakerFailed
            }
        };
        crate::Error::new(kind, err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestError(String);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for TestError {}

    #[test]
    fn test_circuit_open_error() {
        let err: CircuitBreakerError<TestError> = CircuitBreakerError::circuit_open("payment-api");

        assert!(err.is_circuit_open());
        assert!(!err.is_call_failed());
        assert!(!err.is_timeout());
        assert_eq!(err.target(), Some("payment-api"));
        assert!(err.to_string().contains("payment-api"));
    }

    #[test]
    fn test_call_failed_error() {
        let inner = TestError("connection refused".to_string());
        let err: CircuitBreakerError<TestError> = CircuitBreakerError::call_failed(inner);

        assert!(!err.is_circuit_open());
        assert!(err.is_call_failed());
        assert!(!err.is_timeout());
        assert!(err.target().is_none());
        assert!(err.to_string().contains("Call failed"));
    }

    #[test]
    fn test_timeout_error() {
        let err: CircuitBreakerError<TestError> =
            CircuitBreakerError::timeout(Duration::from_secs(5));

        assert!(!err.is_circuit_open());
        assert!(!err.is_call_failed());
        assert!(err.is_timeout());
        assert!(err.target().is_none());
        assert!(err.to_string().contains("5s"));
    }

    #[test]
    fn test_map_error() {
        let err: CircuitBreakerError<String> =
            CircuitBreakerError::call_failed("error".to_string());
        let mapped: CircuitBreakerError<usize> = err.map(|s| s.len());

        match mapped {
            CircuitBreakerError::CallFailed(len) => assert_eq!(len, 5),
            _ => panic!("Expected CallFailed"),
        }
    }

    #[test]
    fn test_map_preserves_circuit_open() {
        let err: CircuitBreakerError<String> = CircuitBreakerError::circuit_open("test");
        let mapped: CircuitBreakerError<usize> = err.map(|s| s.len());

        assert!(mapped.is_circuit_open());
        assert_eq!(mapped.target(), Some("test"));
    }

    #[test]
    fn test_map_preserves_timeout() {
        let duration = Duration::from_millis(100);
        let err: CircuitBreakerError<String> = CircuitBreakerError::timeout(duration);
        let mapped: CircuitBreakerError<usize> = err.map(|s| s.len());

        match mapped {
            CircuitBreakerError::Timeout { duration: d } => assert_eq!(d, duration),
            _ => panic!("Expected Timeout"),
        }
    }

    #[test]
    fn test_is_transient_classifies_variants() {
        let open: CircuitBreakerError<TestError> = CircuitBreakerError::circuit_open("x");
        let timeout: CircuitBreakerError<TestError> =
            CircuitBreakerError::timeout(Duration::from_secs(1));
        let failed: CircuitBreakerError<TestError> =
            CircuitBreakerError::call_failed(TestError("e".to_string()));
        // An open circuit is a deliberate fast-fail: not transient.
        assert!(!open.is_transient());
        assert!(timeout.is_transient());
        assert!(failed.is_transient());
    }

    #[test]
    fn test_bridges_into_unified_error() {
        // Consistency: CircuitOpen -> CircuitBreakerOpen (non-transient in both).
        let open: CircuitBreakerError<TestError> = CircuitBreakerError::circuit_open("db");
        let err: crate::Error = open.into();
        assert_eq!(err.kind(), crate::ErrorKind::CircuitBreakerOpen);
        assert!(!err.is_transient());

        let timeout: CircuitBreakerError<TestError> =
            CircuitBreakerError::timeout(Duration::from_secs(1));
        let err: crate::Error = timeout.into();
        assert_eq!(err.kind(), crate::ErrorKind::CircuitBreakerFailed);
        assert!(err.is_transient());

        // The original error is preserved as the source.
        let failed: CircuitBreakerError<TestError> =
            CircuitBreakerError::call_failed(TestError("boom".to_string()));
        let err: crate::Error = failed.into();
        assert!(std::error::Error::source(&err).is_some());
    }
}
