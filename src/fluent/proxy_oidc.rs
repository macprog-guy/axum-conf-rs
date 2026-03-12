//! Proxy OIDC authentication middleware.
//!
//! Extracts authenticated identity from HTTP headers set by an authenticating
//! reverse proxy (e.g., oauth2-proxy with Nginx `auth_request`).

use axum::{extract::Request, middleware::Next, response::Response};
use std::sync::Arc;

use crate::utils::Sensitive;
use crate::{AuthMethod, AuthenticatedIdentity, HttpProxyOidcConfig};

/// Proxy OIDC authentication middleware function.
pub(crate) async fn proxy_oidc_middleware(
    config: Arc<HttpProxyOidcConfig>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(identity) = extract_identity(&config, request.headers()) {
        tracing::debug!(
            user = %identity.user,
            "Request authenticated via proxy OIDC"
        );
        request.extensions_mut().insert(identity);
    }

    next.run(request).await
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
}
