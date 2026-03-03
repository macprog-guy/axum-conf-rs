# ProxyOidc Authentication & Unified AuthenticatedIdentity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add ProxyOidc authentication (reverse proxy header-based auth) and unify all auth methods under a single `AuthenticatedIdentity` type with an idiomatic Axum extractor.

**Architecture:** ProxyOidc is a new middleware that reads identity from proxy-set HTTP headers (oauth2-proxy pattern). All auth methods (Basic Auth, API Key, OIDC, ProxyOidc) produce the same `AuthenticatedIdentity` struct inserted into request extensions. A custom Axum `FromRequestParts` extractor replaces `Extension<AuthenticatedIdentity>`.

**Tech Stack:** Rust, Axum 0.8, tower, serde, zeroize

**Design doc:** `docs/plans/2026-03-03-proxy-oidc-design.md`

---

### Task 1: Refactor AuthenticatedIdentity and AuthMethod

**Files:**
- Modify: `src/config/http/basic_auth.rs:218-233` (AuthenticatedIdentity and AuthMethod)

**Step 1: Update AuthMethod enum**

In `src/config/http/basic_auth.rs`, replace the existing `AuthMethod` enum (lines 227-233) with:

```rust
/// The authentication method used for a request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    /// HTTP Basic Auth (RFC 7617).
    BasicAuth,
    /// API Key authentication.
    ApiKey,
    /// OIDC/Keycloak JWT authentication.
    Oidc,
    /// Proxy-based OIDC authentication (e.g., oauth2-proxy).
    ProxyOidc,
}
```

**Step 2: Update AuthenticatedIdentity struct**

Replace the existing `AuthenticatedIdentity` struct (lines 218-224) with:

```rust
/// Identity of an authenticated user or service.
///
/// This struct is inserted into request extensions after successful authentication.
/// Use the Axum extractor to access it in handlers:
///
/// ```rust,ignore
/// use axum_conf::AuthenticatedIdentity;
///
/// // Required - returns 401 if not authenticated
/// async fn handler(identity: AuthenticatedIdentity) -> String {
///     format!("Hello, {}!", identity.user)
/// }
///
/// // Optional - returns None if not authenticated
/// async fn handler(identity: Option<AuthenticatedIdentity>) -> String {
///     match identity {
///         Some(id) => format!("Hello, {}!", id.user),
///         None => "Hello, anonymous!".to_string(),
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    /// The authentication method used.
    pub method: AuthMethod,
    /// The authenticated user identifier.
    /// For Basic Auth: username. For API Key: key name. For OIDC: subject. For ProxyOidc: X-Auth-Request-User.
    pub user: String,
    /// Email address of the authenticated user (optional).
    pub email: Option<String>,
    /// Groups the authenticated user belongs to.
    pub groups: Vec<String>,
    /// Preferred username for display purposes (optional).
    pub preferred_username: Option<String>,
    /// Access token (optional, wrapped in Sensitive to prevent logging).
    pub access_token: Option<Sensitive<String>>,
}
```

**Step 3: Add FromRequestParts implementation**

Add the following after the `AuthenticatedIdentity` struct definition (still in `src/config/http/basic_auth.rs`):

```rust
use axum::extract::FromRequestParts;
use http::request::Parts;
use http::StatusCode;

impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedIdentity {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> std::result::Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))
    }
}
```

Note: You'll need to add `axum` and `http` imports at the top of the file. The existing imports include `crate::{Error, Result, utils::Sensitive}` and `serde::Deserialize`. Add:

```rust
use axum::extract::FromRequestParts;
use http::{StatusCode, request::Parts};
```

**Step 4: Run tests to verify compilation**

Run: `cargo test --all-features --lib -- basic_auth`

Expected: Compilation errors in `src/fluent/basic_auth.rs` (uses old `name` field), `src/fluent/user_span.rs` (uses old `name` field), and `tests/basic_auth_tests.rs` (uses old `name` field and `Extension<AuthenticatedIdentity>`). This is expected — we fix those in subsequent tasks.

**Step 5: Commit**

```bash
git add src/config/http/basic_auth.rs
git commit -m "refactor: unify AuthenticatedIdentity with user/email/groups/preferred_username fields

Add Oidc and ProxyOidc variants to AuthMethod.
Rename 'name' to 'user', add email, groups, preferred_username, access_token.
Add FromRequestParts extractor implementation.

BREAKING: AuthenticatedIdentity.name renamed to .user"
```

---

### Task 2: Extend BasicAuthUser and BasicAuthApiKey config structs

**Files:**
- Modify: `src/config/http/basic_auth.rs:59-83` (BasicAuthUser, BasicAuthApiKey structs)

**Step 1: Update BasicAuthUser**

Replace lines 59-65 with:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthUser {
    /// Username for authentication.
    pub username: String,
    /// Password wrapped in Sensitive for secure handling.
    pub password: Sensitive<String>,
    /// Email address (optional, included in AuthenticatedIdentity).
    #[serde(default)]
    pub email: Option<String>,
    /// Groups this user belongs to (optional).
    #[serde(default)]
    pub groups: Vec<String>,
    /// Preferred username for display (optional).
    #[serde(default)]
    pub preferred_username: Option<String>,
}
```

**Step 2: Update BasicAuthApiKey**

Replace lines 76-83 with:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthApiKey {
    /// The API key value wrapped in Sensitive for secure handling.
    pub key: Sensitive<String>,
    /// Optional friendly name for logging and auditing purposes.
    #[serde(default)]
    pub name: Option<String>,
    /// Email address (optional, included in AuthenticatedIdentity).
    #[serde(default)]
    pub email: Option<String>,
    /// Groups this API key belongs to (optional).
    #[serde(default)]
    pub groups: Vec<String>,
    /// Preferred username for display (optional).
    #[serde(default)]
    pub preferred_username: Option<String>,
}
```

**Step 3: Update existing unit tests for new struct fields**

The existing test helper functions in `src/config/http/basic_auth.rs` (tests module around line 235) create `BasicAuthUser` and `BasicAuthApiKey` inline. These will need the new fields. Since they use `..Default::default()` pattern for `HttpBasicAuthConfig` but create users/keys directly, add the new fields with defaults. For example:

```rust
BasicAuthUser {
    username: "admin".to_string(),
    password: Sensitive::from("secret"),
    email: None,
    groups: vec![],
    preferred_username: None,
}
```

Update all test instances of `BasicAuthUser` and `BasicAuthApiKey` in the tests module to include the new fields.

**Step 4: Run config unit tests**

Run: `cargo test --all-features --lib -- config::http::basic_auth`

Expected: All config validation tests pass. The new fields are optional so existing tests should still work after adding defaults.

**Step 5: Commit**

```bash
git add src/config/http/basic_auth.rs
git commit -m "feat: extend BasicAuthUser and BasicAuthApiKey with email/groups/preferred_username"
```

---

### Task 3: Update Basic Auth middleware to populate new identity fields

**Files:**
- Modify: `src/fluent/basic_auth.rs:46-107` (try_basic_auth, try_api_key_auth functions)

**Step 1: Update try_basic_auth to populate new fields**

In `src/fluent/basic_auth.rs`, update the `try_basic_auth` function. Change the successful match block (around line 72-76) from:

```rust
return Ok(Some(AuthenticatedIdentity {
    method: AuthMethod::BasicAuth,
    name: username.to_string(),
}));
```

To:

```rust
return Ok(Some(AuthenticatedIdentity {
    method: AuthMethod::BasicAuth,
    user: username.to_string(),
    email: user.email.clone(),
    groups: user.groups.clone(),
    preferred_username: user.preferred_username.clone(),
    access_token: None,
}));
```

**Step 2: Update try_api_key_auth to populate new fields**

Change the successful match block (around line 97-102) from:

```rust
return Ok(Some(AuthenticatedIdentity {
    method: AuthMethod::ApiKey,
    name: key_config
        .name
        .clone()
        .unwrap_or_else(|| "api-key".to_string()),
}));
```

To:

```rust
return Ok(Some(AuthenticatedIdentity {
    method: AuthMethod::ApiKey,
    user: key_config
        .name
        .clone()
        .unwrap_or_else(|| "api-key".to_string()),
    email: key_config.email.clone(),
    groups: key_config.groups.clone(),
    preferred_username: key_config.preferred_username.clone(),
    access_token: None,
}));
```

**Step 3: Update basic_auth unit tests**

In the tests module of `src/fluent/basic_auth.rs`, update all assertions that reference `identity.name` to use `identity.user` instead. For example:

```rust
assert_eq!(identity.user, "testuser");
```

Also update the test helper `test_config()` function to include the new fields in `BasicAuthUser` and `BasicAuthApiKey`:

```rust
fn test_config() -> HttpBasicAuthConfig {
    HttpBasicAuthConfig {
        mode: BasicAuthMode::Either,
        api_key_header: "X-API-Key".to_string(),
        users: vec![BasicAuthUser {
            username: "testuser".to_string(),
            password: Sensitive::from("testpass"),
            email: None,
            groups: vec![],
            preferred_username: None,
        }],
        api_keys: vec![BasicAuthApiKey {
            key: Sensitive::from("test-api-key-12345"),
            name: Some("test-key".to_string()),
            email: None,
            groups: vec![],
            preferred_username: None,
        }],
    }
}
```

And the `test_api_key_without_name` test:

```rust
api_keys: vec![BasicAuthApiKey {
    key: Sensitive::from("nameless-key"),
    name: None,
    email: None,
    groups: vec![],
    preferred_username: None,
}],
```

And the `test_custom_api_key_header` test:

```rust
api_keys: vec![BasicAuthApiKey {
    key: Sensitive::from("custom-key"),
    name: Some("custom".to_string()),
    email: None,
    groups: vec![],
    preferred_username: None,
}],
```

**Step 4: Run basic_auth unit tests**

Run: `cargo test --all-features --lib -- fluent::basic_auth`

Expected: PASS — all basic auth tests pass with the new field names.

**Step 5: Commit**

```bash
git add src/fluent/basic_auth.rs
git commit -m "feat: populate email/groups/preferred_username in basic auth identity"
```

---

### Task 4: Simplify user_span to use unified AuthenticatedIdentity

**Files:**
- Modify: `src/fluent/user_span.rs` (entire file)

**Step 1: Simplify get_username_from_request**

Replace the `get_username_from_request` function (lines 27-55) with:

```rust
fn get_username_from_request(request: &Request<Body>) -> Option<String> {
    request
        .extensions()
        .get::<crate::AuthenticatedIdentity>()
        .map(|id| {
            id.preferred_username
                .clone()
                .unwrap_or_else(|| id.user.clone())
        })
}
```

Remove the `#[allow(unused_variables)]` attribute since we're no longer using feature-gated branches.

**Step 2: Update unit tests**

Replace the tests module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthMethod, AuthenticatedIdentity};

    #[test]
    fn test_get_username_from_identity_with_preferred() {
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(AuthenticatedIdentity {
            method: AuthMethod::BasicAuth,
            user: "user-id".to_string(),
            email: None,
            groups: vec![],
            preferred_username: Some("display-name".to_string()),
            access_token: None,
        });

        let username = get_username_from_request(&request);
        assert_eq!(username, Some("display-name".to_string()));
    }

    #[test]
    fn test_get_username_from_identity_without_preferred() {
        let mut request = Request::new(Body::empty());
        request.extensions_mut().insert(AuthenticatedIdentity {
            method: AuthMethod::ApiKey,
            user: "api-service".to_string(),
            email: None,
            groups: vec![],
            preferred_username: None,
            access_token: None,
        });

        let username = get_username_from_request(&request);
        assert_eq!(username, Some("api-service".to_string()));
    }

    #[test]
    fn test_get_username_no_auth() {
        let request = Request::new(Body::empty());
        let username = get_username_from_request(&request);
        assert_eq!(username, None);
    }
}
```

**Step 3: Run user_span tests**

Run: `cargo test --all-features --lib -- user_span`

Expected: PASS

**Step 4: Commit**

```bash
git add src/fluent/user_span.rs
git commit -m "refactor: simplify user_span to use unified AuthenticatedIdentity"
```

---

### Task 5: Add Post-OIDC mapper middleware

**Files:**
- Modify: `src/fluent/auth.rs:52-78` (setup_oidc method)

**Step 1: Add OIDC-to-AuthenticatedIdentity mapper**

In `src/fluent/auth.rs`, after the `KeycloakAuthLayer` is applied in `setup_oidc`, add a post-auth mapper layer. Update `setup_oidc` (lines 52-78) to:

```rust
#[cfg(feature = "keycloak")]
pub fn setup_oidc(mut self) -> Result<Self> {
    if let Some(oidc) = &self.config.http.oidc
        && self.is_middleware_enabled(HttpMiddleware::Oidc)
    {
        tracing::trace!(
            realm = %oidc.realm,
            issuer_url = %oidc.issuer_url,
            "OIDC middleware enabled"
        );
        let keycloak_auth_instance = KeycloakAuthInstance::new(
            KeycloakConfig::builder()
                .server(Url::parse(&oidc.issuer_url)?)
                .realm(oidc.realm.clone())
                .build(),
        );

        self.inner = self.inner.route_layer(
            KeycloakAuthLayer::<Role, ProfileAndEmail>::builder()
                .instance(keycloak_auth_instance)
                .passthrough_mode(PassthroughMode::Block)
                .expected_audiences(oidc.audiences.clone())
                .persist_raw_claims(true)
                .build(),
        );

        // Map KeycloakToken to unified AuthenticatedIdentity
        self.inner = self
            .inner
            .layer(axum::middleware::from_fn(map_keycloak_to_identity));
    }
    Ok(self)
}
```

**Step 2: Add the mapper function**

Add the following function in `src/fluent/auth.rs` (outside the impl block, near the top of the file with the other imports):

```rust
#[cfg(feature = "keycloak")]
async fn map_keycloak_to_identity(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(token) = request
        .extensions()
        .get::<axum_keycloak_auth::decode::KeycloakToken<
            crate::Role,
            axum_keycloak_auth::decode::ProfileAndEmail,
        >>()
    {
        let identity = crate::AuthenticatedIdentity {
            method: crate::AuthMethod::Oidc,
            user: token.subject.clone(),
            email: {
                let email = &token.extra.profile.email;
                if email.is_empty() { None } else { Some(email.clone()) }
            },
            groups: token.extra.roles.clone(),
            preferred_username: {
                let pref = &token.extra.profile.preferred_username;
                if pref.is_empty() { None } else { Some(pref.clone()) }
            },
            access_token: None,
        };
        request.extensions_mut().insert(identity);
    }
    next.run(request).await
}
```

**Step 3: Verify compilation**

Run: `cargo check --all-features`

Expected: PASS (or compilation errors in integration tests only, which we fix later).

**Step 4: Commit**

```bash
git add src/fluent/auth.rs
git commit -m "feat: add post-OIDC mapper to produce unified AuthenticatedIdentity"
```

---

### Task 6: Create ProxyOidc config

**Files:**
- Create: `src/config/http/proxy_oidc.rs`
- Modify: `src/config/http/mod.rs` (add module, add field to HttpConfig, update validation)

**Step 1: Create proxy_oidc.rs config module**

Create `src/config/http/proxy_oidc.rs`:

```rust
//! Proxy OIDC authentication configuration.
//!
//! This module provides configuration for reverse-proxy-based OIDC authentication,
//! where an upstream proxy (e.g., oauth2-proxy with Nginx `auth_request`) handles
//! OIDC authentication and passes identity information via HTTP headers.
//!
//! # Example
//!
//! ```toml
//! [http.proxy_oidc]
//! # All fields are optional with sensible defaults matching oauth2-proxy conventions
//! user_header = "X-Auth-Request-User"
//! email_header = "X-Auth-Request-Email"
//! groups_header = "X-Auth-Request-Groups"
//! preferred_username_header = "X-Auth-Request-Preferred-Username"
//! access_token_header = "X-Auth-Request-Access-Token"
//! ```

use serde::Deserialize;

/// Configuration for proxy-based OIDC authentication.
///
/// When present, the middleware reads identity information from HTTP headers
/// set by an authenticating reverse proxy (e.g., oauth2-proxy).
///
/// All header names have sensible defaults matching the oauth2-proxy convention.
/// If the user header is absent from a request, the middleware passes through
/// without setting an identity (no 401 error).
#[derive(Debug, Clone, Deserialize)]
pub struct HttpProxyOidcConfig {
    /// Header containing the authenticated user identifier.
    #[serde(default = "HttpProxyOidcConfig::default_user_header")]
    pub user_header: String,

    /// Header containing the user's email address.
    #[serde(default = "HttpProxyOidcConfig::default_email_header")]
    pub email_header: String,

    /// Header containing comma-separated group memberships.
    #[serde(default = "HttpProxyOidcConfig::default_groups_header")]
    pub groups_header: String,

    /// Header containing the preferred username for display.
    #[serde(default = "HttpProxyOidcConfig::default_preferred_username_header")]
    pub preferred_username_header: String,

    /// Header containing the access token (when proxy passes it).
    #[serde(default = "HttpProxyOidcConfig::default_access_token_header")]
    pub access_token_header: String,
}

impl Default for HttpProxyOidcConfig {
    fn default() -> Self {
        Self {
            user_header: Self::default_user_header(),
            email_header: Self::default_email_header(),
            groups_header: Self::default_groups_header(),
            preferred_username_header: Self::default_preferred_username_header(),
            access_token_header: Self::default_access_token_header(),
        }
    }
}

impl HttpProxyOidcConfig {
    fn default_user_header() -> String {
        "X-Auth-Request-User".to_string()
    }

    fn default_email_header() -> String {
        "X-Auth-Request-Email".to_string()
    }

    fn default_groups_header() -> String {
        "X-Auth-Request-Groups".to_string()
    }

    fn default_preferred_username_header() -> String {
        "X-Auth-Request-Preferred-Username".to_string()
    }

    fn default_access_token_header() -> String {
        "X-Auth-Request-Access-Token".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn test_proxy_oidc_defaults() {
        let config = HttpProxyOidcConfig::default();
        assert_eq!(config.user_header, "X-Auth-Request-User");
        assert_eq!(config.email_header, "X-Auth-Request-Email");
        assert_eq!(config.groups_header, "X-Auth-Request-Groups");
        assert_eq!(
            config.preferred_username_header,
            "X-Auth-Request-Preferred-Username"
        );
        assert_eq!(config.access_token_header, "X-Auth-Request-Access-Token");
    }

    #[test]
    fn test_proxy_oidc_config_parsing() {
        let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.proxy_oidc]
user_header = "X-Custom-User"
email_header = "X-Custom-Email"
        "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");
        assert!(config.http.proxy_oidc.is_some());
        let proxy_oidc = config.http.proxy_oidc.unwrap();
        assert_eq!(proxy_oidc.user_header, "X-Custom-User");
        assert_eq!(proxy_oidc.email_header, "X-Custom-Email");
        // Defaults for unspecified fields
        assert_eq!(proxy_oidc.groups_header, "X-Auth-Request-Groups");
    }

    #[test]
    fn test_proxy_oidc_config_all_defaults() {
        let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.proxy_oidc]
        "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");
        assert!(config.http.proxy_oidc.is_some());
        let proxy_oidc = config.http.proxy_oidc.unwrap();
        assert_eq!(proxy_oidc.user_header, "X-Auth-Request-User");
    }

    #[test]
    fn test_no_proxy_oidc_config() {
        let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"
        "#;

        let config: Config = toml_str.parse().expect("Failed to parse config");
        assert!(config.http.proxy_oidc.is_none());
    }
}
```

**Step 2: Add module and field to HttpConfig**

In `src/config/http/mod.rs`, add the module declaration near the top (after line 8, near other module declarations):

```rust
mod proxy_oidc;
```

And its public re-export (near the other re-exports):

```rust
pub use proxy_oidc::*;
```

Add the proxy_oidc field to `HttpConfig` struct (after the `basic_auth` field, around line 155):

```rust
    /// Proxy OIDC configuration for reverse-proxy-based authentication.
    /// When present, the middleware reads identity from proxy-set HTTP headers.
    #[serde(default)]
    pub proxy_oidc: Option<HttpProxyOidcConfig>,
```

Add it to the `Default` impl for `HttpConfig` (around line 340):

```rust
    proxy_oidc: None,
```

**Step 3: Update HttpConfig::validate() for mutual exclusion**

In `src/config/http/mod.rs`, in the `validate()` method (around line 277-283), after the existing `basic_auth` + `oidc` mutual exclusion check, add:

```rust
        // Mutual exclusion: proxy_oidc cannot be used with basic_auth
        #[cfg(feature = "basic-auth")]
        if self.basic_auth.is_some() && self.proxy_oidc.is_some() {
            return Err(crate::Error::invalid_input(
                "Cannot configure both [http.basic_auth] and [http.proxy_oidc]. Choose one authentication method.",
            ));
        }

        // Mutual exclusion: proxy_oidc cannot be used with oidc
        #[cfg(feature = "keycloak")]
        if self.oidc.is_some() && self.proxy_oidc.is_some() {
            return Err(crate::Error::invalid_input(
                "Cannot configure both [http.oidc] and [http.proxy_oidc]. Choose one authentication method.",
            ));
        }
```

**Step 4: Run config tests**

Run: `cargo test --all-features --lib -- config::http`

Expected: PASS

**Step 5: Commit**

```bash
git add src/config/http/proxy_oidc.rs src/config/http/mod.rs
git commit -m "feat: add HttpProxyOidcConfig with defaults matching oauth2-proxy conventions"
```

---

### Task 7: Add ProxyOidc to HttpMiddleware enum

**Files:**
- Modify: `src/config/http/middleware.rs:98-213` (HttpMiddleware enum)

**Step 1: Add ProxyOidc variant**

In `src/config/http/middleware.rs`, add the `ProxyOidc` variant to the `HttpMiddleware` enum. Add it after the `BasicAuth` variant (around line 110):

```rust
    /// Proxy OIDC authentication middleware.
    /// Reads identity from HTTP headers set by an authenticating reverse proxy.
    /// No feature flag required.
    ProxyOidc,
```

**Step 2: Run middleware tests**

Run: `cargo test --all-features --lib -- config::http::middleware`

Expected: PASS — existing tests don't reference ProxyOidc, so they continue to work.

**Step 3: Commit**

```bash
git add src/config/http/middleware.rs
git commit -m "feat: add ProxyOidc variant to HttpMiddleware enum"
```

---

### Task 8: Create ProxyOidc middleware

**Files:**
- Create: `src/fluent/proxy_oidc.rs`
- Modify: `src/fluent/mod.rs` (add module)
- Modify: `src/fluent/auth.rs` (add setup_proxy_oidc method)

**Step 1: Create proxy_oidc.rs middleware**

Create `src/fluent/proxy_oidc.rs`:

```rust
//! Proxy OIDC authentication middleware.
//!
//! Extracts authenticated identity from HTTP headers set by an authenticating
//! reverse proxy (e.g., oauth2-proxy with Nginx `auth_request`).

use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use crate::{AuthMethod, AuthenticatedIdentity, HttpProxyOidcConfig};
use crate::utils::Sensitive;

/// Proxy OIDC authentication middleware function.
///
/// Reads identity from proxy-set HTTP headers:
/// - `X-Auth-Request-User` → user (required for identity to be set)
/// - `X-Auth-Request-Email` → email
/// - `X-Auth-Request-Groups` → groups (comma-separated)
/// - `X-Auth-Request-Preferred-Username` → preferred_username
/// - `X-Auth-Request-Access-Token` → access_token
///
/// If the user header is absent, the request passes through without identity.
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
        .map(|s| Sensitive::from(s));

    Some(AuthenticatedIdentity {
        method: AuthMethod::ProxyOidc,
        user,
        email,
        groups,
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
        headers.insert(
            "X-Auth-Request-Groups",
            "admin,operators".parse().unwrap(),
        );
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
        assert_eq!(
            identity.preferred_username,
            Some("johndoe".to_string())
        );
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
```

**Step 2: Add module to fluent/mod.rs**

In `src/fluent/mod.rs`, add after the `basic_auth` module (around line 17):

```rust
mod proxy_oidc;
```

This should NOT be feature-gated (ProxyOidc is always available).

**Step 3: Add setup_proxy_oidc to auth.rs**

In `src/fluent/auth.rs`, add the following method inside the `impl<State> FluentRouter<State>` block, after `setup_basic_auth`:

```rust
    /// Sets up Proxy OIDC authentication.
    ///
    /// When configured, reads identity from HTTP headers set by an authenticating
    /// reverse proxy (e.g., oauth2-proxy with Nginx `auth_request`).
    ///
    /// If the user header is absent from a request, it passes through without
    /// setting an identity (no 401 error).
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.proxy_oidc]
    /// user_header = "X-Auth-Request-User"                      # default
    /// email_header = "X-Auth-Request-Email"                    # default
    /// groups_header = "X-Auth-Request-Groups"                  # default
    /// preferred_username_header = "X-Auth-Request-Preferred-Username"  # default
    /// access_token_header = "X-Auth-Request-Access-Token"      # default
    /// ```
    pub fn setup_proxy_oidc(mut self) -> Self {
        if let Some(proxy_oidc_config) = &self.config.http.proxy_oidc
            && self.is_middleware_enabled(HttpMiddleware::ProxyOidc)
        {
            tracing::trace!("ProxyOidc middleware enabled");
            let config = std::sync::Arc::new(proxy_oidc_config.clone());

            self.inner = self
                .inner
                .route_layer(axum::middleware::from_fn(move |request, next| {
                    let config = std::sync::Arc::clone(&config);
                    super::proxy_oidc::proxy_oidc_middleware(config, request, next)
                }));
        }
        self
    }
```

**Step 4: Run proxy_oidc unit tests**

Run: `cargo test --all-features --lib -- proxy_oidc`

Expected: PASS

**Step 5: Commit**

```bash
git add src/fluent/proxy_oidc.rs src/fluent/mod.rs src/fluent/auth.rs
git commit -m "feat: add ProxyOidc middleware for reverse-proxy-based authentication"
```

---

### Task 9: Wire ProxyOidc into the middleware stack

**Files:**
- Modify: `src/fluent/builder.rs:129-178` (setup_middleware method)

**Step 1: Add setup_proxy_oidc() call**

In `src/fluent/builder.rs`, in the `setup_middleware` method, add `setup_proxy_oidc()` after Basic Auth setup (around line 150). Change:

```rust
        #[cfg(feature = "basic-auth")]
        let router = router.setup_basic_auth()?; // 1b. Basic Auth (route_layer - applies to existing routes)

        // Public static files added AFTER auth so they're accessible without authentication
        let router = router.setup_public_files()?;
```

To:

```rust
        #[cfg(feature = "basic-auth")]
        let router = router.setup_basic_auth()?; // 1b. Basic Auth (route_layer - applies to existing routes)

        let router = router.setup_proxy_oidc(); // 1c. ProxyOidc (route_layer - applies to existing routes)

        // Public static files added AFTER auth so they're accessible without authentication
        let router = router.setup_public_files()?;
```

**Step 2: Verify compilation**

Run: `cargo check --all-features`

Expected: PASS (may have warnings about unused imports, which is fine).

**Step 3: Commit**

```bash
git add src/fluent/builder.rs
git commit -m "feat: wire ProxyOidc into middleware stack after Basic Auth"
```

---

### Task 10: Update integration tests

**Files:**
- Modify: `tests/basic_auth_tests.rs` (update for new AuthenticatedIdentity shape)
- Create: `tests/proxy_oidc_tests.rs` (new integration tests)

**Step 1: Update basic_auth_tests.rs**

In `tests/basic_auth_tests.rs`:

1. Change the import from `Extension` extraction to direct extraction (line 20-23):

```rust
use axum::{Router, routing::get};
use axum_conf::{
    AuthenticatedIdentity, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig,
};
```

2. Update the `whoami_handler` (line 130-132) to use the new extractor instead of `Extension`:

```rust
async fn whoami_handler(identity: AuthenticatedIdentity) -> String {
    format!("Hello, {}!", identity.user)
}
```

3. Update the `test_identity_extraction_basic_auth` test assertion (line 443):

```rust
    assert_eq!(body, "Hello, testuser!", "Should extract correct username");
```

This assertion already matches, but confirm the handler uses `identity.user` not `identity.name`.

4. Update the `test_identity_extraction_api_key` test assertion (line 465-468):

```rust
    assert_eq!(
        body, "Hello, test-key!",
        "Should extract correct API key name"
    );
```

This also already matches.

**Step 2: Create proxy_oidc_tests.rs**

Create `tests/proxy_oidc_tests.rs`:

```rust
//! Integration tests for Proxy OIDC authentication middleware.

use axum::{Router, routing::get};
use axum_conf::{
    AuthenticatedIdentity, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig,
};
use reqwest::Client;
use std::time::Duration;
use tokio::net::TcpListener;

fn create_proxy_oidc_config() -> Config {
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.proxy_oidc]

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

async fn whoami_handler(identity: AuthenticatedIdentity) -> String {
    format!(
        "user={} email={} groups={} preferred={}",
        identity.user,
        identity.email.unwrap_or_default(),
        identity.groups.join(","),
        identity.preferred_username.unwrap_or_default(),
    )
}

async fn optional_handler(identity: Option<AuthenticatedIdentity>) -> String {
    match identity {
        Some(id) => format!("Hello, {}!", id.user),
        None => "Hello, anonymous!".to_string(),
    }
}

async fn start_test_server(config: Config) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");

    let port = listener.local_addr().unwrap().port();

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(
            Router::new()
                .route("/test", get(|| async { "OK" }))
                .route("/whoami", get(whoami_handler))
                .route("/optional", get(optional_handler)),
        )
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();

    let handle = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .expect("Server failed to run");
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    (port, handle)
}

#[tokio::test]
async fn test_proxy_oidc_all_headers() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client
        .get(&url)
        .header("X-Auth-Request-User", "jdoe")
        .header("X-Auth-Request-Email", "jdoe@example.com")
        .header("X-Auth-Request-Groups", "admin,operators")
        .header("X-Auth-Request-Preferred-Username", "johndoe")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(
        body,
        "user=jdoe email=jdoe@example.com groups=admin,operators preferred=johndoe"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_no_headers_passes_through() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/optional", port);

    let response = client.get(&url).send().await.expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "Hello, anonymous!");

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_required_identity_missing_returns_401() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client.get(&url).send().await.expect("Request failed");

    assert_eq!(
        response.status(),
        401,
        "Required identity should return 401 when proxy headers are absent"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_health_endpoints_accessible() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create client");

    let health_url = format!("http://127.0.0.1:{}/health", port);
    let response = client
        .get(&health_url)
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "Health endpoint should not require proxy headers"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_user_only() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client
        .get(&url)
        .header("X-Auth-Request-User", "jdoe")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "user=jdoe email= groups= preferred=");

    server_handle.abort();
}
```

**Step 3: Run all integration tests**

Run: `cargo test --all-features --test '*'`

Expected: PASS for proxy_oidc_tests. Basic auth tests should also pass with the updated handler.

**Step 4: Commit**

```bash
git add tests/basic_auth_tests.rs tests/proxy_oidc_tests.rs
git commit -m "test: add proxy_oidc integration tests, update basic_auth tests for unified identity"
```

---

### Task 11: Update exports and Cargo.toml

**Files:**
- Modify: `src/lib.rs` (ensure new types are exported)
- Modify: `Cargo.toml` (no changes needed for deps, but update `full` feature if desired)

**Step 1: Verify exports**

The `AuthenticatedIdentity`, `AuthMethod`, and `HttpProxyOidcConfig` types should already be exported via the chain:
- `src/config/http/proxy_oidc.rs` → `pub use proxy_oidc::*` in `src/config/http/mod.rs` → `pub use config::*` in `src/lib.rs`
- `AuthenticatedIdentity` and `AuthMethod` are already exported via `src/config/http/basic_auth.rs` → `pub use basic_auth::*` (feature-gated behind `basic-auth`).

**Important**: `AuthenticatedIdentity` and `AuthMethod` are currently behind `#[cfg(feature = "basic-auth")]`. Since ProxyOidc doesn't require a feature flag, these types need to be available without `basic-auth`. Move them out of the feature-gated module.

Move `AuthenticatedIdentity`, `AuthMethod`, and the `FromRequestParts` impl from `src/config/http/basic_auth.rs` to a new file `src/config/http/identity.rs` that is NOT feature-gated.

Create `src/config/http/identity.rs`:

```rust
//! Authenticated identity types shared across all authentication methods.

use crate::utils::Sensitive;
use axum::extract::FromRequestParts;
use http::{StatusCode, request::Parts};

/// The authentication method used for a request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    /// HTTP Basic Auth (RFC 7617).
    BasicAuth,
    /// API Key authentication.
    ApiKey,
    /// OIDC/Keycloak JWT authentication.
    Oidc,
    /// Proxy-based OIDC authentication (e.g., oauth2-proxy).
    ProxyOidc,
}

/// Identity of an authenticated user or service.
///
/// This struct is inserted into request extensions after successful authentication.
/// Use the Axum extractor to access it in handlers:
///
/// ```rust,ignore
/// use axum_conf::AuthenticatedIdentity;
///
/// // Required - returns 401 if not authenticated
/// async fn handler(identity: AuthenticatedIdentity) -> String {
///     format!("Hello, {}!", identity.user)
/// }
///
/// // Optional - returns None if not authenticated
/// async fn optional_handler(identity: Option<AuthenticatedIdentity>) -> String {
///     match identity {
///         Some(id) => format!("Hello, {}!", id.user),
///         None => "Hello, anonymous!".to_string(),
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    /// The authentication method used.
    pub method: AuthMethod,
    /// The authenticated user identifier.
    pub user: String,
    /// Email address of the authenticated user (optional).
    pub email: Option<String>,
    /// Groups the authenticated user belongs to.
    pub groups: Vec<String>,
    /// Preferred username for display purposes (optional).
    pub preferred_username: Option<String>,
    /// Access token (optional, wrapped in Sensitive to prevent logging).
    pub access_token: Option<Sensitive<String>>,
}

impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedIdentity {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))
    }
}
```

In `src/config/http/mod.rs`, add:

```rust
mod identity;
pub use identity::*;
```

This should NOT be feature-gated. Then remove the `AuthenticatedIdentity`, `AuthMethod` structs, and `FromRequestParts` impl from `src/config/http/basic_auth.rs` (they now live in `identity.rs`). Also remove the `axum::extract::FromRequestParts` and `http::{StatusCode, request::Parts}` imports from `basic_auth.rs` that were added in Task 1.

**Step 2: Verify the whole project compiles**

Run: `cargo check --all-features`

Expected: PASS

**Step 3: Run ALL tests**

Run: `cargo test --all-features`

Expected: PASS

**Step 4: Commit**

```bash
git add src/config/http/identity.rs src/config/http/mod.rs src/config/http/basic_auth.rs src/lib.rs
git commit -m "refactor: move AuthenticatedIdentity to identity.rs (not feature-gated)"
```

---

### Task 12: Run clippy and fix warnings

**Files:**
- Any files with warnings

**Step 1: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: PASS or fix any warnings.

**Step 2: Fix any warnings**

Address any clippy lints that appear.

**Step 3: Run full test suite**

Run: `cargo test --all-features`

Expected: PASS

**Step 4: Commit if changes were needed**

```bash
git add -A
git commit -m "fix: address clippy warnings"
```

---

### Task 13: Update CLAUDE.md and documentation

**Files:**
- Modify: `CLAUDE.md` (update Architecture > Core Components)

**Step 1: Update CLAUDE.md**

In the Cargo Features section, note that ProxyOidc is always available (no feature flag needed). No other documentation files need updating unless explicitly requested.

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: document ProxyOidc in CLAUDE.md"
```
