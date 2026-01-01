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

/// Log output format.
///
/// Determines how log messages are formatted for output.
///
/// # TOML Values
///
/// Use lowercase names in configuration: `json`, `default`, `compact`, `pretty`.
///
/// # Example
///
/// ```toml
/// [logging]
/// format = "json"  # Recommended for production
/// ```
#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// JSON format - structured logs ideal for log aggregation systems.
    /// Each log entry is a single JSON object with fields like `timestamp`, `level`, `message`, `target`.
    /// Recommended for production environments with centralized logging (ELK, Splunk, etc.).
    Json,

    /// Default human-readable format with full details.
    /// Includes timestamp, level, target, and message with colors (when supported).
    /// Best for development and debugging.
    #[default]
    Default,

    /// Compact single-line format.
    /// Shows level and message only, useful when space is limited.
    Compact,

    /// Pretty multi-line format with indentation.
    /// Similar to default but with better readability for complex log entries.
    /// Useful for development when examining detailed logs.
    Pretty,
}
