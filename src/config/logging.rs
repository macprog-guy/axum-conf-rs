use {crate::Result, serde::Deserialize};

#[cfg(feature = "opentelemetry")]
use crate::config::opentelemetry::OpenTelemetryConfig;

///
/// Configuration for logging and tracing.
///
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoggingConfig {
    /// Format for log output.
    /// The default format is `default`, which is "full" human-readable format.
    /// Other options are `json`, `compact`, and `pretty`.
    pub format: LogFormat,

    /// OpenTelemetry configuration (optional).
    /// When configured, enables distributed tracing with OTLP export.
    #[cfg(feature = "opentelemetry")]
    #[serde(default)]
    pub opentelemetry: Option<OpenTelemetryConfig>,
}

impl LoggingConfig {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,
    #[default]
    Default,
    Compact,
    Pretty,
}
