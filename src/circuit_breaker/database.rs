//! Database pool wrapper with circuit breaker protection.
//!
//! Wraps `sqlx::PgPool` to provide automatic failure tracking.

use super::{guarded_call, CircuitBreakerError, CircuitBreakerRegistry};
use sqlx_postgres::PgPool;
use std::future::Future;
use std::sync::Arc;

/// Database pool wrapper with circuit breaker protection.
///
/// Wraps `sqlx::PgPool` to provide automatic failure tracking.
/// Returns `CircuitBreakerError::CircuitOpen` when circuit is open.
///
/// # Example
///
/// ```rust,no_run
/// use axum_conf::circuit_breaker::{GuardedPool, CircuitBreakerRegistry};
/// use axum_conf::CircuitBreakerConfig;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Get pool from FluentRouter
/// // let pool = router.guarded_db_pool("database");
/// //
/// // Or create manually:
/// // let registry = CircuitBreakerRegistry::new(&CircuitBreakerConfig::default());
/// // let pool = GuardedPool::new(pg_pool, registry, "database");
/// //
/// // Use with query closure:
/// // let users: Vec<User> = pool.query(|p| async move {
/// //     sqlx::query_as!(User, "SELECT * FROM users")
/// //         .fetch_all(&p)
/// //         .await
/// // }).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct GuardedPool {
    pool: PgPool,
    registry: CircuitBreakerRegistry,
    target: String,
}

impl GuardedPool {
    /// Create a new guarded pool.
    ///
    /// # Arguments
    ///
    /// * `pool` - The underlying sqlx PgPool
    /// * `registry` - The circuit breaker registry
    /// * `target` - The target name for this pool's circuit breaker
    pub fn new(pool: PgPool, registry: CircuitBreakerRegistry, target: impl Into<String>) -> Self {
        Self {
            pool,
            registry,
            target: target.into(),
        }
    }

    /// Execute a database operation with circuit breaker protection.
    ///
    /// This is the primary way to execute queries through the guarded pool.
    /// The closure receives a clone of the underlying PgPool and can execute
    /// any sqlx operations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Fetch one row
    /// let user: User = pool.query(|p| async move {
    ///     sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
    ///         .fetch_one(&p)
    ///         .await
    /// }).await?;
    ///
    /// // Fetch multiple rows
    /// let users: Vec<User> = pool.query(|p| async move {
    ///     sqlx::query_as!(User, "SELECT * FROM users")
    ///         .fetch_all(&p)
    ///         .await
    /// }).await?;
    ///
    /// // Execute a statement
    /// pool.query(|p| async move {
    ///     sqlx::query("DELETE FROM users WHERE id = $1")
    ///         .bind(user_id)
    ///         .execute(&p)
    ///         .await
    /// }).await?;
    /// ```
    pub async fn query<F, Fut, T>(&self, f: F) -> Result<T, CircuitBreakerError<sqlx::Error>>
    where
        F: FnOnce(PgPool) -> Fut,
        Fut: Future<Output = Result<T, sqlx::Error>>,
    {
        let breaker = self.registry.get_or_default(&self.target);
        let pool = self.pool.clone();

        guarded_call(&breaker, &self.target, f(pool)).await
    }

    /// Access the underlying pool directly.
    ///
    /// Use this when you need features not exposed by GuardedPool,
    /// but note that these calls will not be protected by the circuit breaker.
    pub fn inner(&self) -> &PgPool {
        &self.pool
    }

    /// Get a clone of the underlying pool.
    ///
    /// Use this when you need to pass the pool to another function,
    /// but note that these calls will not be protected by the circuit breaker.
    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    /// Get the target name for this pool's circuit breaker.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Get the circuit breaker for this pool.
    pub fn circuit_breaker(&self) -> Arc<super::CircuitBreakerState> {
        self.registry.get_or_default(&self.target)
    }

    /// Get a reference to the circuit breaker registry.
    pub fn registry(&self) -> &CircuitBreakerRegistry {
        &self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CircuitBreakerConfig;

    #[test]
    fn test_guarded_pool_target() {
        // We can't easily test the full pool without a database,
        // but we can test the basic construction
        let config = CircuitBreakerConfig::default();
        let registry = CircuitBreakerRegistry::new(&config);

        // Just verify the registry works
        let breaker = registry.get_or_default("database");
        assert!(breaker.should_allow());
    }
}
