//! Bearer JWT token validation using JWKS.
//!
//! Replaces `axum-keycloak-auth` with direct JWT validation via `jsonwebtoken`.
//! Fetches JWKS from the OIDC provider at startup and refreshes periodically.

use std::sync::Arc;

use axum::{extract::Request, middleware::Next, response::{IntoResponse, Response}};
use http::StatusCode;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use tokio::sync::RwLock;

use crate::{AuthMethod, AuthenticatedIdentity, Error, Result, utils::Sensitive};

/// Configuration for Bearer JWT validation.
#[derive(Clone)]
pub(crate) struct BearerAuthConfig {
    pub audiences: Vec<String>,
    pub issuer: String,
    pub passthrough: bool,
    pub roles_claim: String,
}

/// Provides JWKS keys for JWT validation, with periodic refresh.
pub(crate) struct JwksProvider {
    jwks: RwLock<JwkSet>,
    jwks_url: String,
    http_client: openidconnect::reqwest::Client,
}

impl JwksProvider {
    /// Create a new provider, fetching the initial JWKS.
    pub async fn new(jwks_url: String) -> Result<Arc<Self>> {
        let http_client = openidconnect::reqwest::Client::new();
        let jwks = Self::fetch_jwks(&http_client, &jwks_url).await?;

        let provider = Arc::new(Self {
            jwks: RwLock::new(jwks),
            jwks_url,
            http_client,
        });

        // Spawn background refresh task
        let weak = Arc::downgrade(&provider);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await; // skip immediate first tick
            loop {
                interval.tick().await;
                let Some(provider) = weak.upgrade() else {
                    break;
                };
                if let Err(e) = provider.refresh().await {
                    tracing::warn!(error = %e, "JWKS background refresh failed");
                }
            }
        });

        Ok(provider)
    }

    /// Re-fetch JWKS from the provider.
    pub async fn refresh(&self) -> Result<()> {
        let jwks = Self::fetch_jwks(&self.http_client, &self.jwks_url).await?;
        *self.jwks.write().await = jwks;
        tracing::debug!("JWKS refreshed successfully");
        Ok(())
    }

    async fn fetch_jwks(
        client: &openidconnect::reqwest::Client,
        url: &str,
    ) -> Result<JwkSet> {
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::config(format!("JWKS fetch failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(Error::config(format!(
                "JWKS fetch returned status {}",
                resp.status()
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| Error::config(format!("JWKS response read failed: {e}")))?;

        serde_json::from_str::<JwkSet>(&body)
            .map_err(|e| Error::config(format!("JWKS deserialization failed: {e}")))
    }

    /// Validate a Bearer JWT token and return the decoded claims.
    pub async fn validate_token(
        self: &Arc<Self>,
        token: &str,
        config: &BearerAuthConfig,
    ) -> std::result::Result<serde_json::Value, TokenError> {
        let header = decode_header(token).map_err(|e| {
            tracing::debug!(error = %e, "JWT header decode failed");
            TokenError::Invalid
        })?;

        let kid = header.kid.as_deref().ok_or_else(|| {
            tracing::debug!("JWT missing kid in header");
            TokenError::Invalid
        })?;

        // Try to find key, refresh once if not found (key rotation)
        let claims = match self.try_decode(token, kid, header.alg, config).await {
            Ok(claims) => claims,
            Err(TokenError::KeyNotFound) => {
                tracing::debug!(kid, "Key not found in JWKS, refreshing");
                self.refresh().await.map_err(|_| TokenError::Invalid)?;
                self.try_decode(token, kid, header.alg, config).await?
            }
            Err(e) => return Err(e),
        };

        Ok(claims)
    }

    async fn try_decode(
        &self,
        token: &str,
        kid: &str,
        alg: Algorithm,
        config: &BearerAuthConfig,
    ) -> std::result::Result<serde_json::Value, TokenError> {
        let jwks = self.jwks.read().await;

        let jwk = jwks.find(kid).ok_or(TokenError::KeyNotFound)?;

        let key = DecodingKey::from_jwk(jwk).map_err(|e| {
            tracing::debug!(error = %e, kid, "Failed to create decoding key from JWK");
            TokenError::Invalid
        })?;

        // Use the algorithm from the JWT header. Security is enforced by jsonwebtoken:
        // it verifies the key's algorithm family matches the header's algorithm, preventing
        // alg-confusion attacks (e.g., an RSA key won't verify an HS256 signature).
        let mut validation = Validation::new(alg);

        if !config.audiences.is_empty() {
            validation.set_audience(&config.audiences);
        } else {
            validation.validate_aud = false;
        }
        validation.set_issuer(&[&config.issuer]);

        let token_data = decode::<serde_json::Value>(token, &key, &validation).map_err(|e| {
            tracing::debug!(
                error = %e,
                expected_issuer = %config.issuer,
                expected_audiences = ?config.audiences,
                kid,
                alg = ?alg,
                "JWT validation failed"
            );
            TokenError::Invalid
        })?;

        Ok(token_data.claims)
    }
}

#[derive(Debug)]
pub(crate) enum TokenError {
    KeyNotFound,
    Invalid,
}

/// Extract `AuthenticatedIdentity` from decoded JWT claims.
pub(crate) fn claims_to_identity(
    claims: &serde_json::Value,
    roles_claim: &str,
    access_token: Option<&str>,
) -> Option<AuthenticatedIdentity> {
    let sub = claims.get("sub")?.as_str()?.to_string();

    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    let preferred_username = claims
        .get("preferred_username")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    // Keycloak realm roles → groups
    let groups: Vec<String> = claims
        .get("realm_access")
        .and_then(|ra| ra.get("roles"))
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Application-specific roles from configurable claim
    let roles: Vec<String> = claims
        .get(roles_claim)
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    tracing::debug!(
        subject = %sub,
        email = ?email,
        preferred_username = ?preferred_username,
        group_count = groups.len(),
        role_count = roles.len(),
        "bearer token claims mapped to identity"
    );

    Some(AuthenticatedIdentity {
        method: AuthMethod::Oidc,
        user: sub,
        email,
        groups,
        roles,
        preferred_username,
        access_token: access_token.map(Sensitive::from),
    })
}

/// Bearer token validation middleware.
///
/// Extracts and validates `Authorization: Bearer <token>` JWTs.
/// In passthrough mode, requests without a Bearer token pass through.
/// In block mode, missing tokens return 401.
pub(crate) async fn bearer_auth_middleware(
    jwks: Arc<JwksProvider>,
    config: Arc<BearerAuthConfig>,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer_token = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let Some(token) = bearer_token else {
        if config.passthrough {
            return next.run(request).await;
        }
        return (StatusCode::UNAUTHORIZED, "Missing Bearer token").into_response();
    };

    match jwks.validate_token(token, &config).await {
        Ok(claims) => {
            if let Some(identity) = claims_to_identity(&claims, &config.roles_claim, Some(token)) {
                request.extensions_mut().insert(identity);
            }
            next.run(request).await
        }
        Err(e) => {
            tracing::debug!(?e, "Bearer token validation failed");
            if config.passthrough {
                return next.run(request).await;
            }
            (StatusCode::UNAUTHORIZED, "Invalid Bearer token").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claims_to_identity_full() {
        let claims = serde_json::json!({
            "sub": "user-123",
            "email": "user@example.com",
            "preferred_username": "jdoe",
            "realm_access": {
                "roles": ["admin", "user"]
            },
            "applicationRoles": ["editor", "viewer"]
        });

        let identity = claims_to_identity(&claims, "applicationRoles", None).unwrap();
        assert_eq!(identity.user, "user-123");
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
        assert_eq!(identity.preferred_username.as_deref(), Some("jdoe"));
        assert_eq!(identity.groups, vec!["admin", "user"]);
        assert_eq!(identity.roles, vec!["editor", "viewer"]);
        assert!(matches!(identity.method, AuthMethod::Oidc));
        assert!(identity.access_token.is_none());
    }

    #[test]
    fn test_claims_to_identity_minimal() {
        let claims = serde_json::json!({ "sub": "user-456" });

        let identity = claims_to_identity(&claims, "applicationRoles", None).unwrap();
        assert_eq!(identity.user, "user-456");
        assert!(identity.email.is_none());
        assert!(identity.preferred_username.is_none());
        assert!(identity.groups.is_empty());
        assert!(identity.roles.is_empty());
    }

    #[test]
    fn test_claims_to_identity_missing_sub() {
        let claims = serde_json::json!({ "email": "no-sub@example.com" });
        assert!(claims_to_identity(&claims, "applicationRoles", None).is_none());
    }

    #[test]
    fn test_claims_to_identity_empty_email_is_none() {
        let claims = serde_json::json!({ "sub": "user", "email": "" });
        let identity = claims_to_identity(&claims, "applicationRoles", None).unwrap();
        assert!(identity.email.is_none());
    }

    #[test]
    fn test_claims_to_identity_custom_roles_claim() {
        let claims = serde_json::json!({
            "sub": "user-custom",
            "myAppRoles": ["admin", "manager"]
        });

        let identity = claims_to_identity(&claims, "myAppRoles", None).unwrap();
        assert_eq!(identity.roles, vec!["admin", "manager"]);
    }

    #[test]
    fn test_claims_to_identity_empty_realm_roles() {
        let claims = serde_json::json!({
            "sub": "user",
            "realm_access": { "roles": [] }
        });

        let identity = claims_to_identity(&claims, "applicationRoles", None).unwrap();
        assert!(identity.groups.is_empty());
    }
}
