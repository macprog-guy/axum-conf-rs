//! Circuit breaker configuration for external service resilience.

use serde::Deserialize;
use std::{collections::HashMap, time::Duration};

/// Configuration for a single circuit breaker target.
///
/// Each target (e.g., "database", "payment-api") has its own circuit breaker
/// with independent state and thresholds.
#[derive(Debug, Clone, Deserialize)]
pub struct CircuitBreakerTargetConfig {
    /// Number of consecutive failures before the circuit opens.
    /// Default: 5
    #[serde(default = "CircuitBreakerTargetConfig::default_failure_threshold")]
    pub failure_threshold: u32,

    /// Number of successes in half-open state required to close the circuit.
    /// Default: 3
    #[serde(default = "CircuitBreakerTargetConfig::default_success_threshold")]
    pub success_threshold: u32,

    /// Time to wait in open state before transitioning to half-open.
    /// Default: 30s
    #[serde(
        default = "CircuitBreakerTargetConfig::default_reset_timeout",
        with = "humantime_serde"
    )]
    pub reset_timeout: Duration,

    /// Maximum concurrent requests allowed in half-open state for probing.
    /// Default: 3
    #[serde(default = "CircuitBreakerTargetConfig::default_half_open_max_calls")]
    pub half_open_max_calls: u32,

    /// Optional per-call timeout. If set, calls exceeding this duration
    /// are considered failures.
    #[serde(default, with = "humantime_serde")]
    pub call_timeout: Option<Duration>,
}

impl CircuitBreakerTargetConfig {
    fn default_failure_threshold() -> u32 {
        5
    }

    fn default_success_threshold() -> u32 {
        3
    }

    fn default_reset_timeout() -> Duration {
        Duration::from_secs(30)
    }

    fn default_half_open_max_calls() -> u32 {
        3
    }
}

impl Default for CircuitBreakerTargetConfig {
    fn default() -> Self {
        Self {
            failure_threshold: Self::default_failure_threshold(),
            success_threshold: Self::default_success_threshold(),
            reset_timeout: Self::default_reset_timeout(),
            half_open_max_calls: Self::default_half_open_max_calls(),
            call_timeout: None,
        }
    }
}

/// Root configuration for all circuit breakers.
///
/// # Example TOML
///
/// ```toml
/// [circuit_breaker.targets.database]
/// failure_threshold = 5
/// reset_timeout = "30s"
///
/// [circuit_breaker.targets.payment-api]
/// failure_threshold = 3
/// reset_timeout = "60s"
/// call_timeout = "10s"
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CircuitBreakerConfig {
    /// Named circuit breaker configurations.
    /// Keys are target names like "database", "payment-api", "auth-service".
    #[serde(default)]
    pub targets: HashMap<String, CircuitBreakerTargetConfig>,
}

impl CircuitBreakerConfig {
    /// Returns true if any circuit breaker targets are configured.
    pub fn is_enabled(&self) -> bool {
        !self.targets.is_empty()
    }

    /// Get configuration for a specific target, or None if not configured.
    pub fn get(&self, target: &str) -> Option<&CircuitBreakerTargetConfig> {
        self.targets.get(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_target_config() {
        let config = CircuitBreakerTargetConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 3);
        assert_eq!(config.reset_timeout, Duration::from_secs(30));
        assert_eq!(config.half_open_max_calls, 3);
        assert!(config.call_timeout.is_none());
    }

    #[test]
    fn test_default_root_config() {
        let config = CircuitBreakerConfig::default();
        assert!(config.targets.is_empty());
        assert!(!config.is_enabled());
    }

    #[test]
    fn test_parse_circuit_breaker_config() {
        let toml_str = r#"
[targets.database]
failure_threshold = 10
success_threshold = 5
reset_timeout = "60s"
half_open_max_calls = 2

[targets.payment-api]
failure_threshold = 3
reset_timeout = "120s"
call_timeout = "5s"
"#;

        let config: CircuitBreakerConfig = toml::from_str(toml_str).unwrap();
        assert!(config.is_enabled());
        assert_eq!(config.targets.len(), 2);

        let db_config = config.get("database").unwrap();
        assert_eq!(db_config.failure_threshold, 10);
        assert_eq!(db_config.success_threshold, 5);
        assert_eq!(db_config.reset_timeout, Duration::from_secs(60));
        assert_eq!(db_config.half_open_max_calls, 2);
        assert!(db_config.call_timeout.is_none());

        let payment_config = config.get("payment-api").unwrap();
        assert_eq!(payment_config.failure_threshold, 3);
        assert_eq!(payment_config.success_threshold, 3); // default
        assert_eq!(payment_config.reset_timeout, Duration::from_secs(120));
        assert_eq!(payment_config.call_timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_parse_with_defaults() {
        let toml_str = r#"
[targets.minimal]
"#;

        let config: CircuitBreakerConfig = toml::from_str(toml_str).unwrap();
        let minimal = config.get("minimal").unwrap();

        assert_eq!(minimal.failure_threshold, 5);
        assert_eq!(minimal.success_threshold, 3);
        assert_eq!(minimal.reset_timeout, Duration::from_secs(30));
        assert_eq!(minimal.half_open_max_calls, 3);
    }
}
