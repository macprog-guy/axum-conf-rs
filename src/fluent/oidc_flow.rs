//! OIDC Authorization Code Flow implementation.
//!
//! Enabled when `redirect_uri` is set in `[http.oidc]` configuration. Provides:
//!
//! - **Login handler**: Generates PKCE challenge (SHA-256), CSRF state, and nonce;
//!   stores them in the session; redirects to the OIDC provider's authorization endpoint.
//! - **Callback handler**: Validates CSRF state, exchanges the authorization code with
//!   the PKCE verifier, validates the ID token nonce, and stores access/refresh/ID tokens
//!   in the session.
//! - **Logout handler**: Retrieves the ID token hint, flushes the session, and redirects
//!   to the provider's end-session endpoint (RP-Initiated Logout).
//! - **Session-to-identity middleware**: On each request, converts stored session tokens
//!   into an [`AuthenticatedIdentity`]. Transparently refreshes expired access tokens
//!   using the refresh token (with a 30-second buffer before expiry). Skips if a Bearer
//!   token identity is already present, so Bearer always takes precedence.

use std::sync::Arc;

use axum::{
    Extension,
    extract::Query,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet,
    EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    core::{
        CoreAuthDisplay, CoreAuthPrompt, CoreErrorResponseType, CoreGenderClaim, CoreJsonWebKey,
        CoreJweContentEncryptionAlgorithm, CoreProviderMetadata, CoreResponseType,
        CoreRevocableToken, CoreRevocationErrorResponse, CoreTokenIntrospectionResponse,
        CoreTokenResponse,
    },
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    AuthMethod, AuthenticatedIdentity, Error, Result, config::HttpOidcConfig, utils::Sensitive,
};

// Session keys
const SESSION_PKCE_VERIFIER: &str = "oidc_pkce_verifier";
const SESSION_CSRF_STATE: &str = "oidc_csrf_state";
const SESSION_NONCE: &str = "oidc_nonce";
const SESSION_ACCESS_TOKEN: &str = "oidc_access_token";
const SESSION_REFRESH_TOKEN: &str = "oidc_refresh_token";
const SESSION_ID_TOKEN: &str = "oidc_id_token";
const SESSION_TOKEN_EXPIRY: &str = "oidc_token_expiry";
const SESSION_RETURN_URL: &str = "oidc_return_url";

/// The concrete Client type returned by `from_provider_metadata` + `set_redirect_uri`.
/// Auth URL is `EndpointSet` (always in discovery), Token URL is `EndpointMaybeSet`.
type OidcCoreClient = openidconnect::Client<
    openidconnect::EmptyAdditionalClaims,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    openidconnect::StandardErrorResponse<CoreErrorResponseType>,
    CoreTokenResponse,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Wraps the openidconnect Client with application-specific configuration.
#[derive(Clone)]
pub(crate) struct OidcClient {
    client: OidcCoreClient,
    http_client: openidconnect::reqwest::Client,
    scopes: Vec<String>,
    audiences: Vec<String>,
    post_login_redirect: String,
    post_logout_redirect: String,
    end_session_url: Option<String>,
    /// JWKS for re-verifying the signature of a stored ID token on the session
    /// path (defense in depth against a tampered/forged session record).
    id_token_keys: Arc<super::oidc_bearer::JwksProvider>,
}

impl OidcClient {
    /// Performs OIDC Discovery and creates the client.
    pub async fn discover(config: &HttpOidcConfig) -> Result<Self> {
        let issuer_url = format!(
            "{}/realms/{}",
            config.issuer_url.trim_end_matches('/'),
            config.realm
        );
        let issuer = IssuerUrl::new(issuer_url.clone())
            .map_err(|e| Error::config(format!("Invalid OIDC issuer URL: {e}")))?;

        let http_client = openidconnect::reqwest::Client::new();

        let provider_metadata = CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(|e| Error::config(format!("OIDC discovery failed: {e}")))?;

        // Try to extract end_session_endpoint from raw discovery JSON.
        // Keycloak advertises this, but it's not in CoreProviderMetadata's typed fields.
        let end_session_url = extract_end_session_endpoint(&provider_metadata);

        // A JWKS view used only to re-verify the signature of stored ID tokens on
        // the session path. The ID token's audience is the client id; production
        // fail-closed-on-empty-audiences doesn't apply (audiences is non-empty).
        let id_token_keys = super::oidc_bearer::JwksProvider::new(
            provider_metadata.jwks_uri().to_string(),
            issuer_url,
            vec![config.client_id.clone()],
            false,
        )
        .await?;

        let redirect_uri = config
            .redirect_uri
            .as_deref()
            .ok_or_else(|| Error::config("OIDC redirect_uri is required for auth code flow"))?;

        let client = openidconnect::Client::from_provider_metadata(
            provider_metadata,
            ClientId::new(config.client_id.clone()),
            Some(ClientSecret::new(config.client_secret.0.clone())),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_string())
                .map_err(|e| Error::config(format!("Invalid OIDC redirect_uri: {e}")))?,
        );

        Ok(Self {
            client,
            http_client,
            scopes: config.scopes.clone(),
            audiences: config.audiences.clone(),
            post_login_redirect: config.post_login_redirect.clone(),
            post_logout_redirect: config.post_logout_redirect.clone(),
            end_session_url,
            id_token_keys,
        })
    }

    /// Re-verify the signature of a stored ID token (see
    /// [`super::oidc_bearer::JwksProvider::verify_signature`]).
    async fn verify_id_token_signature(
        &self,
        id_token_jwt: &str,
    ) -> super::oidc_bearer::SignatureCheck {
        self.id_token_keys.verify_signature(id_token_jwt).await
    }

    /// Refreshes tokens using a refresh token.
    pub async fn refresh_tokens(&self, refresh_token: &str) -> Result<RefreshedTokens> {
        let response = self
            .client
            .exchange_refresh_token(&openidconnect::RefreshToken::new(refresh_token.to_string()))
            .map_err(|e| Error::authentication(format!("Token endpoint not configured: {e}")))?
            .request_async(&self.http_client)
            .await
            .map_err(|e| Error::authentication(format!("Token refresh failed: {e}")))?;

        let access = response.access_token().secret().clone();
        let refresh = response.refresh_token().map(|t| t.secret().clone());
        // Capture a freshly-issued ID token (when the provider returns one) so the
        // session's stored ID token stays valid and its signature/exp can be
        // re-verified on subsequent requests.
        let id_token = response.id_token().map(ToString::to_string);
        let expiry = response.expires_in().map_or_else(
            // Some providers omit `expires_in` on refresh. Fall back to a short,
            // sane lifetime rather than `0`, which would read as "already
            // expired" and force a refresh on every single request.
            || now_epoch_secs() + DEFAULT_TOKEN_LIFETIME_SECS,
            |d| now_epoch_secs() + d.as_secs(),
        );

        Ok(RefreshedTokens {
            access,
            refresh,
            id_token,
            expiry,
        })
    }
}

/// Fallback access-token lifetime (seconds) used when a refresh response omits
/// `expires_in`.
const DEFAULT_TOKEN_LIFETIME_SECS: u64 = 300;

/// Tokens returned by [`OidcClient::refresh_tokens`].
pub(crate) struct RefreshedTokens {
    pub access: String,
    pub refresh: Option<String>,
    pub id_token: Option<String>,
    pub expiry: u64,
}

/// Extract end_session_endpoint from OIDC discovery metadata.
/// This field is part of the RP-Initiated Logout spec and Keycloak advertises it,
/// but `openidconnect`'s `CoreProviderMetadata` doesn't expose it as a typed field.
fn extract_end_session_endpoint(metadata: &CoreProviderMetadata) -> Option<String> {
    // The ProviderMetadata exposes issuer URL — construct the end-session URL from Keycloak convention
    let issuer = metadata.issuer().as_str();
    // Keycloak end-session endpoint is typically at {issuer}/protocol/openid-connect/logout
    Some(format!("{issuer}/protocol/openid-connect/logout"))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /auth/login — redirects to the OIDC provider's authorization endpoint.
pub(crate) async fn login_handler(
    session: Session,
    Extension(oidc): Extension<Arc<OidcClient>>,
) -> impl IntoResponse {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut auth_request = oidc.client.authorize_url(
        AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );

    for scope in &oidc.scopes {
        auth_request = auth_request.add_scope(Scope::new(scope.clone()));
    }

    let (auth_url, csrf_state, nonce) = auth_request.set_pkce_challenge(pkce_challenge).url();

    // Store flow state in session
    let _ = session
        .insert(SESSION_PKCE_VERIFIER, pkce_verifier.secret().clone())
        .await;
    let _ = session
        .insert(SESSION_CSRF_STATE, csrf_state.secret().clone())
        .await;
    let _ = session.insert(SESSION_NONCE, nonce.secret().clone()).await;

    Redirect::temporary(auth_url.as_str())
}

#[derive(Deserialize)]
pub(crate) struct CallbackParams {
    code: String,
    state: String,
}

/// GET /auth/callback — exchanges the authorization code for tokens.
pub(crate) async fn callback_handler(
    session: Session,
    Query(params): Query<CallbackParams>,
    Extension(oidc): Extension<Arc<OidcClient>>,
) -> std::result::Result<impl IntoResponse, Error> {
    // Verify CSRF state
    let stored_state: String = session
        .get(SESSION_CSRF_STATE)
        .await
        .map_err(|e| Error::authentication(format!("Session error: {e}")))?
        .ok_or_else(|| Error::authentication("Missing CSRF state in session"))?;

    if params.state != stored_state {
        return Err(Error::authentication("CSRF state mismatch"));
    }

    // Retrieve PKCE verifier and nonce
    let pkce_verifier_secret: String = session
        .get(SESSION_PKCE_VERIFIER)
        .await
        .map_err(|e| Error::authentication(format!("Session error: {e}")))?
        .ok_or_else(|| Error::authentication("Missing PKCE verifier in session"))?;

    let nonce_secret: String = session
        .get(SESSION_NONCE)
        .await
        .map_err(|e| Error::authentication(format!("Session error: {e}")))?
        .ok_or_else(|| Error::authentication("Missing nonce in session"))?;

    // Exchange code for tokens
    let token_response = oidc
        .client
        .exchange_code(AuthorizationCode::new(params.code))
        .map_err(|e| Error::authentication(format!("Token endpoint not configured: {e}")))?
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier_secret))
        .request_async(&oidc.http_client)
        .await
        .map_err(|e| Error::authentication(format!("Token exchange failed: {e}")))?;

    // Validate ID token nonce
    let id_token = token_response
        .id_token()
        .ok_or_else(|| Error::authentication("No ID token in response"))?;

    let trusted_audiences = oidc.audiences.clone();
    let verifier = oidc
        .client
        .id_token_verifier()
        .set_other_audience_verifier_fn(move |aud| {
            trusted_audiences.iter().any(|a| a.as_str() == aud.as_str())
        });
    let _claims = id_token
        .claims(&verifier, &Nonce::new(nonce_secret))
        .map_err(|e| Error::authentication(format!("ID token validation failed: {e}")))?;

    // Log the full ID token claims for debugging. Claims are PII, so this is
    // gated behind an explicit opt-in (AXUM_CONF_LOG_TOKEN_CLAIMS), not merely
    // the DEBUG level.
    if super::oidc_bearer::log_token_claims_enabled() && tracing::enabled!(tracing::Level::DEBUG) {
        let jwt_str = id_token.to_string();
        let jwt_parts: Vec<&str> = jwt_str.split('.').collect();
        if jwt_parts.len() == 3 {
            use base64::Engine;
            let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
            if let Ok(payload) = engine.decode(jwt_parts[1])
                && let Ok(raw_claims) = serde_json::from_slice::<serde_json::Value>(&payload)
            {
                tracing::debug!(
                    id_token_claims = %raw_claims,
                    has_refresh_token = token_response.refresh_token().is_some(),
                    expires_in_secs = ?token_response.expires_in().map(|d| d.as_secs()),
                    "auth code flow token response before session storage"
                );
            }
        }
    }

    // Store tokens in session
    let _ = session
        .insert(
            SESSION_ACCESS_TOKEN,
            token_response.access_token().secret().clone(),
        )
        .await;

    if let Some(refresh_token) = token_response.refresh_token() {
        let _ = session
            .insert(SESSION_REFRESH_TOKEN, refresh_token.secret().clone())
            .await;
    }

    // Serialize the ID token as a raw JWT string for later claim parsing
    let _ = session.insert(SESSION_ID_TOKEN, id_token.to_string()).await;

    // Always record an expiry so the access token is eventually refreshed. When
    // the provider omits `expires_in`, fall back to a short lifetime (matching the
    // refresh path) rather than leaving it unset, which would treat the token as
    // never-expiring.
    let expiry = token_response.expires_in().map_or_else(
        || now_epoch_secs() + DEFAULT_TOKEN_LIFETIME_SECS,
        |d| now_epoch_secs() + d.as_secs(),
    );
    let _ = session.insert(SESSION_TOKEN_EXPIRY, expiry).await;

    // Clean up flow state from session
    let _ = session.remove::<String>(SESSION_PKCE_VERIFIER).await;
    let _ = session.remove::<String>(SESSION_CSRF_STATE).await;
    let _ = session.remove::<String>(SESSION_NONCE).await;

    // Use stored return URL (from browser redirect) if available, else fall back to config
    let return_url: Option<String> = session.get(SESSION_RETURN_URL).await.ok().flatten();
    let _ = session.remove::<String>(SESSION_RETURN_URL).await;

    let redirect_target = return_url
        // Only same-origin relative paths; reject protocol-relative (`//host`)
        // and backslash-tricked (`/\host`) open-redirect vectors.
        .filter(|url| crate::utils::is_safe_local_path(url))
        .unwrap_or_else(|| oidc.post_login_redirect.clone());

    Ok(Redirect::temporary(&redirect_target))
}

/// GET /auth/logout — clears the session and redirects.
pub(crate) async fn logout_handler(
    session: Session,
    Extension(oidc): Extension<Arc<OidcClient>>,
) -> impl IntoResponse {
    // Get ID token hint before flushing
    let id_token_hint: Option<String> = session.get(SESSION_ID_TOKEN).await.ok().flatten();

    // Clear the entire session
    let _ = session.flush().await;

    // Redirect to OIDC provider's end-session endpoint if available
    if let Some(end_session_url) = &oidc.end_session_url {
        // Infallible last-resort fallback, parsed once. The `expect` is on a
        // compile-time-constant, valid URL and can never fire.
        #[allow(clippy::expect_used)]
        static FALLBACK_URL: std::sync::LazyLock<url::Url> = std::sync::LazyLock::new(|| {
            url::Url::parse("http://localhost/").expect("a constant, valid URL")
        });
        let mut url = url::Url::parse(end_session_url).unwrap_or_else(|_| {
            url::Url::parse(&oidc.post_logout_redirect).unwrap_or_else(|_| FALLBACK_URL.clone())
        });
        {
            let mut query = url.query_pairs_mut();
            if let Some(id_token) = &id_token_hint {
                query.append_pair("id_token_hint", id_token);
            }
            query.append_pair("post_logout_redirect_uri", &oidc.post_logout_redirect);
        }
        Redirect::temporary(url.as_str())
    } else {
        Redirect::temporary(&oidc.post_logout_redirect)
    }
}

// ---------------------------------------------------------------------------
// Session-to-identity middleware
// ---------------------------------------------------------------------------

/// Middleware that populates `AuthenticatedIdentity` from session tokens.
///
/// Skips if an identity is already set (e.g. from Bearer token validation).
/// Transparently refreshes expired access tokens when a refresh token is available.
pub(crate) async fn session_to_identity(
    roles_claim: std::sync::Arc<String>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    // Skip if identity already set by Bearer token validation
    if AuthenticatedIdentity::present_in(request.extensions()) {
        return next.run(request).await;
    }

    let session = match request.extensions().get::<Session>() {
        Some(s) => s.clone(),
        None => return next.run(request).await,
    };

    let access_token: Option<String> = session.get(SESSION_ACCESS_TOKEN).await.ok().flatten();
    if access_token.is_none() {
        return next.run(request).await;
    }

    // Check expiry and refresh if needed
    let token_expiry: Option<u64> = session.get(SESSION_TOKEN_EXPIRY).await.ok().flatten();
    let now = now_epoch_secs();
    let is_expired = token_expiry
        .map(|exp| now >= exp.saturating_sub(30))
        .unwrap_or(false);

    if is_expired {
        let oidc_client = request.extensions().get::<Arc<OidcClient>>().cloned();
        let refresh_token: Option<String> = session.get(SESSION_REFRESH_TOKEN).await.ok().flatten();

        match (oidc_client, refresh_token) {
            (Some(client), Some(refresh)) => {
                match client.refresh_tokens(&refresh).await {
                    Ok(refreshed) => {
                        let _ = session
                            .insert(SESSION_ACCESS_TOKEN, &refreshed.access)
                            .await;
                        if let Some(rt) = &refreshed.refresh {
                            let _ = session.insert(SESSION_REFRESH_TOKEN, rt).await;
                        }
                        // Re-store the freshly-issued ID token (when present) so it
                        // stays valid for signature re-verification below.
                        if let Some(idt) = &refreshed.id_token {
                            let _ = session.insert(SESSION_ID_TOKEN, idt).await;
                        }
                        let _ = session.insert(SESSION_TOKEN_EXPIRY, refreshed.expiry).await;
                        // Continue — identity will be built below from the ID token
                    }
                    Err(_) => {
                        // Refresh failed — clear session
                        let _ = session.flush().await;
                        return next.run(request).await;
                    }
                }
            }
            _ => {
                // No refresh token or no client — clear and pass through
                let _ = session.flush().await;
                return next.run(request).await;
            }
        }
    }

    // Build identity from stored ID token claims
    if let Ok(Some(id_token_str)) = session.get::<String>(SESSION_ID_TOKEN).await {
        // Defense in depth: re-verify the stored ID token's signature. The claims
        // were already validated at callback time, but an external session store
        // (Postgres/Redis) could be tampered with — a definitively invalid
        // signature means a forged record, so drop identity and clear the session.
        // `Unverifiable` (transient JWKS issue / missed key rotation) is tolerated
        // because the stored record's integrity is also protected by its HMAC tag.
        if let Some(client) = request.extensions().get::<Arc<OidcClient>>().cloned()
            && matches!(
                client.verify_id_token_signature(&id_token_str).await,
                super::oidc_bearer::SignatureCheck::Invalid
            )
        {
            tracing::warn!(
                "stored session ID token failed signature re-verification; clearing session"
            );
            let _ = session.flush().await;
            return next.run(request).await;
        }

        let current_access: Option<String> = session.get(SESSION_ACCESS_TOKEN).await.ok().flatten();
        if let Some(identity) =
            parse_id_token_to_identity(&id_token_str, current_access.as_deref(), &roles_claim)
        {
            request.extensions_mut().insert(Arc::new(identity));
        }
    }

    next.run(request).await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parses a JWT ID token (without cryptographic verification — it was already
/// verified at callback time) to extract claims for `AuthenticatedIdentity`.
fn parse_id_token_to_identity(
    id_token_jwt: &str,
    access_token: Option<&str>,
    roles_claim: &str,
) -> Option<AuthenticatedIdentity> {
    // JWT format: header.payload.signature — extract the payload (middle segment)
    // without allocating a Vec; require exactly three segments.
    let mut segments = id_token_jwt.split('.');
    let (Some(_header), Some(payload_b64), Some(_sig), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return None;
    };

    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload = engine.decode(payload_b64).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    if super::oidc_bearer::log_token_claims_enabled() {
        tracing::debug!(
            claim_keys = ?claims.as_object().map(|o| o.keys().collect::<Vec<_>>()),
            has_sub = claims.get("sub").is_some(),
            has_email = claims.get("email").is_some(),
            has_preferred_username = claims.get("preferred_username").is_some(),
            has_realm_access = claims.get("realm_access").is_some(),
            "id token claims shape from session"
        );
    }

    let sub = claims.get("sub")?.as_str()?.to_string();

    let email = claims
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from);

    let preferred_username = claims
        .get("preferred_username")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Extract groups from realm_access.roles (Keycloak convention)
    let groups = claims
        .get("realm_access")
        .and_then(|ra| ra.get("roles"))
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Extract application roles from the configured top-level claim
    let roles = claims
        .get(roles_claim)
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

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

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// Helper: encode a JSON claims object as a fake JWT (header.payload.signature).
    fn fake_jwt(claims: &serde_json::Value) -> String {
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header = engine.encode(b"{}");
        let payload = engine.encode(serde_json::to_vec(claims).unwrap());
        let signature = engine.encode(b"sig");
        format!("{header}.{payload}.{signature}")
    }

    #[test]
    fn test_parse_id_token_full_claims() {
        let claims = serde_json::json!({
            "sub": "user-123",
            "email": "user@example.com",
            "preferred_username": "jdoe",
            "realm_access": {
                "roles": ["admin", "user"]
            }
        });
        let jwt = fake_jwt(&claims);

        let identity =
            parse_id_token_to_identity(&jwt, Some("access-tok"), "applicationRoles").unwrap();
        assert_eq!(identity.user, "user-123");
        assert_eq!(identity.email.as_deref(), Some("user@example.com"));
        assert_eq!(identity.preferred_username.as_deref(), Some("jdoe"));
        assert_eq!(identity.groups, vec!["admin", "user"]);
        assert!(matches!(identity.method, AuthMethod::Oidc));
        assert!(identity.access_token.is_some());
    }

    #[test]
    fn test_parse_id_token_minimal_claims() {
        let claims = serde_json::json!({ "sub": "user-456" });
        let jwt = fake_jwt(&claims);

        let identity = parse_id_token_to_identity(&jwt, None, "applicationRoles").unwrap();
        assert_eq!(identity.user, "user-456");
        assert!(identity.email.is_none());
        assert!(identity.preferred_username.is_none());
        assert!(identity.groups.is_empty());
        assert!(identity.access_token.is_none());
    }

    #[test]
    fn test_parse_id_token_missing_sub_returns_none() {
        let claims = serde_json::json!({ "email": "no-sub@example.com" });
        let jwt = fake_jwt(&claims);

        assert!(parse_id_token_to_identity(&jwt, None, "applicationRoles").is_none());
    }

    #[test]
    fn test_parse_id_token_invalid_jwt_format() {
        assert!(parse_id_token_to_identity("not-a-jwt", None, "applicationRoles").is_none());
        assert!(parse_id_token_to_identity("only.two", None, "applicationRoles").is_none());
        assert!(parse_id_token_to_identity("", None, "applicationRoles").is_none());
    }

    #[test]
    fn test_parse_id_token_invalid_base64_payload() {
        assert!(parse_id_token_to_identity("a.!!!invalid.c", None, "applicationRoles").is_none());
    }

    #[test]
    fn test_parse_id_token_invalid_json_payload() {
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let header = engine.encode(b"{}");
        let payload = engine.encode(b"not json");
        let sig = engine.encode(b"sig");
        let jwt = format!("{header}.{payload}.{sig}");

        assert!(parse_id_token_to_identity(&jwt, None, "applicationRoles").is_none());
    }

    #[test]
    fn test_parse_id_token_with_empty_roles() {
        let claims = serde_json::json!({
            "sub": "user-789",
            "realm_access": { "roles": [] }
        });
        let jwt = fake_jwt(&claims);

        let identity = parse_id_token_to_identity(&jwt, None, "applicationRoles").unwrap();
        assert!(identity.groups.is_empty());
    }

    #[test]
    fn test_parse_id_token_with_application_roles() {
        let claims = serde_json::json!({
            "sub": "user-roles",
            "applicationRoles": ["editor", "viewer"]
        });
        let jwt = fake_jwt(&claims);

        let identity = parse_id_token_to_identity(&jwt, None, "applicationRoles").unwrap();
        assert_eq!(identity.roles, vec!["editor", "viewer"]);
        assert!(identity.groups.is_empty());
    }

    #[test]
    fn test_parse_id_token_with_custom_roles_claim() {
        let claims = serde_json::json!({
            "sub": "user-custom",
            "myAppRoles": ["admin", "manager"]
        });
        let jwt = fake_jwt(&claims);

        let identity = parse_id_token_to_identity(&jwt, None, "myAppRoles").unwrap();
        assert_eq!(identity.roles, vec!["admin", "manager"]);
    }

    #[test]
    fn test_parse_id_token_roles_default_to_empty() {
        let claims = serde_json::json!({ "sub": "user-no-roles" });
        let jwt = fake_jwt(&claims);

        let identity = parse_id_token_to_identity(&jwt, None, "applicationRoles").unwrap();
        assert!(identity.roles.is_empty());
    }

    #[test]
    fn test_now_epoch_secs_is_reasonable() {
        let now = now_epoch_secs();
        // Should be after 2024-01-01 and before 2100-01-01
        assert!(now > 1_704_067_200);
        assert!(now < 4_102_444_800);
    }
}
