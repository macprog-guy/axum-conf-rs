//! Proxy OIDC authentication middleware.
//!
//! Extracts authenticated identity from HTTP headers set by an authenticating
//! reverse proxy (e.g., oauth2-proxy with Nginx `auth_request`).

use axum::{extract::ConnectInfo, extract::Request, middleware::Next, response::Response};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::utils::{Sensitive, constant_time_eq};
use crate::{AuthMethod, AuthenticatedIdentity, HttpProxyOidcConfig};

/// Proxy OIDC authentication middleware function.
///
/// Identity headers are only honored when the request demonstrably comes from a
/// trusted proxy (see [`is_trusted_source`]); otherwise the headers are ignored
/// so a direct client cannot spoof identity/roles. `is_production` drives the
/// fail-closed default when no trust anchor is configured.
pub(crate) async fn proxy_oidc_middleware(
    config: Arc<HttpProxyOidcConfig>,
    is_production: bool,
    mut request: Request,
    next: Next,
) -> Response {
    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip());

    if is_trusted_source(&config, is_production, peer_ip, request.headers()) {
        if let Some(identity) = extract_identity(&config, request.headers()) {
            // Don't log the username here — it is PII and would leak into logs at
            // DEBUG. Identity is recorded to the request span by `setup_user_span`.
            tracing::debug!("Request authenticated via proxy OIDC");
            request.extensions_mut().insert(Arc::new(identity));
        }
    } else if request.headers().get(&config.user_header).is_some() {
        // Identity headers are present but the request did not come from a
        // trusted proxy — ignore them rather than trusting a spoofable source.
        tracing::warn!(
            ?peer_ip,
            "Ignoring proxy identity headers from an untrusted source; configure \
             [http.proxy_oidc] trusted_proxies or shared_secret to trust them"
        );
    }

    next.run(request).await
}

/// Decides whether the request's proxy identity headers may be trusted.
///
/// Trust is granted if a configured trust anchor matches (peer IP within
/// `trusted_proxies`, or a matching `shared_secret`). If a trust anchor is
/// configured but none matches, trust is denied. If none is configured, trust
/// falls back to the environment: denied in production (fail-closed), allowed
/// otherwise (dev convenience).
fn is_trusted_source(
    config: &HttpProxyOidcConfig,
    is_production: bool,
    peer_ip: Option<IpAddr>,
    headers: &axum::http::HeaderMap,
) -> bool {
    // An empty/absent configured secret is NOT a trust anchor (e.g. an unset
    // `{{ ENV_VAR }}`); never trust an empty secret matching an empty header.
    if let Some(secret) = config.effective_shared_secret() {
        let provided = headers
            .get(&config.shared_secret_header)
            .and_then(|v| v.to_str().ok());
        if let Some(provided) = provided
            && constant_time_eq(provided.as_bytes(), secret.as_bytes())
        {
            return true;
        }
    }

    if !config.trusted_proxies.is_empty()
        && let Some(ip) = peer_ip
        && config.trusted_proxies.iter().any(|net| net.contains(&ip))
    {
        return true;
    }

    if config.has_trust_anchor() {
        // A trust anchor is configured but the request matched none of them.
        return false;
    }

    // Nothing configured: fail-closed in production, permissive elsewhere.
    !is_production
}

/// Extracts identity from proxy headers if the user header is present.
fn extract_identity(
    config: &HttpProxyOidcConfig,
    headers: &axum::http::HeaderMap,
) -> Option<AuthenticatedIdentity> {
    let user = headers
        .get(&config.user_header)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())?;

    let email = headers
        .get(&config.email_header)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let groups = headers
        .get(&config.groups_header)
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let preferred_username = headers
        .get(&config.preferred_username_header)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let access_token = headers
        .get(&config.access_token_header)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(Sensitive::from);

    let roles = headers
        .get(&config.roles_header)
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            s.split(',')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Some(AuthenticatedIdentity {
        method: AuthMethod::ProxyOidc,
        user,
        email,
        groups,
        roles,
        preferred_username,
        access_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn default_config() -> HttpProxyOidcConfig {
        HttpProxyOidcConfig::default()
    }

    #[test]
    fn test_extract_identity_all_headers() {
        let config = default_config();
        let mut headers = HeaderMap::new();
        headers.insert("X-Auth-Request-User", "jdoe".parse().unwrap());
        headers.insert("X-Auth-Request-Email", "jdoe@example.com".parse().unwrap());
        headers.insert("X-Auth-Request-Groups", "admin,operators".parse().unwrap());
        headers.insert(
            "X-Auth-Request-Preferred-Username",
            "johndoe".parse().unwrap(),
        );
        headers.insert(
            "X-Auth-Request-Access-Token",
            "eyJhbGciOi...".parse().unwrap(),
        );

        let identity = extract_identity(&config, &headers).unwrap();
        assert_eq!(identity.method, AuthMethod::ProxyOidc);
        assert_eq!(identity.user, "jdoe");
        assert_eq!(identity.email, Some("jdoe@example.com".to_string()));
        assert_eq!(identity.groups, vec!["admin", "operators"]);
        assert_eq!(identity.preferred_username, Some("johndoe".to_string()));
        assert!(identity.access_token.is_some());
        assert_eq!(identity.access_token.unwrap().0, "eyJhbGciOi...");
    }

    #[test]
    fn test_extract_identity_user_only() {
        let config = default_config();
        let mut headers = HeaderMap::new();
        headers.insert("X-Auth-Request-User", "jdoe".parse().unwrap());

        let identity = extract_identity(&config, &headers).unwrap();
        assert_eq!(identity.user, "jdoe");
        assert_eq!(identity.email, None);
        assert!(identity.groups.is_empty());
        assert_eq!(identity.preferred_username, None);
        assert!(identity.access_token.is_none());
    }

    #[test]
    fn test_extract_identity_no_user_header() {
        let config = default_config();
        let headers = HeaderMap::new();

        let identity = extract_identity(&config, &headers);
        assert!(identity.is_none());
    }

    #[test]
    fn test_extract_identity_groups_parsing() {
        let config = default_config();
        let mut headers = HeaderMap::new();
        headers.insert("X-Auth-Request-User", "jdoe".parse().unwrap());
        headers.insert(
            "X-Auth-Request-Groups",
            "admin, operators, devs".parse().unwrap(),
        );

        let identity = extract_identity(&config, &headers).unwrap();
        assert_eq!(identity.groups, vec!["admin", "operators", "devs"]);
    }

    #[test]
    fn test_extract_identity_empty_groups() {
        let config = default_config();
        let mut headers = HeaderMap::new();
        headers.insert("X-Auth-Request-User", "jdoe".parse().unwrap());
        headers.insert("X-Auth-Request-Groups", "".parse().unwrap());

        let identity = extract_identity(&config, &headers).unwrap();
        assert!(identity.groups.is_empty());
    }

    #[test]
    fn test_extract_identity_custom_headers() {
        let config = HttpProxyOidcConfig {
            user_header: "X-Custom-User".to_string(),
            email_header: "X-Custom-Email".to_string(),
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        headers.insert("X-Custom-User", "jane".parse().unwrap());
        headers.insert("X-Custom-Email", "jane@co.com".parse().unwrap());

        let identity = extract_identity(&config, &headers).unwrap();
        assert_eq!(identity.user, "jane");
        assert_eq!(identity.email, Some("jane@co.com".to_string()));
    }

    #[test]
    fn test_extract_identity_empty_email_is_none() {
        let config = default_config();
        let mut headers = HeaderMap::new();
        headers.insert("X-Auth-Request-User", "jdoe".parse().unwrap());
        headers.insert("X-Auth-Request-Email", "".parse().unwrap());

        let identity = extract_identity(&config, &headers).unwrap();
        assert_eq!(identity.email, None);
    }

    // --- trust-anchor gating -------------------------------------------------

    fn ip(s: &str) -> std::net::IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn untrusted_unconfigured_production_is_fail_closed() {
        let config = default_config();
        assert!(!is_trusted_source(
            &config,
            true,
            Some(ip("203.0.113.7")),
            &HeaderMap::new()
        ));
    }

    #[test]
    fn untrusted_unconfigured_dev_is_permissive() {
        let config = default_config();
        assert!(is_trusted_source(
            &config,
            false,
            Some(ip("203.0.113.7")),
            &HeaderMap::new()
        ));
    }

    #[test]
    fn trusted_proxies_match_grants_trust_even_in_production() {
        let config = HttpProxyOidcConfig {
            trusted_proxies: vec!["10.0.0.0/8".parse().unwrap()],
            ..Default::default()
        };
        assert!(is_trusted_source(
            &config,
            true,
            Some(ip("10.1.2.3")),
            &HeaderMap::new()
        ));
    }

    #[test]
    fn trusted_proxies_non_match_is_denied() {
        let config = HttpProxyOidcConfig {
            trusted_proxies: vec!["10.0.0.0/8".parse().unwrap()],
            ..Default::default()
        };
        // Out-of-range peer, and a missing peer IP, are both untrusted.
        assert!(!is_trusted_source(
            &config,
            false,
            Some(ip("192.168.1.1")),
            &HeaderMap::new()
        ));
        assert!(!is_trusted_source(&config, false, None, &HeaderMap::new()));
    }

    #[test]
    fn shared_secret_match_grants_trust() {
        let config = HttpProxyOidcConfig {
            shared_secret: Some(Sensitive::from("s3cr3t")),
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        headers.insert("X-Proxy-Secret", "s3cr3t".parse().unwrap());
        assert!(is_trusted_source(&config, true, None, &headers));
    }

    #[test]
    fn empty_shared_secret_is_not_a_trust_anchor() {
        // `shared_secret = "{{ VAR }}"` with VAR unset deserializes to Some("").
        let config = HttpProxyOidcConfig {
            shared_secret: Some(Sensitive::from("")),
            ..Default::default()
        };
        assert!(
            !config.has_trust_anchor(),
            "an empty secret must not count as a configured trust anchor"
        );
        // An attacker echoing an empty secret header must NOT be trusted; with
        // nothing else configured this falls back to the prod fail-closed default.
        let mut headers = HeaderMap::new();
        headers.insert("X-Proxy-Secret", "".parse().unwrap());
        assert!(!is_trusted_source(&config, true, None, &headers));
        // And outside production the unconfigured fallback is permissive.
        assert!(is_trusted_source(&config, false, None, &headers));
    }

    #[test]
    fn shared_secret_mismatch_or_missing_is_denied() {
        let config = HttpProxyOidcConfig {
            shared_secret: Some(Sensitive::from("s3cr3t")),
            ..Default::default()
        };
        let mut wrong = HeaderMap::new();
        wrong.insert("X-Proxy-Secret", "nope".parse().unwrap());
        assert!(!is_trusted_source(&config, false, None, &wrong));
        // Missing secret header, even in dev, is denied because a trust anchor
        // is configured.
        assert!(!is_trusted_source(&config, false, None, &HeaderMap::new()));
    }
}
