# Keycloak/OIDC Authentication

The `keycloak` feature adds OpenID Connect (OIDC) authentication with two modes:

- **Bearer-Only (API)**: Validates JWT tokens in `Authorization: Bearer` headers. Best for APIs and service-to-service communication.
- **Authorization Code Flow (Browser)**: Full login/logout flow with session-based identity. Best for web applications with browser users.

Both modes produce a unified `AuthenticatedIdentity` available as an Axum extractor.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["keycloak"] }
```

## Configuration

### Bearer-Only Mode (API)

```toml
# config/prod.toml
[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "myrealm"
client_id = "my-service"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
audiences = ["my-service", "account"]
```

Set the secret via environment variable:

```bash
export OIDC_CLIENT_SECRET="your-client-secret"
```

### Authorization Code Flow Mode (Browser)

Adding `redirect_uri` enables the full OIDC login flow with auto-registered routes.

```toml
# config/prod.toml
[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "myrealm"
client_id = "my-web-app"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
audiences = ["my-web-app"]
redirect_uri = "https://myapp.example.com/auth/callback"
scopes = ["openid", "profile", "email"]
post_login_redirect = "/dashboard"
post_logout_redirect = "/"
```

## Using AuthenticatedIdentity

`AuthenticatedIdentity` is the primary extractor for all authentication methods. It works with both Bearer-only and Auth Code Flow modes.

### Required Authentication

Returns 401 if the request is not authenticated:

```rust
use axum::Json;
use axum_conf::AuthenticatedIdentity;
use serde::Serialize;

#[derive(Serialize)]
struct UserInfo {
    user: String,
    email: Option<String>,
    groups: Vec<String>,
}

async fn whoami(identity: AuthenticatedIdentity) -> Json<UserInfo> {
    Json(UserInfo {
        user: identity.user,
        email: identity.email,
        groups: identity.groups,
    })
}
```

### Optional Authentication

Returns `None` for unauthenticated requests instead of 401:

```rust
use axum::Json;
use axum_conf::AuthenticatedIdentity;

async fn greet(identity: Option<AuthenticatedIdentity>) -> String {
    match identity {
        Some(id) => format!(
            "Hello, {}!",
            id.preferred_username.as_deref().unwrap_or(&id.user)
        ),
        None => "Hello, anonymous!".to_string(),
    }
}
```

## Authorization Code Flow

### How It Works

```
  Browser                     Your App                      Keycloak
    │                            │                              │
    │  GET /auth/login           │                              │
    │──────────────────────────▶│                              │
    │                            │  Generate PKCE + CSRF + nonce│
    │                            │  Store in session            │
    │  302 Redirect              │                              │
    │◀──────────────────────────│                              │
    │                            │                              │
    │  GET /realms/.../auth?...  │                              │
    │─────────────────────────────────────────────────────────▶│
    │                            │                              │
    │  User logs in              │                              │
    │◀─────────────────────────────────────────────────────────│
    │                            │                              │
    │  GET /auth/callback?code=…&state=…                       │
    │──────────────────────────▶│                              │
    │                            │  Verify CSRF state           │
    │                            │  Exchange code + PKCE verifier│
    │                            │─────────────────────────────▶│
    │                            │  Access + Refresh + ID tokens│
    │                            │◀─────────────────────────────│
    │                            │  Validate ID token nonce     │
    │                            │  Store tokens in session     │
    │  302 Redirect to /dashboard│                              │
    │◀──────────────────────────│                              │
    │                            │                              │
    │  GET /dashboard            │                              │
    │──────────────────────────▶│                              │
    │                            │  Session → AuthenticatedIdentity
    │  200 OK                    │  (auto-refreshes if expired) │
    │◀──────────────────────────│                              │
    │                            │                              │
    │  GET /auth/logout          │                              │
    │──────────────────────────▶│                              │
    │                            │  Flush session               │
    │  302 Redirect              │  Redirect to Keycloak logout │
    │◀──────────────────────────│─────────────────────────────▶│
```

### Keycloak Client Setup

1. Go to your Keycloak realm → **Clients** → **Create client**
2. Set **Client ID**: `my-web-app`
3. Set **Client type**: OpenID Connect
4. Enable **Client authentication** (makes it a confidential client)
5. Enable **Standard flow** (Authorization Code Flow)
6. Set **Valid redirect URIs**: `https://myapp.example.com/auth/callback`
7. Set **Valid post logout redirect URIs**: `https://myapp.example.com/`
8. Copy the **Client secret** from the Credentials tab

### Complete Working Example

```rust
use axum::{Json, routing::get};
use axum_conf::{AuthenticatedIdentity, Config, FluentRouter, Result};
use serde::Serialize;

#[derive(Serialize)]
struct UserInfo {
    user: String,
    email: Option<String>,
    preferred_username: Option<String>,
    groups: Vec<String>,
}

/// Protected route — requires authentication (401 if not logged in)
async fn dashboard(identity: AuthenticatedIdentity) -> Json<UserInfo> {
    Json(UserInfo {
        user: identity.user,
        email: identity.email,
        preferred_username: identity.preferred_username,
        groups: identity.groups,
    })
}

/// Public route — works with or without authentication
async fn home(identity: Option<AuthenticatedIdentity>) -> String {
    match identity {
        Some(id) => format!("Welcome back, {}!", id.preferred_username.as_deref().unwrap_or(&id.user)),
        None => "Welcome! Please <a href=\"/auth/login\">log in</a>.".to_string(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/", get(home))
        .route("/dashboard", get(dashboard))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

With `config/dev.toml`:

```toml
[http]
bind_port = 3000
max_payload_size_bytes = "1MiB"

[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "myrealm"
client_id = "my-web-app"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
audiences = ["my-web-app"]
redirect_uri = "http://localhost:3000/auth/callback"
post_login_redirect = "/dashboard"
post_logout_redirect = "/"
```

### Auto-Registered Routes

When `redirect_uri` is set, axum-conf automatically registers these routes:

| Route | Default Path | Purpose |
|-------|-------------|---------|
| Login | `/auth/login` | Redirects to Keycloak authorization endpoint |
| Callback | `/auth/callback` | Handles the authorization code exchange |
| Logout | `/auth/logout` | Clears session and redirects to Keycloak logout |

These paths are configurable:

```toml
[http.oidc]
login_route = "/sso/login"
callback_route = "/sso/callback"
logout_route = "/sso/logout"
```

### Security

The auth code flow includes multiple security measures:

- **PKCE (SHA-256)**: Prevents authorization code interception attacks
- **CSRF state parameter**: Validates the callback originated from our login request
- **Nonce validation**: Ensures the ID token was issued for this specific authentication
- **Transparent token refresh**: Expired access tokens are automatically refreshed using the refresh token (with 30-second buffer before expiry)
- **Session-based token storage**: Tokens are stored server-side in the session, never exposed to the browser

### Bearer + Session Coexistence

When auth code flow is enabled, both Bearer tokens and session cookies work simultaneously:

- **Bearer token takes precedence**: If a request has a valid `Authorization: Bearer` header, it is used regardless of session state
- **Session fallback**: Requests without a Bearer token fall back to session-based identity
- **Passthrough mode**: Unauthenticated requests (no Bearer, no session) pass through without a 401, allowing public routes to work

## Extracting Keycloak-Specific Claims

For Bearer-only mode, when you need Keycloak-specific claims like realm roles and client roles, use `KeycloakToken`:

```rust
use axum::Json;
use axum_conf::KeycloakToken;
use serde::Serialize;

#[derive(Serialize)]
struct TokenInfo {
    subject: String,
    email: Option<String>,
    realm_roles: Vec<String>,
    client_roles: Vec<String>,
}

async fn token_info(token: KeycloakToken) -> Json<TokenInfo> {
    Json(TokenInfo {
        subject: token.subject().to_string(),
        email: token.email().map(String::from),
        realm_roles: token.realm_roles().iter().map(|r| r.to_string()).collect(),
        client_roles: token.client_roles("my-service")
            .iter()
            .map(|r| r.to_string())
            .collect(),
    })
}
```

> **Note**: `AuthenticatedIdentity` is the recommended extractor for most use cases. Use `KeycloakToken` only when you need Keycloak-specific claims like `client_roles()`.

## Role-Based Access Control

Check user roles before allowing access:

```rust
use axum::{Json, http::StatusCode};
use axum_conf::{AuthenticatedIdentity, Result};

async fn admin_only(identity: AuthenticatedIdentity) -> Result<Json<&'static str>> {
    if !identity.groups.contains(&"admin".to_string()) {
        return Err(axum_conf::Error::Unauthorized(
            "Admin role required".to_string()
        ));
    }

    Ok(Json("Welcome, admin!"))
}
```

## Disabling Authentication for Specific Routes

Use middleware include/exclude to have some routes without auth:

```toml
[http.middleware]
exclude = ["oidc"]  # Disables auth globally - usually not what you want
```

Instead, use public routes before auth middleware:

```rust
use axum::routing::get;
use axum_conf::{Config, FluentRouter, Result};

async fn public_health() -> &'static str { "OK" }
async fn protected_data() -> &'static str { "Secret" }

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();

    FluentRouter::without_state(config)?
        .route("/protected", get(protected_data))
        .setup_middleware()
        .route("/public", get(public_health))
        .await?
        .start()
        .await
}
```

## Configuration Options

| Option | Description | Required | Default |
|--------|-------------|----------|---------|
| `issuer_url` | Base URL of the OIDC provider | Yes | — |
| `realm` | OIDC realm/tenant name | Yes | `"pictet"` |
| `client_id` | OAuth2 client identifier | Yes | — |
| `client_secret` | OAuth2 client secret | Yes | — |
| `audiences` | Expected JWT audiences (aud claim) | No | `[]` |
| `redirect_uri` | Callback URL; enables auth code flow when set | No | — |
| `scopes` | OAuth2 scopes to request | No | `["openid", "profile", "email"]` |
| `post_login_redirect` | Redirect destination after login | No | `"/"` |
| `post_logout_redirect` | Redirect destination after logout | No | `"/"` |
| `login_route` | Login endpoint path | No | `"/auth/login"` |
| `callback_route` | Callback endpoint path | No | `"/auth/callback"` |
| `logout_route` | Logout endpoint path | No | `"/auth/logout"` |

## Error Responses

When authentication fails (Bearer-only mode):

```bash
# Missing token
curl http://localhost:3000/protected
# 401 Unauthorized: {"error":"unauthorized","message":"Missing authorization header"}

# Invalid token
curl -H "Authorization: Bearer invalid" http://localhost:3000/protected
# 401 Unauthorized: {"error":"unauthorized","message":"Invalid token"}

# Expired token
curl -H "Authorization: Bearer $EXPIRED_TOKEN" http://localhost:3000/protected
# 401 Unauthorized: {"error":"unauthorized","message":"Token expired"}
```

## Next Steps

- [Sessions](sessions.md) - Session management details
- [PostgreSQL](postgres.md) - Add database support
- [Security Middleware](../middleware/security.md) - Rate limiting, CORS
