//! HTTP Basic Auth and API Key authentication middleware.
//!
//! This module provides middleware for authenticating requests using either:
//! - HTTP Basic Auth (RFC 7617) with username/password credentials
//! - API Key authentication with a configurable header

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use std::sync::Arc;

use crate::{AuthMethod, AuthenticatedIdentity, BasicAuthMode, HttpBasicAuthConfig};

/// Authenticates a request using HTTP Basic Auth or API Key.
///
/// Returns the authenticated identity on success, or an error response on failure.
#[allow(clippy::result_large_err)]
pub(crate) fn authenticate(
    config: &HttpBasicAuthConfig,
    headers: &HeaderMap,
) -> Result<AuthenticatedIdentity, Response> {
    // Try Basic Auth first (if mode allows)
    if matches!(config.mode, BasicAuthMode::Basic | BasicAuthMode::Either)
        && let Some(identity) = try_basic_auth(config, headers)?
    {
        return Ok(identity);
    }

    // Try API Key (if mode allows)
    if matches!(config.mode, BasicAuthMode::ApiKey | BasicAuthMode::Either)
        && let Some(identity) = try_api_key_auth(config, headers)?
    {
        return Ok(identity);
    }

    // No valid authentication found
    Err(unauthorized_response(config))
}

/// Attempts to authenticate using HTTP Basic Auth.
#[allow(clippy::result_large_err)]
fn try_basic_auth(
    config: &HttpBasicAuthConfig,
    headers: &HeaderMap,
) -> Result<Option<AuthenticatedIdentity>, Response> {
    let auth_header = match headers.get(AUTHORIZATION) {
        Some(h) => h.to_str().map_err(|_| bad_request_response())?,
        None => return Ok(None),
    };

    if !auth_header.starts_with("Basic ") {
        return Ok(None);
    }

    let encoded = &auth_header[6..];
    let decoded = BASE64.decode(encoded).map_err(|_| bad_request_response())?;
    let credentials = String::from_utf8(decoded).map_err(|_| bad_request_response())?;

    let (username, password) = credentials
        .split_once(':')
        .ok_or_else(bad_request_response)?;

    // Find matching user and verify password
    for user in &config.users {
        if user.username == username && constant_time_compare(password.as_bytes(), user.password.0.as_bytes()) {
            return Ok(Some(AuthenticatedIdentity {
                method: AuthMethod::BasicAuth,
                name: username.to_string(),
            }));
        }
    }

    Ok(None)
}

/// Attempts to authenticate using API Key.
#[allow(clippy::result_large_err)]
fn try_api_key_auth(
    config: &HttpBasicAuthConfig,
    headers: &HeaderMap,
) -> Result<Option<AuthenticatedIdentity>, Response> {
    let api_key = match headers.get(&config.api_key_header) {
        Some(h) => h.to_str().map_err(|_| bad_request_response())?,
        None => return Ok(None),
    };

    // Find matching API key (constant-time comparison)
    for key_config in &config.api_keys {
        if constant_time_compare(api_key.as_bytes(), key_config.key.0.as_bytes()) {
            return Ok(Some(AuthenticatedIdentity {
                method: AuthMethod::ApiKey,
                name: key_config
                    .name
                    .clone()
                    .unwrap_or_else(|| "api-key".to_string()),
            }));
        }
    }

    Ok(None)
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Creates an unauthorized response with WWW-Authenticate header.
fn unauthorized_response(config: &HttpBasicAuthConfig) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Body::from("Authentication required"),
    )
        .into_response();

    // Add WWW-Authenticate header for Basic Auth
    if matches!(config.mode, BasicAuthMode::Basic | BasicAuthMode::Either) {
        response.headers_mut().insert(
            "WWW-Authenticate",
            "Basic realm=\"API\"".parse().unwrap(),
        );
    }

    response
}

/// Creates a bad request response for malformed auth headers.
fn bad_request_response() -> Response {
    (StatusCode::BAD_REQUEST, Body::from("Invalid authentication format")).into_response()
}

/// Basic authentication middleware function.
///
/// This middleware:
/// - Extracts credentials from Authorization header (Basic Auth) or API Key header
/// - Validates credentials against configured users/keys
/// - Inserts `AuthenticatedIdentity` into request extensions on success
/// - Returns 401 Unauthorized on failure
pub(crate) async fn basic_auth_middleware(
    config: Arc<HttpBasicAuthConfig>,
    mut request: Request,
    next: axum::middleware::Next,
) -> Response {
    match authenticate(&config, request.headers()) {
        Ok(identity) => {
            tracing::debug!(
                method = ?identity.method,
                name = %identity.name,
                "Request authenticated"
            );
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Err(response) => {
            tracing::warn!("Authentication failed");
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BasicAuthApiKey, BasicAuthUser, utils::Sensitive};

    fn test_config() -> HttpBasicAuthConfig {
        HttpBasicAuthConfig {
            mode: BasicAuthMode::Either,
            api_key_header: "X-API-Key".to_string(),
            users: vec![BasicAuthUser {
                username: "testuser".to_string(),
                password: Sensitive::from("testpass"),
            }],
            api_keys: vec![BasicAuthApiKey {
                key: Sensitive::from("test-api-key-12345"),
                name: Some("test-key".to_string()),
            }],
        }
    }

    fn basic_auth_header() -> HeaderMap {
        let mut headers = HeaderMap::new();
        let credentials = BASE64.encode("testuser:testpass");
        headers.insert(
            AUTHORIZATION,
            format!("Basic {}", credentials).parse().unwrap(),
        );
        headers
    }

    fn api_key_header() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "test-api-key-12345".parse().unwrap());
        headers
    }

    #[test]
    fn test_basic_auth_valid() {
        let config = test_config();
        let headers = basic_auth_header();

        let result = authenticate(&config, &headers);
        assert!(result.is_ok());

        let identity = result.unwrap();
        assert_eq!(identity.method, AuthMethod::BasicAuth);
        assert_eq!(identity.name, "testuser");
    }

    #[test]
    fn test_basic_auth_invalid_password() {
        let config = test_config();
        let mut headers = HeaderMap::new();
        let credentials = BASE64.encode("testuser:wrongpass");
        headers.insert(
            AUTHORIZATION,
            format!("Basic {}", credentials).parse().unwrap(),
        );

        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_basic_auth_invalid_username() {
        let config = test_config();
        let mut headers = HeaderMap::new();
        let credentials = BASE64.encode("wronguser:testpass");
        headers.insert(
            AUTHORIZATION,
            format!("Basic {}", credentials).parse().unwrap(),
        );

        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_api_key_valid() {
        let config = test_config();
        let headers = api_key_header();

        let result = authenticate(&config, &headers);
        assert!(result.is_ok());

        let identity = result.unwrap();
        assert_eq!(identity.method, AuthMethod::ApiKey);
        assert_eq!(identity.name, "test-key");
    }

    #[test]
    fn test_api_key_invalid() {
        let config = test_config();
        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "wrong-key".parse().unwrap());

        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_auth_headers() {
        let config = test_config();
        let headers = HeaderMap::new();

        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_basic_mode_rejects_api_key() {
        let mut config = test_config();
        config.mode = BasicAuthMode::Basic;

        let headers = api_key_header();
        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_api_key_mode_rejects_basic_auth() {
        let mut config = test_config();
        config.mode = BasicAuthMode::ApiKey;

        let headers = basic_auth_header();
        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_constant_time_compare_equal() {
        assert!(constant_time_compare(b"secret", b"secret"));
    }

    #[test]
    fn test_constant_time_compare_not_equal() {
        assert!(!constant_time_compare(b"secret", b"different"));
    }

    #[test]
    fn test_constant_time_compare_different_lengths() {
        assert!(!constant_time_compare(b"short", b"longer"));
    }

    #[test]
    fn test_malformed_basic_auth_header() {
        let config = test_config();
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Basic not-valid-base64!!!".parse().unwrap());

        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_basic_auth_missing_colon() {
        let config = test_config();
        let mut headers = HeaderMap::new();
        let credentials = BASE64.encode("usernameonly");
        headers.insert(
            AUTHORIZATION,
            format!("Basic {}", credentials).parse().unwrap(),
        );

        let result = authenticate(&config, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_api_key_without_name() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::ApiKey,
            api_key_header: "X-API-Key".to_string(),
            users: vec![],
            api_keys: vec![BasicAuthApiKey {
                key: Sensitive::from("nameless-key"),
                name: None,
            }],
        };

        let mut headers = HeaderMap::new();
        headers.insert("X-API-Key", "nameless-key".parse().unwrap());

        let result = authenticate(&config, &headers);
        assert!(result.is_ok());

        let identity = result.unwrap();
        assert_eq!(identity.name, "api-key");
    }

    #[test]
    fn test_custom_api_key_header() {
        let config = HttpBasicAuthConfig {
            mode: BasicAuthMode::ApiKey,
            api_key_header: "Authorization-Token".to_string(),
            users: vec![],
            api_keys: vec![BasicAuthApiKey {
                key: Sensitive::from("custom-key"),
                name: Some("custom".to_string()),
            }],
        };

        let mut headers = HeaderMap::new();
        headers.insert("Authorization-Token", "custom-key".parse().unwrap());

        let result = authenticate(&config, &headers);
        assert!(result.is_ok());

        let identity = result.unwrap();
        assert_eq!(identity.method, AuthMethod::ApiKey);
        assert_eq!(identity.name, "custom");
    }
}
