//! Error types and handling for the Axum service library.
//!
//! This module provides structured error responses with unique error codes and
//! automatic HTTP status code mapping. All errors implement `IntoResponse` and
//! automatically serialize to JSON.
//!
//! # Design
//!
//! This module uses an opaque `Error` struct paired with an `ErrorKind` enum,
//! following the `std::io::Error` pattern. This design provides API stability:
//! internal error sources can change without breaking consumers.
//!
//! # Example
//!
//! ```rust
//! use axum_conf::{Error, ErrorKind};
//!
//! // Create errors using convenience constructors
//! let error = Error::internal("Something went wrong");
//!
//! // Match on the error kind
//! match error.kind() {
//!     ErrorKind::Internal => println!("Internal error: {}", error),
//!     ErrorKind::Database => println!("Database error: {}", error),
//!     _ => println!("Other error: {}", error),
//! }
//!
//! // Get the HTTP status code
//! use axum::http::StatusCode;
//! assert_eq!(error.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
//! ```

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::fmt;
use thiserror::Error;

/// The kind of error that occurred.
///
/// This enum categorizes errors for matching purposes. Use `Error::kind()`
/// to get the kind of an error.
///
/// # Stability
///
/// This enum is marked `#[non_exhaustive]`, so new variants may be added
/// in future versions without breaking existing code. Always include a
/// wildcard arm when matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Database error (connection, query, pool issues).
    #[error("database error")]
    Database,

    /// Authentication or authorization error.
    #[error("authentication error")]
    Authentication,

    /// Configuration error (invalid TOML, missing values).
    #[error("configuration error")]
    Configuration,

    /// TLS/certificate error.
    #[error("TLS error")]
    Tls,

    /// I/O error (file operations, network).
    #[error("I/O error")]
    Io,

    /// Invalid input (bad URL, header, request data).
    #[error("invalid input")]
    InvalidInput,

    /// Circuit breaker is open, rejecting requests.
    #[error("circuit breaker open")]
    CircuitBreakerOpen,

    /// Call through circuit breaker failed.
    #[error("circuit breaker call failed")]
    CircuitBreakerFailed,

    /// Internal/unexpected error.
    #[error("internal error")]
    Internal,
}

/// An error that can occur in the axum-conf library.
///
/// This is an opaque error type that wraps an underlying error source.
/// Use [`Error::kind()`] to determine the category of error for matching,
/// and the `Display` implementation to get a human-readable message.
///
/// # Creating Errors
///
/// Use the convenience constructors for common cases:
///
/// ```rust
/// use axum_conf::Error;
///
/// let err = Error::internal("unexpected state");
/// let err = Error::invalid_input("missing required field");
/// let err = Error::database("connection timeout");
/// ```
///
/// Or use [`Error::new()`] for full control:
///
/// ```rust
/// use axum_conf::{Error, ErrorKind};
///
/// let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
/// let err = Error::new(ErrorKind::Io, io_err);
/// ```
pub struct Error {
    kind: ErrorKind,
    source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl Error {
    /// Creates a new error with the given kind and source.
    ///
    /// # Example
    ///
    /// ```rust
    /// use axum_conf::{Error, ErrorKind};
    ///
    /// let err = Error::new(ErrorKind::Internal, "something went wrong");
    /// assert_eq!(err.kind(), ErrorKind::Internal);
    /// ```
    pub fn new<E>(kind: ErrorKind, error: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        Self {
            kind,
            source: error.into(),
        }
    }

    /// Returns the kind of this error.
    ///
    /// Use this to match on error categories:
    ///
    /// ```rust
    /// use axum_conf::{Error, ErrorKind};
    ///
    /// fn handle_error(err: Error) {
    ///     match err.kind() {
    ///         ErrorKind::Database => eprintln!("Database issue, will retry"),
    ///         ErrorKind::InvalidInput => eprintln!("Bad request"),
    ///         _ => eprintln!("Unexpected error"),
    ///     }
    /// }
    /// ```
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Returns the error code string for this error.
    ///
    /// This is a stable identifier suitable for client-side error handling.
    pub fn error_code(&self) -> &'static str {
        match self.kind {
            ErrorKind::Database => "DATABASE_ERROR",
            ErrorKind::Authentication => "AUTH_ERROR",
            ErrorKind::Configuration => "CONFIG_ERROR",
            ErrorKind::Tls => "TLS_ERROR",
            ErrorKind::Io => "IO_ERROR",
            ErrorKind::InvalidInput => "INVALID_INPUT",
            ErrorKind::CircuitBreakerOpen => "CIRCUIT_BREAKER_OPEN",
            ErrorKind::CircuitBreakerFailed => "CIRCUIT_BREAKER_CALL_FAILED",
            ErrorKind::Internal => "INTERNAL_ERROR",
        }
    }

    /// Returns the HTTP status code for this error.
    pub fn status_code(&self) -> StatusCode {
        match self.kind {
            ErrorKind::Database => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::Authentication => StatusCode::UNAUTHORIZED,
            ErrorKind::Configuration => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorKind::Tls => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorKind::Io => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            ErrorKind::CircuitBreakerOpen => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::CircuitBreakerFailed => StatusCode::BAD_GATEWAY,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Converts the error into a structured error response.
    pub fn to_error_response(&self) -> ErrorResponse {
        ErrorResponse::new(self.error_code(), self.to_string())
    }

    /// Consumes the error and returns the inner error source.
    pub fn into_inner(self) -> Box<dyn std::error::Error + Send + Sync + 'static> {
        self.source
    }
}

// ============================================================================
// Convenience constructors
// ============================================================================

impl Error {
    /// Creates a database error.
    pub fn database(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Database, msg.into())
    }

    /// Creates a database configuration error.
    ///
    /// This is a convenience method that creates a `Database` kind error
    /// with a "Database configuration error" prefix.
    pub fn database_config(msg: impl Into<String>) -> Self {
        Self::new(
            ErrorKind::Database,
            format!("Database configuration error: {}", msg.into()),
        )
    }

    /// Creates an authentication error.
    pub fn authentication(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Authentication, msg.into())
    }

    /// Creates a configuration error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Configuration, msg.into())
    }

    /// Creates a TLS error.
    pub fn tls(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Tls, msg.into())
    }

    /// Creates an I/O error from a message.
    pub fn io(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, msg.into())
    }

    /// Creates an I/O error from a `std::io::Error`.
    pub fn from_io(err: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, err)
    }

    /// Creates an invalid input error.
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, msg.into())
    }

    /// Creates a circuit breaker open error.
    #[cfg(feature = "circuit-breaker")]
    pub fn circuit_breaker_open(target: impl Into<String>) -> Self {
        Self::new(
            ErrorKind::CircuitBreakerOpen,
            format!("Circuit breaker open for target: {}", target.into()),
        )
    }

    /// Creates a circuit breaker call failed error.
    #[cfg(feature = "circuit-breaker")]
    pub fn circuit_breaker_failed(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::CircuitBreakerFailed, msg.into())
    }

    /// Creates an internal error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, msg.into())
    }
}

// ============================================================================
// Trait implementations
// ============================================================================

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("source", &self.source)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_response = self.to_error_response();

        tracing::error!(
            error_code = %error_response.error_code,
            message = %error_response.message,
            status = %status.as_u16(),
            "Error occurred"
        );

        (status, Json(error_response)).into_response()
    }
}

// ============================================================================
// From implementations
// ============================================================================

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Self::new(ErrorKind::Configuration, err)
    }
}

impl From<url::ParseError> for Error {
    fn from(err: url::ParseError) -> Self {
        Self::new(ErrorKind::InvalidInput, err)
    }
}

impl From<std::env::VarError> for Error {
    fn from(err: std::env::VarError) -> Self {
        Self::new(ErrorKind::Configuration, err)
    }
}

impl From<http::header::InvalidHeaderValue> for Error {
    fn from(err: http::header::InvalidHeaderValue) -> Self {
        Self::new(ErrorKind::InvalidInput, err)
    }
}

#[cfg(feature = "postgres")]
impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Self::new(ErrorKind::Database, err)
    }
}

#[cfg(feature = "rustls")]
impl From<rustls::Error> for Error {
    fn from(err: rustls::Error) -> Self {
        Self::new(ErrorKind::Tls, err)
    }
}

#[cfg(feature = "keycloak")]
impl From<axum_keycloak_auth::error::AuthError> for Error {
    fn from(err: axum_keycloak_auth::error::AuthError) -> Self {
        Self::new(ErrorKind::Authentication, err)
    }
}

// ============================================================================
// ErrorResponse
// ============================================================================

/// Structured error response with error code and details.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorResponse {
    /// Unique error code for client-side error handling.
    pub error_code: String,
    /// Human-readable error message.
    pub message: String,
    /// Optional additional details about the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ErrorResponse {
    /// Creates a new error response.
    pub fn new(error_code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_code: error_code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Adds details to the error response.
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;

    // ========================================================================
    // ErrorKind tests
    // ========================================================================

    #[test]
    fn test_error_kind_equality() {
        assert_eq!(ErrorKind::Database, ErrorKind::Database);
        assert_ne!(ErrorKind::Database, ErrorKind::Internal);
    }

    #[test]
    fn test_error_kind_display() {
        assert_eq!(format!("{}", ErrorKind::Database), "database error");
        assert_eq!(format!("{}", ErrorKind::Internal), "internal error");
        assert_eq!(format!("{}", ErrorKind::InvalidInput), "invalid input");
    }

    #[test]
    fn test_error_kind_clone() {
        let kind = ErrorKind::Database;
        let cloned = kind;
        assert_eq!(kind, cloned);
    }

    // ========================================================================
    // Error constructor tests
    // ========================================================================

    #[test]
    fn test_error_new() {
        let err = Error::new(ErrorKind::Internal, "test error");
        assert_eq!(err.kind(), ErrorKind::Internal);
        assert_eq!(format!("{}", err), "test error");
    }

    #[test]
    fn test_error_database() {
        let err = Error::database("connection failed");
        assert_eq!(err.kind(), ErrorKind::Database);
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_error_database_config() {
        let err = Error::database_config("invalid URL");
        assert_eq!(err.kind(), ErrorKind::Database);
        assert!(err.to_string().contains("Database configuration error"));
        assert!(err.to_string().contains("invalid URL"));
    }

    #[test]
    fn test_error_authentication() {
        let err = Error::authentication("invalid token");
        assert_eq!(err.kind(), ErrorKind::Authentication);
        assert!(err.to_string().contains("invalid token"));
    }

    #[test]
    fn test_error_config() {
        let err = Error::config("missing field");
        assert_eq!(err.kind(), ErrorKind::Configuration);
        assert!(err.to_string().contains("missing field"));
    }

    #[test]
    fn test_error_tls() {
        let err = Error::tls("certificate expired");
        assert_eq!(err.kind(), ErrorKind::Tls);
        assert!(err.to_string().contains("certificate expired"));
    }

    #[test]
    fn test_error_io() {
        let err = Error::io("file not found");
        assert_eq!(err.kind(), ErrorKind::Io);
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_error_invalid_input() {
        let err = Error::invalid_input("bad request");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(err.to_string().contains("bad request"));
    }

    #[test]
    fn test_error_internal() {
        let err = Error::internal("unexpected state");
        assert_eq!(err.kind(), ErrorKind::Internal);
        assert!(err.to_string().contains("unexpected state"));
    }

    #[cfg(feature = "circuit-breaker")]
    #[test]
    fn test_error_circuit_breaker_open() {
        let err = Error::circuit_breaker_open("payment-api");
        assert_eq!(err.kind(), ErrorKind::CircuitBreakerOpen);
        assert!(err.to_string().contains("payment-api"));
    }

    #[cfg(feature = "circuit-breaker")]
    #[test]
    fn test_error_circuit_breaker_failed() {
        let err = Error::circuit_breaker_failed("timeout");
        assert_eq!(err.kind(), ErrorKind::CircuitBreakerFailed);
        assert!(err.to_string().contains("timeout"));
    }

    // ========================================================================
    // Error code tests
    // ========================================================================

    #[test]
    fn test_error_code_database() {
        let err = Error::database("test");
        assert_eq!(err.error_code(), "DATABASE_ERROR");
    }

    #[test]
    fn test_error_code_authentication() {
        let err = Error::authentication("test");
        assert_eq!(err.error_code(), "AUTH_ERROR");
    }

    #[test]
    fn test_error_code_config() {
        let err = Error::config("test");
        assert_eq!(err.error_code(), "CONFIG_ERROR");
    }

    #[test]
    fn test_error_code_tls() {
        let err = Error::tls("test");
        assert_eq!(err.error_code(), "TLS_ERROR");
    }

    #[test]
    fn test_error_code_io() {
        let err = Error::io("test");
        assert_eq!(err.error_code(), "IO_ERROR");
    }

    #[test]
    fn test_error_code_invalid_input() {
        let err = Error::invalid_input("test");
        assert_eq!(err.error_code(), "INVALID_INPUT");
    }

    #[test]
    fn test_error_code_internal() {
        let err = Error::internal("test");
        assert_eq!(err.error_code(), "INTERNAL_ERROR");
    }

    #[cfg(feature = "circuit-breaker")]
    #[test]
    fn test_error_code_circuit_breaker_open() {
        let err = Error::circuit_breaker_open("test");
        assert_eq!(err.error_code(), "CIRCUIT_BREAKER_OPEN");
    }

    #[cfg(feature = "circuit-breaker")]
    #[test]
    fn test_error_code_circuit_breaker_failed() {
        let err = Error::circuit_breaker_failed("test");
        assert_eq!(err.error_code(), "CIRCUIT_BREAKER_CALL_FAILED");
    }

    // ========================================================================
    // Status code tests
    // ========================================================================

    #[test]
    fn test_status_code_database() {
        let err = Error::database("test");
        assert_eq!(err.status_code(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn test_status_code_authentication() {
        let err = Error::authentication("test");
        assert_eq!(err.status_code(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_status_code_config() {
        let err = Error::config("test");
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_status_code_tls() {
        let err = Error::tls("test");
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_status_code_io() {
        let err = Error::io("test");
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_status_code_invalid_input() {
        let err = Error::invalid_input("test");
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_status_code_internal() {
        let err = Error::internal("test");
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[cfg(feature = "circuit-breaker")]
    #[test]
    fn test_status_code_circuit_breaker_open() {
        let err = Error::circuit_breaker_open("test");
        assert_eq!(err.status_code(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[cfg(feature = "circuit-breaker")]
    #[test]
    fn test_status_code_circuit_breaker_failed() {
        let err = Error::circuit_breaker_failed("test");
        assert_eq!(err.status_code(), StatusCode::BAD_GATEWAY);
    }

    // ========================================================================
    // From trait tests
    // ========================================================================

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: Error = io_err.into();
        assert_eq!(err.kind(), ErrorKind::Io);
    }

    #[test]
    fn test_from_toml_error() {
        let toml_err = "invalid".parse::<toml::Value>().unwrap_err();
        let err: Error = toml_err.into();
        assert_eq!(err.kind(), ErrorKind::Configuration);
    }

    #[test]
    fn test_from_url_parse_error() {
        let url_err = url::Url::parse("not a url").unwrap_err();
        let err: Error = url_err.into();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn test_from_var_error() {
        let var_err = std::env::VarError::NotPresent;
        let err: Error = var_err.into();
        assert_eq!(err.kind(), ErrorKind::Configuration);
    }

    #[test]
    fn test_from_invalid_header() {
        let header_err = http::header::HeaderValue::from_bytes(b"\x00").unwrap_err();
        let err: Error = header_err.into();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    // ========================================================================
    // ErrorResponse tests
    // ========================================================================

    #[test]
    fn test_error_response_new() {
        let response = ErrorResponse::new("TEST_CODE", "Test message");
        assert_eq!(response.error_code, "TEST_CODE");
        assert_eq!(response.message, "Test message");
        assert!(response.details.is_none());
    }

    #[test]
    fn test_error_response_with_details() {
        let response = ErrorResponse::new("CODE", "message").with_details("extra info");
        assert_eq!(response.error_code, "CODE");
        assert_eq!(response.message, "message");
        assert_eq!(response.details, Some("extra info".to_string()));
    }

    #[test]
    fn test_to_error_response() {
        let err = Error::internal("Something went wrong");
        let response = err.to_error_response();
        assert_eq!(response.error_code, "INTERNAL_ERROR");
        assert!(response.message.contains("Something went wrong"));
    }

    // ========================================================================
    // Misc trait tests
    // ========================================================================

    #[test]
    fn test_error_debug() {
        let err = Error::internal("test");
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Error"));
        assert!(debug_str.contains("Internal"));
    }

    #[test]
    fn test_error_display() {
        let err = Error::internal("my error message");
        assert_eq!(format!("{}", err), "my error message");
    }

    #[test]
    fn test_error_into_inner() {
        let err = Error::internal("test message");
        let inner = err.into_inner();
        assert_eq!(format!("{}", inner), "test message");
    }

    #[test]
    fn test_error_source_trait() {
        let err = Error::internal("test");
        assert!(StdError::source(&err).is_some());
    }
}
