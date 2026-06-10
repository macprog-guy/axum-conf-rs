//! Bearer JWT token validation using JWKS.
//!
//! Replaces `axum-keycloak-auth` with direct JWT validation via `jsonwebtoken`.
//! Fetches JWKS from the OIDC provider at startup and refreshes periodically.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use http::StatusCode;
use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{Jwk, JwkSet, KeyAlgorithm},
};
use tokio::sync::RwLock;

use crate::{AuthMethod, AuthenticatedIdentity, Error, Result, utils::Sensitive};

/// Configuration for the Bearer JWT middleware.
///
/// Validation parameters (issuer, audiences) live on the [`JwksProvider`]; this
/// only carries what the middleware itself needs.
#[derive(Clone)]
pub(crate) struct BearerAuthConfig {
    pub passthrough: bool,
    pub roles_claim: String,
}

/// A decoding key paired with its pre-built validation parameters.
///
/// Both are derived once when the JWKS is loaded/refreshed, so token validation
/// is a cheap map lookup instead of re-parsing the JWK and rebuilding
/// `Validation` (with its issuer/audience `Vec`s) on every request.
struct CachedKey {
    decoding_key: DecodingKey,
    validation: Validation,
}

/// Provides JWKS keys for JWT validation, with periodic refresh.
pub(crate) struct JwksProvider {
    /// Pre-built decode key + validation, keyed by JWK `kid`.
    keys: RwLock<HashMap<String, Arc<CachedKey>>>,
    jwks_url: String,
    http_client: openidconnect::reqwest::Client,
    /// Validation parameters, fixed for the provider's lifetime.
    issuer: String,
    audiences: Vec<String>,
    /// Whether the service runs in production; when `true` and no audiences are
    /// configured, the provider fails closed (accepts no Bearer tokens).
    is_production: bool,
}

impl JwksProvider {
    /// Create a new provider, fetching the initial JWKS.
    pub(crate) async fn new(
        jwks_url: String,
        issuer: String,
        audiences: Vec<String>,
        is_production: bool,
    ) -> Result<Arc<Self>> {
        let http_client = openidconnect::reqwest::Client::new();
        // Retry the initial fetch with bounded exponential backoff so a single
        // transient network hiccup at startup doesn't fail the whole service.
        let jwks = Self::fetch_jwks_with_retry(&http_client, &jwks_url).await?;
        let keys = Self::build_keys(&jwks, &issuer, &audiences, is_production);

        let provider = Arc::new(Self {
            keys: RwLock::new(keys),
            jwks_url,
            http_client,
            issuer,
            audiences,
            is_production,
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

    /// Test-only constructor: build a provider from an in-memory JWK set, with no
    /// network fetch and no background refresh task. The `jwks_url` points at an
    /// unreachable address so a refresh-on-key-miss fails (yielding `Unverifiable`).
    #[cfg(test)]
    fn from_keys_for_test(jwks: &JwkSet, issuer: &str, audiences: &[String]) -> Arc<Self> {
        let keys = Self::build_keys(jwks, issuer, audiences, false);
        Arc::new(Self {
            keys: RwLock::new(keys),
            jwks_url: "http://127.0.0.1:1/unreachable".to_string(),
            http_client: openidconnect::reqwest::Client::new(),
            issuer: issuer.to_string(),
            audiences: audiences.to_vec(),
            is_production: false,
        })
    }

    /// Re-fetch JWKS from the provider and rebuild the cached keys.
    pub(crate) async fn refresh(&self) -> Result<()> {
        let jwks = Self::fetch_jwks(&self.http_client, &self.jwks_url).await?;
        let keys = Self::build_keys(&jwks, &self.issuer, &self.audiences, self.is_production);
        *self.keys.write().await = keys;
        tracing::debug!("JWKS refreshed successfully");
        Ok(())
    }

    /// Build the per-`kid` decode key + validation cache from a fetched JWK set.
    ///
    /// When no audiences are configured, `aud` validation would otherwise be
    /// disabled (any token from the issuer accepted). In production that is
    /// unsafe, so the provider **fails closed**: it builds no keys, and every
    /// Bearer token is rejected until audiences are configured.
    fn build_keys(
        jwks: &JwkSet,
        issuer: &str,
        audiences: &[String],
        is_production: bool,
    ) -> HashMap<String, Arc<CachedKey>> {
        if audiences.is_empty() {
            if is_production {
                tracing::error!(
                    "OIDC audience validation cannot be disabled in production: no [http.oidc] \
                     audiences are configured. Refusing all Bearer tokens (fail-closed) — set \
                     audiences to enable Bearer authentication."
                );
                return HashMap::new();
            }
            tracing::warn!(
                "OIDC audience validation is disabled: no audiences configured. Any token \
                 issued by the trusted issuer will be accepted regardless of its `aud` claim. \
                 Set [http.oidc] audiences for production."
            );
        }

        let mut map = HashMap::new();
        for jwk in &jwks.keys {
            let Some(kid) = jwk.common.key_id.clone() else {
                continue;
            };
            let decoding_key = match DecodingKey::from_jwk(jwk) {
                Ok(key) => key,
                Err(e) => {
                    tracing::warn!(error = %e, kid, "Skipping JWK that failed to parse");
                    continue;
                }
            };

            // Use the key's declared algorithm; `jsonwebtoken` additionally
            // enforces that the key family matches, preventing alg-confusion.
            let alg = jwk_signing_algorithm(jwk).unwrap_or(Algorithm::RS256);
            let mut validation = Validation::new(alg);
            if audiences.is_empty() {
                validation.validate_aud = false;
            } else {
                validation.set_audience(audiences);
            }
            validation.set_issuer(&[issuer]);

            map.insert(
                kid,
                Arc::new(CachedKey {
                    decoding_key,
                    validation,
                }),
            );
        }
        map
    }

    /// Fetches the JWKS, retrying transient (network/HTTP) failures with
    /// full-jitter exponential backoff via [`crate::resilience::retry_transient`].
    /// Deserialization failures are not transient and are returned immediately.
    async fn fetch_jwks_with_retry(
        client: &openidconnect::reqwest::Client,
        url: &str,
    ) -> Result<JwkSet> {
        crate::resilience::retry_transient(crate::resilience::RetryPolicy::default(), || {
            Self::fetch_jwks(client, url)
        })
        .await
    }

    async fn fetch_jwks(client: &openidconnect::reqwest::Client, url: &str) -> Result<JwkSet> {
        // Network/HTTP failures are transient (kind `Io`) so `retry_transient`
        // retries them; a deserialization failure is deterministic (`config`).
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| Error::io(format!("JWKS fetch failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let msg = format!("JWKS fetch returned status {status}");
            // 4xx is a deterministic misconfiguration (wrong URL / auth) — don't
            // retry it; 5xx and other statuses are treated as transient.
            return Err(if status.is_client_error() {
                Error::config(msg)
            } else {
                Error::io(msg)
            });
        }

        let body = resp
            .text()
            .await
            .map_err(|e| Error::io(format!("JWKS response read failed: {e}")))?;

        serde_json::from_str::<JwkSet>(&body)
            .map_err(|e| Error::config(format!("JWKS deserialization failed: {e}")))
    }

    /// Validate a Bearer JWT token and return the decoded claims.
    pub(crate) async fn validate_token(
        self: &Arc<Self>,
        token: &str,
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
        match self.try_decode(token, kid).await {
            Ok(claims) => Ok(claims),
            Err(TokenError::KeyNotFound) => {
                tracing::debug!(kid, "Key not found in JWKS, refreshing");
                self.refresh().await.map_err(|_| TokenError::Invalid)?;
                self.try_decode(token, kid).await
            }
            Err(e) => Err(e),
        }
    }

    async fn try_decode(
        &self,
        token: &str,
        kid: &str,
    ) -> std::result::Result<serde_json::Value, TokenError> {
        // Clone the Arc out so the read lock is released before decoding.
        let cached = {
            let keys = self.keys.read().await;
            keys.get(kid).cloned()
        };
        let cached = cached.ok_or(TokenError::KeyNotFound)?;

        let token_data =
            decode::<serde_json::Value>(token, &cached.decoding_key, &cached.validation).map_err(
                |e| {
                    tracing::debug!(error = %e, kid, "JWT validation failed");
                    TokenError::Invalid
                },
            )?;

        Ok(token_data.claims)
    }

    /// Re-verify only the **signature and issuer** of a JWT against the cached
    /// JWKS, ignoring `exp`, `aud`, and `nonce`.
    ///
    /// Used to re-validate a stored OIDC ID token on the session path as defense
    /// in depth: the claims were already fully validated at callback time, so the
    /// purpose here is solely to detect a forged/tampered stored token. `exp` is
    /// intentionally **not** checked — ID tokens are short-lived and a live
    /// session legitimately outlives them via token refresh, so an exp check
    /// would spuriously drop valid sessions.
    ///
    /// Returns [`SignatureCheck::Unverifiable`] (rather than `Invalid`) when the
    /// signing key cannot be located even after a refresh, so the caller can fall
    /// back to other integrity guarantees instead of logging the user out on a
    /// transient JWKS problem or an unobserved key rotation.
    pub(crate) async fn verify_signature(self: &Arc<Self>, token: &str) -> SignatureCheck {
        let Ok(header) = decode_header(token) else {
            return SignatureCheck::Invalid;
        };
        let Some(kid) = header.kid else {
            return SignatureCheck::Invalid;
        };

        // Locate the key, refreshing once for rotation if it's missing.
        let mut cached = {
            let keys = self.keys.read().await;
            keys.get(&kid).cloned()
        };
        if cached.is_none() {
            if self.refresh().await.is_err() {
                return SignatureCheck::Unverifiable;
            }
            cached = {
                let keys = self.keys.read().await;
                keys.get(&kid).cloned()
            };
        }
        let Some(cached) = cached else {
            return SignatureCheck::Unverifiable;
        };

        // Reuse the cached key/algorithm/issuer, but relax exp and aud.
        let mut validation = cached.validation.clone();
        validation.validate_exp = false;
        validation.validate_aud = false;
        match decode::<serde_json::Value>(token, &cached.decoding_key, &validation) {
            Ok(_) => SignatureCheck::Valid,
            Err(e) => {
                tracing::debug!(error = %e, kid, "stored ID token failed signature re-verification");
                SignatureCheck::Invalid
            }
        }
    }
}

/// Outcome of re-verifying a stored token's signature (see
/// [`JwksProvider::verify_signature`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignatureCheck {
    /// The signature is valid: the token was issued by the trusted issuer.
    Valid,
    /// The signature is definitively invalid (forged or signed by an unknown key).
    Invalid,
    /// The signature could not be checked (JWKS unavailable, or the key was not
    /// found even after a refresh). Integrity must be ensured by other means.
    Unverifiable,
}

/// Maps a JWK's declared signing algorithm to a `jsonwebtoken::Algorithm`.
///
/// Returns `None` for keys with no declared (or non-signing) algorithm, in which
/// case the caller falls back to a sensible default.
fn jwk_signing_algorithm(jwk: &Jwk) -> Option<Algorithm> {
    match jwk.common.key_algorithm? {
        KeyAlgorithm::HS256 => Some(Algorithm::HS256),
        KeyAlgorithm::HS384 => Some(Algorithm::HS384),
        KeyAlgorithm::HS512 => Some(Algorithm::HS512),
        KeyAlgorithm::ES256 => Some(Algorithm::ES256),
        KeyAlgorithm::ES384 => Some(Algorithm::ES384),
        KeyAlgorithm::RS256 => Some(Algorithm::RS256),
        KeyAlgorithm::RS384 => Some(Algorithm::RS384),
        KeyAlgorithm::RS512 => Some(Algorithm::RS512),
        KeyAlgorithm::PS256 => Some(Algorithm::PS256),
        KeyAlgorithm::PS384 => Some(Algorithm::PS384),
        KeyAlgorithm::PS512 => Some(Algorithm::PS512),
        KeyAlgorithm::EdDSA => Some(Algorithm::EdDSA),
        _ => None,
    }
}

#[derive(Debug)]
pub(crate) enum TokenError {
    KeyNotFound,
    Invalid,
}

/// Whether verbose token-claim logging is explicitly opted in via the
/// `AXUM_CONF_LOG_TOKEN_CLAIMS` environment variable (`1`/`true`), read once.
///
/// Token claims contain PII (subject, email, username, roles), so they are
/// **not** logged merely because `DEBUG` is enabled — that would leak PII into
/// logs whenever someone raises the log level to troubleshoot. This gate keeps
/// claim logging an explicit, deliberate opt-in.
pub(crate) fn log_token_claims_enabled() -> bool {
    static ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
        std::env::var("AXUM_CONF_LOG_TOKEN_CLAIMS")
            .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    });
    *ENABLED
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

    if log_token_claims_enabled() {
        tracing::debug!(
            subject = %sub,
            email = ?email,
            preferred_username = ?preferred_username,
            group_count = groups.len(),
            role_count = roles.len(),
            "bearer token claims mapped to identity"
        );
    }

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

    match jwks.validate_token(token).await {
        Ok(claims) => {
            if let Some(identity) = claims_to_identity(&claims, &config.roles_claim, Some(token)) {
                request.extensions_mut().insert(Arc::new(identity));
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

    // --- ID-token signature re-verification --------------------------------

    const TEST_KID: &str = "test-kid";
    const TEST_ISSUER: &str = "https://issuer.example";
    const TEST_SECRET: &[u8] = b"this-is-a-test-hmac-signing-secret";

    /// Builds a provider whose only key is a symmetric HS256 key. HS256 exercises
    /// the same kid-lookup → decode → three-way-result path as the production
    /// RS256 keys (the real RS256 flow is covered by the Keycloak integration test).
    fn test_provider() -> Arc<JwksProvider> {
        use base64::Engine;
        let k = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(TEST_SECRET);
        let jwk_json = format!(r#"{{"kty":"oct","kid":"{TEST_KID}","alg":"HS256","k":"{k}"}}"#);
        let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_str(&jwk_json).unwrap();
        let jwks = JwkSet { keys: vec![jwk] };
        JwksProvider::from_keys_for_test(&jwks, TEST_ISSUER, &[])
    }

    fn sign(kid: &str, claims: &serde_json::Value) -> String {
        let mut header = jsonwebtoken::Header::new(Algorithm::HS256);
        header.kid = Some(kid.to_string());
        jsonwebtoken::encode(
            &header,
            claims,
            &jsonwebtoken::EncodingKey::from_secret(TEST_SECRET),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn verify_signature_accepts_valid_even_when_expired() {
        let provider = test_provider();
        // `exp` in the past: re-verification deliberately ignores exp, so a live
        // session whose stored ID token has expired must still verify as Valid.
        let token = sign(
            TEST_KID,
            &serde_json::json!({"sub":"alice","iss":TEST_ISSUER,"exp":0}),
        );
        assert_eq!(
            provider.verify_signature(&token).await,
            SignatureCheck::Valid
        );
    }

    #[tokio::test]
    async fn verify_signature_rejects_tampered_token() {
        let provider = test_provider();
        let token = sign(
            TEST_KID,
            &serde_json::json!({"sub":"alice","iss":TEST_ISSUER,"exp":0}),
        );
        // Flip the last character of the signature.
        let mut tampered = token;
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert_eq!(
            provider.verify_signature(&tampered).await,
            SignatureCheck::Invalid
        );
    }

    #[tokio::test]
    async fn verify_signature_rejects_wrong_issuer() {
        let provider = test_provider();
        let token = sign(
            TEST_KID,
            &serde_json::json!({"sub":"alice","iss":"https://evil.example","exp":0}),
        );
        assert_eq!(
            provider.verify_signature(&token).await,
            SignatureCheck::Invalid
        );
    }

    #[tokio::test]
    async fn verify_signature_unknown_kid_is_unverifiable() {
        let provider = test_provider();
        // A token whose kid is absent from the JWKS: a refresh is attempted (and
        // fails against the unreachable URL), so the result is Unverifiable rather
        // than Invalid — the caller falls back to the record's HMAC integrity.
        let token = sign(
            "unknown-kid",
            &serde_json::json!({"sub":"alice","iss":TEST_ISSUER}),
        );
        assert_eq!(
            provider.verify_signature(&token).await,
            SignatureCheck::Unverifiable
        );
    }

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
