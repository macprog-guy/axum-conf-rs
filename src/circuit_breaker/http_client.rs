//! HTTP client wrapper for circuit-protected external calls.
//!
//! Provides a simple wrapper for making HTTP calls through a circuit breaker.

use super::{CircuitBreakerError, CircuitBreakerRegistry, guarded_call};
use std::future::Future;
use std::sync::Arc;

/// HTTP client wrapper for circuit-protected external calls.
///
/// This is a lightweight wrapper that doesn't include an HTTP client itself,
/// allowing you to use any HTTP client (reqwest, hyper, etc.) while still
/// getting circuit breaker protection.
///
/// # Example
///
/// ```rust,no_run
/// use axum_conf::circuit_breaker::{GuardedHttpClient, CircuitBreakerRegistry, CircuitBreakerError};
/// use axum_conf::CircuitBreakerConfig;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = CircuitBreakerConfig::default();
/// let registry = CircuitBreakerRegistry::new(&config);
/// let client = GuardedHttpClient::new(registry);
///
/// // Use with any async operation
/// let result: Result<String, CircuitBreakerError<reqwest::Error>> = client.request("payment-api", async {
///     reqwest::get("https://api.stripe.com/v1/charges")
///         .await?
///         .text()
///         .await
/// }).await;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct GuardedHttpClient {
    registry: CircuitBreakerRegistry,
}

impl GuardedHttpClient {
    /// Create a new guarded HTTP client.
    ///
    /// # Arguments
    ///
    /// * `registry` - The circuit breaker registry to use
    pub fn new(registry: CircuitBreakerRegistry) -> Self {
        Self { registry }
    }

    /// Execute an HTTP request through a circuit breaker.
    ///
    /// The target name is used to look up (or create) a circuit breaker
    /// in the registry. Different targets have independent circuit breakers.
    ///
    /// # Arguments
    ///
    /// * `target` - The circuit breaker target name (e.g., "payment-api")
    /// * `f` - The async operation to execute
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let response = client.request("user-service", async {
    ///     reqwest::get("https://users.example.com/api/v1/users")
    ///         .await?
    ///         .json::<Vec<User>>()
    ///         .await
    /// }).await?;
    /// ```
    pub async fn request<F, T, E>(&self, target: &str, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        let breaker = self.registry.get_or_default(target);
        guarded_call(&breaker, target, f).await
    }

    /// Get a circuit breaker for a specific target.
    ///
    /// Useful for checking circuit state before making a request.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let breaker = client.circuit_breaker("payment-api");
    /// if breaker.current_state() == CircuitState::Open {
    ///     // Handle degraded mode
    /// }
    /// ```
    pub fn circuit_breaker(&self, target: &str) -> Arc<super::CircuitBreakerState> {
        self.registry.get_or_default(target)
    }

    /// Get a reference to the underlying registry.
    pub fn registry(&self) -> &CircuitBreakerRegistry {
        &self.registry
    }
}

impl Default for GuardedHttpClient {
    fn default() -> Self {
        Self::new(CircuitBreakerRegistry::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit_breaker::CircuitState;
    use crate::config::{CircuitBreakerConfig, CircuitBreakerTargetConfig};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_request_success() {
        let client = GuardedHttpClient::default();

        let result: Result<i32, CircuitBreakerError<&str>> =
            client.request("test-api", async { Ok(42) }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_request_failure() {
        let client = GuardedHttpClient::default();

        let result: Result<i32, CircuitBreakerError<&str>> = client
            .request("test-api", async { Err("connection refused") })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_call_failed());
    }

    #[tokio::test]
    async fn test_circuit_opens_after_failures() {
        let mut targets = HashMap::new();
        targets.insert(
            "failing-api".to_string(),
            CircuitBreakerTargetConfig {
                failure_threshold: 2,
                ..Default::default()
            },
        );

        let config = CircuitBreakerConfig { targets };
        let registry = CircuitBreakerRegistry::new(&config);
        let client = GuardedHttpClient::new(registry);

        // First failure
        let _: Result<i32, _> = client
            .request("failing-api", async { Err::<i32, _>("error") })
            .await;

        // Second failure - should trip the circuit
        let _: Result<i32, _> = client
            .request("failing-api", async { Err::<i32, _>("error") })
            .await;

        // Circuit should now be open
        let breaker = client.circuit_breaker("failing-api");
        assert_eq!(breaker.current_state(), CircuitState::Open);

        // Next request should fail fast
        let result: Result<i32, CircuitBreakerError<&str>> =
            client.request("failing-api", async { Ok(42) }).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_circuit_open());
    }

    #[test]
    fn test_different_targets_independent() {
        let client = GuardedHttpClient::default();

        let breaker_a = client.circuit_breaker("api-a");
        let breaker_b = client.circuit_breaker("api-b");

        // Record failure on A
        breaker_a.record_failure();
        breaker_a.record_failure();
        breaker_a.record_failure();
        breaker_a.record_failure();
        breaker_a.record_failure();

        // A should be open, B should still be closed
        assert_eq!(breaker_a.current_state(), CircuitState::Open);
        assert_eq!(breaker_b.current_state(), CircuitState::Closed);
    }

    #[test]
    fn test_default_client() {
        let client = GuardedHttpClient::default();
        assert!(client.registry().is_empty());
    }
}
