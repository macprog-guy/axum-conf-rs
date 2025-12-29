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
