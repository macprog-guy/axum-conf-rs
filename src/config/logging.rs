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
    #[serde(default)]
    pub format: LogFormat,

    /// OpenTelemetry configuration (optional).
    /// When configured, enables distributed tracing with OTLP export.
    #[cfg(feature = "opentelemetry")]
    #[serde(default)]
    pub opentelemetry: Option<OpenTelemetryConfig>,
}

impl LoggingConfig {
    /// Validates the logging configuration. Currently always succeeds; present
    /// for symmetry with the other config sections and future validation.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        logging: LoggingConfig,
    }

    fn parse(toml: &str) -> LoggingConfig {
        toml::from_str::<Wrapper>(toml)
            .expect("valid logging config")
            .logging
    }

    #[test]
    fn log_format_default_is_default_variant() {
        assert!(matches!(LogFormat::default(), LogFormat::Default));
        // An empty config yields the default format.
        assert!(matches!(parse("").format, LogFormat::Default));
    }

    #[test]
    fn log_format_parses_all_lowercase_variants() {
        assert!(matches!(
            parse("[logging]\nformat = \"json\"").format,
            LogFormat::Json
        ));
        assert!(matches!(
            parse("[logging]\nformat = \"compact\"").format,
            LogFormat::Compact
        ));
        assert!(matches!(
            parse("[logging]\nformat = \"pretty\"").format,
            LogFormat::Pretty
        ));
        assert!(matches!(
            parse("[logging]\nformat = \"default\"").format,
            LogFormat::Default
        ));
    }

    #[test]
    fn log_format_rejects_unknown_variant() {
        let result = toml::from_str::<Wrapper>("[logging]\nformat = \"verbose\"");
        assert!(result.is_err(), "unknown log format must be rejected");
    }

    #[test]
    fn validate_always_succeeds() {
        assert!(parse("[logging]\nformat = \"json\"").validate().is_ok());
    }
}
