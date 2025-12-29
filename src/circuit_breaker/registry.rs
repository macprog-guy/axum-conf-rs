//! Registry of named circuit breakers.
//!
//! Provides a thread-safe collection of circuit breakers indexed by target name.

use crate::config::{CircuitBreakerConfig, CircuitBreakerTargetConfig};
use dashmap::DashMap;
use std::sync::Arc;

use super::state::CircuitBreakerState;

/// Registry holding all circuit breakers by name.
///
/// # Example
/// ```rust,no_run
/// # use axum_conf::circuit_breaker::{CircuitBreakerRegistry, CircuitState};
/// # use axum_conf::CircuitBreakerConfig;
/// let registry = CircuitBreakerRegistry::new(&CircuitBreakerConfig::default());
/// let breaker = registry.get_or_default("payment-api");
/// if breaker.should_allow() {
///     // make call
/// }
/// ```
#[derive(Clone)]
pub struct CircuitBreakerRegistry {
    breakers: Arc<DashMap<String, Arc<CircuitBreakerState>>>,
    default_config: CircuitBreakerTargetConfig,
}

impl CircuitBreakerRegistry {
    /// Create a new registry from configuration.
    ///
    /// Pre-populates circuit breakers for all configured targets.
    pub fn new(config: &CircuitBreakerConfig) -> Self {
        let breakers = Arc::new(DashMap::new());

        // Pre-create circuit breakers for configured targets
        for (name, target_config) in &config.targets {
            let state = Arc::new(CircuitBreakerState::new(target_config.clone()));
            breakers.insert(name.clone(), state);
        }

        Self {
            breakers,
            default_config: CircuitBreakerTargetConfig::default(),
        }
    }

    /// Get a circuit breaker by target name.
    ///
    /// Returns `None` if the target was not pre-configured.
    pub fn get(&self, target: &str) -> Option<Arc<CircuitBreakerState>> {
        self.breakers.get(target).map(|r| r.value().clone())
    }

    /// Get a circuit breaker by target name, creating one with defaults if not found.
    ///
    /// This is useful for dynamic targets that weren't configured upfront.
    pub fn get_or_default(&self, target: &str) -> Arc<CircuitBreakerState> {
        self.breakers
            .entry(target.to_string())
            .or_insert_with(|| Arc::new(CircuitBreakerState::new(self.default_config.clone())))
            .value()
            .clone()
    }

    /// List all registered target names.
    pub fn targets(&self) -> Vec<String> {
        self.breakers.iter().map(|r| r.key().clone()).collect()
    }

    /// Get the number of registered circuit breakers.
    pub fn len(&self) -> usize {
        self.breakers.len()
    }

    /// Returns true if no circuit breakers are registered.
    pub fn is_empty(&self) -> bool {
        self.breakers.is_empty()
    }
}

impl Default for CircuitBreakerRegistry {
    fn default() -> Self {
        Self::new(&CircuitBreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_new_empty_registry() {
        let config = CircuitBreakerConfig::default();
        let registry = CircuitBreakerRegistry::new(&config);

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_new_with_configured_targets() {
        let mut targets = HashMap::new();
        targets.insert(
            "database".to_string(),
            CircuitBreakerTargetConfig {
                failure_threshold: 10,
                ..Default::default()
            },
        );
        targets.insert("api".to_string(), CircuitBreakerTargetConfig::default());

        let config = CircuitBreakerConfig { targets };
        let registry = CircuitBreakerRegistry::new(&config);

        assert_eq!(registry.len(), 2);
        assert!(registry.get("database").is_some());
        assert!(registry.get("api").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_get_or_default_creates_new() {
        let registry = CircuitBreakerRegistry::default();

        assert!(registry.get("dynamic").is_none());

        let breaker = registry.get_or_default("dynamic");
        assert!(breaker.should_allow());

        // Now it should exist
        assert!(registry.get("dynamic").is_some());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_get_or_default_returns_existing() {
        let mut targets = HashMap::new();
        targets.insert(
            "configured".to_string(),
            CircuitBreakerTargetConfig {
                failure_threshold: 100,
                ..Default::default()
            },
        );

        let config = CircuitBreakerConfig { targets };
        let registry = CircuitBreakerRegistry::new(&config);

        let breaker1 = registry.get_or_default("configured");
        let breaker2 = registry.get_or_default("configured");

        // Should be the same Arc
        assert!(Arc::ptr_eq(&breaker1, &breaker2));
    }

    #[test]
    fn test_targets_list() {
        let mut targets = HashMap::new();
        targets.insert("a".to_string(), CircuitBreakerTargetConfig::default());
        targets.insert("b".to_string(), CircuitBreakerTargetConfig::default());
        targets.insert("c".to_string(), CircuitBreakerTargetConfig::default());

        let config = CircuitBreakerConfig { targets };
        let registry = CircuitBreakerRegistry::new(&config);

        let mut target_names = registry.targets();
        target_names.sort();

        assert_eq!(target_names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_registry_clone_shares_state() {
        let registry1 = CircuitBreakerRegistry::default();
        let registry2 = registry1.clone();

        // Add to registry1
        registry1.get_or_default("shared");

        // Should be visible in registry2
        assert!(registry2.get("shared").is_some());
    }

    #[test]
    fn test_circuit_breaker_state_is_shared() {
        let registry = CircuitBreakerRegistry::default();

        let breaker1 = registry.get_or_default("test");
        let breaker2 = registry.get_or_default("test");

        // Record failure through one reference
        breaker1.record_failure();

        // Should be visible through the other
        assert_eq!(breaker2.failure_count(), 1);
    }
}
