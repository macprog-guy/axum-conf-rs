use serde::Deserialize;

///
/// Configuration for OpenTelemetry distributed tracing.
///
/// Enables exporting traces to an OTLP-compatible collector (e.g., Jaeger, Tempo).
///
/// # Examples
///
/// In TOML configuration:
/// ```toml
/// [logging.opentelemetry]
/// endpoint = "http://localhost:4317"
/// service_name = "my-service"
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct OpenTelemetryConfig {
    /// OTLP endpoint URL (e.g., "http://localhost:4317")
    pub endpoint: String,

    /// Service name for traces (defaults to package name)
    #[serde(default)]
    pub service_name: Option<String>,
}

impl OpenTelemetryConfig {
    /// Creates a new OpenTelemetryConfig with the specified endpoint.
    pub fn new(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            service_name: Some(env!("CARGO_PKG_NAME").into()),
        }
    }

    /// Sets the service name for the OpenTelemetryConfig.
    pub fn with_service_name(mut self, name: &str) -> Self {
        self.service_name = Some(name.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_endpoint_and_optional_service_name() {
        let cfg: OpenTelemetryConfig =
            toml::from_str("endpoint = \"http://localhost:4317\"\nservice_name = \"my-service\"")
                .expect("valid otel config");
        assert_eq!(cfg.endpoint, "http://localhost:4317");
        assert_eq!(cfg.service_name.as_deref(), Some("my-service"));
    }

    #[test]
    fn service_name_defaults_to_none_when_absent() {
        let cfg: OpenTelemetryConfig =
            toml::from_str("endpoint = \"http://localhost:4317\"").expect("valid otel config");
        assert_eq!(cfg.service_name, None);
    }

    #[test]
    fn endpoint_is_required() {
        let result = toml::from_str::<OpenTelemetryConfig>("service_name = \"x\"");
        assert!(result.is_err(), "endpoint must be required");
    }

    #[test]
    fn builders_set_fields() {
        let cfg = OpenTelemetryConfig::new("http://collector:4317").with_service_name("svc");
        assert_eq!(cfg.endpoint, "http://collector:4317");
        assert_eq!(cfg.service_name.as_deref(), Some("svc"));
    }

    #[test]
    fn parses_when_nested_under_logging() {
        // Exercises the documented `[logging.opentelemetry]` placement.
        let config: crate::Config = "[logging.opentelemetry]\nendpoint = \"http://localhost:4317\""
            .parse()
            .expect("valid config");
        assert_eq!(
            config
                .logging
                .opentelemetry
                .as_ref()
                .map(|o| o.endpoint.as_str()),
            Some("http://localhost:4317")
        );
    }
}
