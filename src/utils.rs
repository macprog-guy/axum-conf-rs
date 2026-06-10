//!
//! Utility types and functions for common patterns in the service.
//!
//! This module provides:
//! - [`Sensitive`] - A wrapper type for sensitive data that hides values in debug output
//! - `RequestIdGenerator` - Generates or preserves request IDs for distributed tracing (internal)
//! - `replace_handlebars_with_env` - Template substitution for environment variables (internal)
//! - [`ApiVersion`] - API version extraction and management for versioned APIs
//!

use {
    http::{HeaderValue, Request},
    regex::{Captures, Regex},
    serde::Deserialize,
    std::{env, sync::LazyLock},
    tower_http::request_id::{MakeRequestId, RequestId},
    uuid::{ContextV7, Timestamp, Uuid},
    zeroize::{Zeroize, ZeroizeOnDrop},
};

/// Compiles a regex from a compile-time-constant pattern. The patterns in this
/// crate are fixed string literals, so compilation cannot fail at runtime — the
/// single `expect` is centralized here rather than at each call site.
fn compile_const_regex(pattern: &str) -> Regex {
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("constant regex pattern is valid")
}

/// Regular expression pattern for matching handlebars-style environment variable references.
/// Matches patterns like `{{ VAR_NAME }}` with optional whitespace around the variable name.
/// Variable names must be uppercase letters, digits, or underscores (standard env var naming).
static HANDLEBAR_REGEXP: LazyLock<Regex> =
    LazyLock::new(|| compile_const_regex(r"\{\{\s*([A-Z0-9_]+)\s*\}\}"));

/// A wrapper type for sensitive data that obscures the value in debug output
/// and securely zeros memory when dropped.
///
/// This type is useful for wrapping secrets, passwords, API keys, and other
/// sensitive information that should not be accidentally exposed in logs,
/// error messages, or debug output.
///
/// The inner value remains accessible through the public field `0`, but when
/// formatted using `Debug`, it displays as `Sensitive(****)` instead of the
/// actual value.
///
/// # Type Parameters
///
/// - `T`: The type of the sensitive value, which must implement `Default`
///
/// # Examples
///
/// ```
/// use axum_conf::Sensitive;
///
/// let api_key = Sensitive::from("secret-key-12345");
/// println!("{:?}", api_key);  // Prints: Sensitive(****)
///
/// // Access the actual value when needed
/// let key_value: &str = &api_key.0;
/// ```
///
/// # Security Features
///
/// - **Debug hiding**: Debug output shows `Sensitive(****)` instead of the value
/// - **Memory zeroing**: When `Sensitive<String>` is dropped, the memory is securely
///   overwritten with zeros to prevent secrets from lingering in memory
///
/// # Security Limitations
///
/// This type does NOT:
/// - Prevent the value from being read if you have access to the `Sensitive` instance
/// - Encrypt or secure the value in memory while in use
/// - Prevent the value from being serialized if using `Serialize`
/// - Prevent the compiler from copying the value (use with care in generic contexts)
///
/// For true security, combine with other security measures like secure memory handling.
///
/// # Derive Macros
///
/// Uses `ZeroizeOnDrop` from the `zeroize` crate to automatically zero memory when dropped.
#[derive(Clone, Deserialize, Default, Zeroize, ZeroizeOnDrop)]
pub struct Sensitive<T: Default + Zeroize>(pub T);

impl<T: Default + Zeroize> Sensitive<T> {
    /// Returns a reference to the wrapped secret.
    ///
    /// Use this to access the value deliberately (and greppably) rather than
    /// reaching for the public `.0` field. Take care not to copy the returned
    /// value into a location that outlives the `Sensitive` wrapper or that is
    /// logged/serialized.
    #[must_use]
    pub fn expose_secret(&self) -> &T {
        &self.0
    }
}

impl From<&str> for Sensitive<String> {
    /// Creates a `Sensitive<String>` from a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use axum_conf::Sensitive;
    ///
    /// let password = Sensitive::from("my-secret-password");
    /// ```
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl<T: Default + Zeroize + PartialEq> PartialEq for Sensitive<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Default + Zeroize> std::fmt::Debug for Sensitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sensitive(****)")
    }
}

/// Constant-time byte-slice equality, to avoid leaking secret *contents* through
/// comparison timing. Backed by the audited [`subtle`] crate. Note that, like any
/// such comparison over variable-length inputs, a length mismatch is detected
/// without a full scan (length is far less sensitive than contents).
#[must_use]
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

/// Returns whether `path` is a safe same-origin redirect target.
///
/// A safe target is a relative path beginning with a single `/`. This rejects
/// protocol-relative (`//evil.com`) and backslash-tricked (`/\evil.com`) forms,
/// which browsers resolve as cross-origin — the classic open-redirect vectors.
#[cfg(feature = "keycloak")]
#[must_use]
pub(crate) fn is_safe_local_path(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("//") && !path.starts_with("/\\")
}

/// Request ID generator for distributed tracing and request correlation.
///
/// This generator implements the `MakeRequestId` trait from `tower-http` to either:
/// 1. Preserve an existing `x-request-id` header from the incoming request, or
/// 2. Generate a new UUIDv7 if no request ID is present
///
/// Using UUIDv7 provides several benefits:
/// - Time-ordered: IDs are sortable by creation time
/// - Unique: Collision-resistant across distributed systems
/// - Traceable: Can correlate requests across multiple services
///
/// # Request ID Flow
///
/// ```text
/// Client Request
///     │
///     ├─ Has x-request-id header? ─> Preserve it
///     │
///     └─ No header? ─> Generate new UUIDv7
/// ```
///
/// # Usage
///
/// This is an internal type wired automatically by `setup_middleware()` (and by
/// `setup_request_id()`); it is not part of the public API. Conceptually it is
/// installed as:
///
/// ```ignore
/// use tower_http::request_id::SetRequestIdLayer;
/// let layer = SetRequestIdLayer::x_request_id(RequestIdGenerator);
/// ```
///
/// # Use Cases
///
/// - **Distributed Tracing**: Track a request across multiple microservices
/// - **Debugging**: Correlate logs from different components of a request
/// - **Auditing**: Track the lifecycle of a request for compliance
/// - **Monitoring**: Measure end-to-end request latency
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestIdGenerator;

impl MakeRequestId for RequestIdGenerator {
    /// Generates or extracts a request ID from an HTTP request.
    ///
    /// If the request already has an `x-request-id` header, that value is preserved.
    /// Otherwise, a new UUIDv7 is generated with high-precision timestamp context.
    ///
    /// # Arguments
    ///
    /// * `req` - The HTTP request to process
    ///
    /// # Returns
    ///
    /// An `Option<RequestId>` containing either the existing or newly generated ID.
    /// Returns `None` only if UUID generation or header value creation fails
    /// (which is extremely rare in practice).
    fn make_request_id<B>(&mut self, req: &Request<B>) -> Option<RequestId> {
        // Preserve a client-supplied id only if it is well-formed. An unvalidated
        // value flows into logs and trace correlation, so an attacker could
        // otherwise inject control characters or oversized junk (log injection /
        // correlation poisoning). Reject anything else and mint a fresh id.
        if let Some(value) = req.headers().get("x-request-id")
            && value.to_str().is_ok_and(is_valid_request_id)
        {
            return Some(RequestId::new(value.clone()));
        }

        let cx = ContextV7::new().with_additional_precision();
        let uuid = Uuid::new_v7(Timestamp::now(cx));
        let value = HeaderValue::from_str(&uuid.to_string()).ok()?;
        Some(RequestId::new(value))
    }
}

/// Whether a client-supplied request id is safe to preserve.
///
/// Accepts 1..=128 characters limited to ASCII alphanumerics and `-._:` —
/// enough for UUIDs and common trace-id formats, while excluding whitespace,
/// control characters, and newlines used in log-injection attacks.
fn is_valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b':'))
}

/// Replaces handlebars-style placeholders with environment variable values.
///
/// Searches through the input string for patterns like `{{ VAR_NAME }}` and replaces
/// them with the corresponding environment variable value. Variable names are
/// case-sensitive and must consist of uppercase letters, digits, or underscores.
///
/// Whitespace around the variable name is allowed: `{{VAR}}`, `{{ VAR }}`, and
/// `{{  VAR  }}` are all valid and equivalent.
///
/// # Arguments
///
/// * `input` - A string slice containing the template text with placeholders
///
/// # Returns
///
/// A new `String` with all placeholders replaced by their environment variable values.
/// If an environment variable is not set, it is replaced with an empty string.
///
/// # Examples
///
/// ```ignore
/// // Internal helper (crate-private). Given `HOME=/home/user`:
/// let result = replace_handlebars_with_env("Path: {{ HOME }}/config");
/// assert_eq!(result, "Path: /home/user/config");
///
/// // Missing variables become empty strings:
/// assert_eq!(replace_handlebars_with_env("Value: {{ MISSING_VAR }}"), "Value: ");
/// ```
///
/// # Use Cases
///
/// This function is primarily used for:
/// - **Configuration files**: Keep sensitive values out of TOML files
/// - **Connection strings**: Inject credentials from environment
/// - **Dynamic configuration**: Support different values per environment
///
/// # Pattern Details
///
/// The function uses a regular expression that matches:
/// - Opening braces: `{{`
/// - Optional whitespace: `\s*`
/// - Variable name: `[A-Z0-9_]+` (uppercase alphanumeric and underscores)
/// - Optional whitespace: `\s*`
/// - Closing braces: `}}`
///
/// # Security Considerations
///
/// - Environment variables are NOT encrypted in memory
/// - Substituted values appear in the returned string in plain text
/// - Consider using [`Sensitive`] wrapper for secrets after substitution
/// - Be cautious when logging or displaying the result
pub(crate) fn replace_handlebars_with_env(input: &str) -> String {
    HANDLEBAR_REGEXP
        .replace_all(input, |caps: &Captures| {
            let var_name = &caps[1];
            env::var(var_name).unwrap_or_else(|_| {
                tracing::warn!(
                    variable = %var_name,
                    "Environment variable not found, substituting with empty string"
                );
                String::new()
            })
        })
        .to_string()
}

/// API version extracted from request headers or path.
///
/// This type is used to track which version of the API a request is targeting.
/// It can be inserted into request extensions by versioning middleware and
/// extracted in handlers for version-specific logic.
///
/// # Examples
///
/// ```
/// use axum_conf::ApiVersion;
///
/// let version = ApiVersion::new(2);
/// assert_eq!(version.as_u32(), 2);
/// assert_eq!(version.to_string(), "v2");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion(u32);

impl ApiVersion {
    /// Creates a new API version
    pub fn new(version: u32) -> Self {
        Self(version)
    }

    /// Returns the version number as u32
    #[allow(unused)]
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Extracts API version from request path.
    ///
    /// Looks for patterns like `/v1/`, `/v2/`, `/api/v1/`, etc.
    pub fn from_path(path: &str) -> Option<Self> {
        static VERSION_PATH_REGEX: LazyLock<Regex> =
            LazyLock::new(|| compile_const_regex(r"/v(\d+)(?:/|$)"));

        VERSION_PATH_REGEX
            .captures(path)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .map(ApiVersion::new)
    }

    /// Extracts API version from request header.
    ///
    /// Supports headers like:
    /// - `X-API-Version: 2`
    /// - `Accept: application/vnd.api+json;version=2`
    pub fn from_header(header_value: &str) -> Option<Self> {
        // Try direct version number first (X-API-Version: 2)
        if let Ok(version) = header_value.trim().parse::<u32>() {
            return Some(ApiVersion::new(version));
        }

        // Try Accept header format (version=2)
        static VERSION_HEADER_REGEX: LazyLock<Regex> =
            LazyLock::new(|| compile_const_regex(r"version=(\d+)"));

        VERSION_HEADER_REGEX
            .captures(header_value)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .map(ApiVersion::new)
    }

    /// Extracts API version from query parameter.
    ///
    /// Looks for `?version=2` or `&version=2` in the query string.
    pub fn from_query(query: &str) -> Option<Self> {
        static VERSION_QUERY_REGEX: LazyLock<Regex> =
            LazyLock::new(|| compile_const_regex(r"[?&]version=(\d+)"));

        VERSION_QUERY_REGEX
            .captures(query)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .map(ApiVersion::new)
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl From<u32> for ApiVersion {
    fn from(version: u32) -> Self {
        ApiVersion::new(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn constant_time_eq_matches_eq() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"Secret"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(constant_time_eq(b"", b""));
    }

    #[cfg(feature = "keycloak")]
    #[test]
    fn is_safe_local_path_rejects_open_redirects() {
        // Same-origin relative paths are safe.
        assert!(is_safe_local_path("/"));
        assert!(is_safe_local_path("/dashboard?x=1"));
        // Protocol-relative and backslash-tricked targets are cross-origin.
        assert!(!is_safe_local_path("//evil.com"));
        assert!(!is_safe_local_path("/\\evil.com"));
        // Absolute and scheme-qualified URLs are not local paths.
        assert!(!is_safe_local_path("https://evil.com"));
        assert!(!is_safe_local_path("dashboard"));
        assert!(!is_safe_local_path(""));
    }

    #[test]
    fn test_is_valid_request_id() {
        // UUID and common trace-id shapes are accepted.
        assert!(is_valid_request_id("018f9c3e-7b2a-7e51-9d3a-1c2b3d4e5f60"));
        assert!(is_valid_request_id("trace_id-123.4:5"));
        assert!(is_valid_request_id(&"a".repeat(128)));

        // Rejected: empty, too long, whitespace/control chars, log-injection.
        assert!(!is_valid_request_id(""));
        assert!(!is_valid_request_id(&"a".repeat(129)));
        assert!(!is_valid_request_id("has space"));
        assert!(!is_valid_request_id("line\nbreak"));
        assert!(!is_valid_request_id("tab\there"));
    }

    // ========================================================================
    // Property-based tests for replace_handlebars_with_env
    // ========================================================================

    proptest! {
        /// Strings without handlebars patterns should pass through unchanged
        #[test]
        fn handlebars_no_pattern_unchanged(s in "[^{}]*") {
            // Input without any braces should be unchanged
            let result = replace_handlebars_with_env(&s);
            prop_assert_eq!(result, s);
        }

        /// The function should never panic on arbitrary input
        #[test]
        fn handlebars_never_panics(s in ".*") {
            // Just verify it doesn't panic - we don't care about the result
            let _ = replace_handlebars_with_env(&s);
        }

        /// Single braces should pass through unchanged
        #[test]
        fn handlebars_single_braces_unchanged(
            prefix in "[^{}]*",
            middle in "[^{}]*",
            suffix in "[^{}]*"
        ) {
            let input = format!("{prefix}{{{middle}}}{suffix}");
            let result = replace_handlebars_with_env(&input);
            // Single braces aren't our pattern, should be unchanged
            prop_assert_eq!(result, input);
        }

        /// Valid patterns with set env vars should be substituted
        #[test]
        fn handlebars_valid_pattern_substituted(
            var_name in "[A-Z][A-Z0-9_]{0,10}",
            var_value in "[a-zA-Z0-9_]{1,20}",
            prefix in "[^{}]{0,10}",
            suffix in "[^{}]{0,10}"
        ) {
            // Set up test env var with unique name to avoid conflicts
            let test_var = format!("PROPTEST_{var_name}");
            unsafe { std::env::set_var(&test_var, &var_value); }

            let input = format!("{prefix}{{{{ {test_var} }}}}{suffix}");
            let result = replace_handlebars_with_env(&input);
            let expected = format!("{prefix}{var_value}{suffix}");

            unsafe { std::env::remove_var(&test_var); }

            prop_assert_eq!(result, expected);
        }

        /// Multiple patterns in one string should all be substituted
        #[test]
        fn handlebars_multiple_patterns(
            var1 in "[A-Z][A-Z0-9_]{0,5}",
            var2 in "[A-Z][A-Z0-9_]{0,5}",
            val1 in "[a-z]{1,10}",
            val2 in "[a-z]{1,10}"
        ) {
            let test_var1 = format!("PROPTEST_MULTI1_{var1}");
            let test_var2 = format!("PROPTEST_MULTI2_{var2}");

            unsafe {
                std::env::set_var(&test_var1, &val1);
                std::env::set_var(&test_var2, &val2);
            }

            let input = format!("a={{{{ {test_var1} }}}} b={{{{ {test_var2} }}}}");
            let result = replace_handlebars_with_env(&input);
            let expected = format!("a={val1} b={val2}");

            unsafe {
                std::env::remove_var(&test_var1);
                std::env::remove_var(&test_var2);
            }

            prop_assert_eq!(result, expected);
        }

        /// Missing env vars should become empty strings
        #[test]
        fn handlebars_missing_var_empty(
            var_name in "[A-Z][A-Z0-9_]{5,15}"  // Use longer names to avoid collisions
        ) {
            let test_var = format!("PROPTEST_MISSING_{var_name}");
            // Ensure it's not set
            unsafe { std::env::remove_var(&test_var); }

            let input = format!("value={{{{ {test_var} }}}}");
            let result = replace_handlebars_with_env(&input);

            prop_assert_eq!(result, "value=");
        }
    }

    // ========================================================================
    // Property-based tests for ApiVersion
    // ========================================================================

    proptest! {
        /// ApiVersion round-trips through u32
        #[test]
        fn api_version_roundtrip(version in 0u32..1000) {
            let api_version = ApiVersion::new(version);
            prop_assert_eq!(api_version.as_u32(), version);
        }

        /// ApiVersion from_path extracts version correctly
        #[test]
        fn api_version_from_path(version in 1u32..100) {
            let path = format!("/v{version}/resource");
            let result = ApiVersion::from_path(&path);
            prop_assert_eq!(result, Some(ApiVersion::new(version)));
        }

        /// ApiVersion from_header with direct number
        #[test]
        fn api_version_from_header_direct(version in 1u32..100) {
            let header = format!("{version}");
            let result = ApiVersion::from_header(&header);
            prop_assert_eq!(result, Some(ApiVersion::new(version)));
        }

        /// ApiVersion from_header with version= format
        #[test]
        fn api_version_from_header_param(version in 1u32..100) {
            let header = format!("application/json; version={version}");
            let result = ApiVersion::from_header(&header);
            prop_assert_eq!(result, Some(ApiVersion::new(version)));
        }

        /// ApiVersion from_query extracts version correctly
        #[test]
        fn api_version_from_query(version in 1u32..100) {
            let query = format!("?foo=bar&version={version}&baz=qux");
            let result = ApiVersion::from_query(&query);
            prop_assert_eq!(result, Some(ApiVersion::new(version)));
        }

        /// ApiVersion Display format is correct
        #[test]
        fn api_version_display(version in 0u32..1000) {
            let api_version = ApiVersion::new(version);
            let display = api_version.to_string();
            prop_assert_eq!(display, format!("v{version}"));
        }
    }

    // ========================================================================
    // Property-based tests for Sensitive wrapper
    // ========================================================================

    proptest! {
        /// Sensitive wrapper preserves the inner value
        #[test]
        fn sensitive_preserves_value(s in ".*") {
            let sensitive = Sensitive::from(s.as_str());
            prop_assert_eq!(&sensitive.0, &s);
        }

        /// Sensitive Debug output never contains the actual value
        #[test]
        fn sensitive_debug_hides_value(s in "[a-zA-Z0-9]{1,50}") {
            let sensitive = Sensitive::from(s.as_str());
            let debug_output = format!("{:?}", sensitive);

            // Debug output should contain "****" and NOT the actual value
            prop_assert!(debug_output.contains("****"));
            // Only check non-trivial strings to avoid false positives
            if s.len() > 4 {
                prop_assert!(!debug_output.contains(&s));
            }
        }
    }

    // ========================================================================
    // Memory zeroing tests for Sensitive
    // ========================================================================

    #[test]
    fn sensitive_drop_zeros_memory() {
        // We can't directly inspect memory after drop in safe Rust,
        // but we can verify the Drop implementation runs without panicking
        // and that the inner value is accessible before drop.
        let secret = "super-secret-password-12345";
        let sensitive = Sensitive::from(secret);

        // Value is accessible before drop
        assert_eq!(sensitive.0, secret);

        // Drop runs without panic (zeroize is called)
        drop(sensitive);

        // If we got here, Drop ran successfully
    }

    #[test]
    fn sensitive_clone_creates_independent_copy() {
        let original = Sensitive::from("original-secret");
        let cloned = original.clone();

        // Both have the same value
        assert_eq!(original.0, cloned.0);

        // Dropping one doesn't affect the other
        drop(original);
        assert_eq!(cloned.0, "original-secret");
    }

    #[test]
    fn sensitive_zeroize_trait_is_used() {
        use zeroize::Zeroize;

        // Verify that String implements Zeroize (which is required by our Sensitive)
        let mut s = String::from("secret");
        s.zeroize();
        assert!(s.is_empty(), "Zeroize should clear the string");
    }
}
