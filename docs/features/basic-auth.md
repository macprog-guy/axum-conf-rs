# Basic Auth & API Keys

The `basic-auth` feature adds HTTP Basic Authentication and API key authentication for simple use cases where OIDC is overkill.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.5", features = ["basic-auth"] }
```

## Configuration

### Basic Auth Only

```toml
# config/prod.toml
[http.basic_auth]
mode = "basic"

[[http.basic_auth.users]]
username = "admin"
password = "{{ ADMIN_PASSWORD }}"

[[http.basic_auth.users]]
username = "readonly"
password = "{{ READONLY_PASSWORD }}"
```

### API Keys Only

```toml
[http.basic_auth]
mode = "api_key"
api_key_header = "X-API-Key"  # Default header name

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_A_API_KEY }}"
name = "service-a"

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_B_API_KEY }}"
name = "service-b"
```

### Either Mode (Basic Auth OR API Key)

```toml
[http.basic_auth]
mode = "either"
api_key_header = "X-API-Key"

[[http.basic_auth.users]]
username = "admin"
password = "{{ ADMIN_PASSWORD }}"

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_API_KEY }}"
name = "automated-service"
```

## Basic Usage

```rust
use axum::{Json, routing::get};
use axum_conf::{Config, FluentRouter, Result, AuthenticatedIdentity};
use serde::Serialize;

#[derive(Serialize)]
struct WhoAmI {
    identity: String,
    auth_method: String,
}

async fn whoami(identity: AuthenticatedIdentity) -> Json<WhoAmI> {
    Json(WhoAmI {
        identity: identity.name().to_string(),
        auth_method: format!("{:?}", identity.method()),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/whoami", get(whoami))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Testing Authentication

### Basic Auth

```bash
# Using username:password
curl -u admin:secret http://localhost:3000/whoami
# Output: {"identity":"admin","auth_method":"Basic"}

# Using Authorization header directly
curl -H "Authorization: Basic YWRtaW46c2VjcmV0" http://localhost:3000/whoami
# (YWRtaW46c2VjcmV0 is base64 for admin:secret)
```

### API Key

```bash
# Using X-API-Key header
curl -H "X-API-Key: sk-your-api-key" http://localhost:3000/whoami
# Output: {"identity":"service-a","auth_method":"ApiKey"}
```

## AuthenticatedIdentity Extractor

The `AuthenticatedIdentity` type provides access to the authenticated user or service:

```rust
use axum_conf::{AuthenticatedIdentity, AuthMethod};

async fn protected_handler(identity: AuthenticatedIdentity) -> String {
    // Get the username or API key name
    let name = identity.name();

    // Check how they authenticated
    match identity.method() {
        AuthMethod::Basic => {
            format!("User {} authenticated with Basic Auth", name)
        }
        AuthMethod::ApiKey => {
            format!("Service {} authenticated with API Key", name)
        }
    }
}
```

## Optional Authentication

For routes that work with or without authentication:

```rust
use axum::Json;
use axum_conf::AuthenticatedIdentity;
use serde::Serialize;

#[derive(Serialize)]
struct Response {
    message: String,
}

async fn maybe_protected(identity: Option<AuthenticatedIdentity>) -> Json<Response> {
    let message = match identity {
        Some(id) => format!("Hello, {}!", id.name()),
        None => "Hello, anonymous!".to_string(),
    };

    Json(Response { message })
}
```

## Role-Based Access Control

Assign roles to users and API keys in configuration, then use the role extractors to gate routes:

```toml
[[http.basic_auth.users]]
username = "admin"
password = "{{ ADMIN_PASSWORD }}"
roles = ["admin", "editor"]

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_API_KEY }}"
name = "content-service"
roles = ["viewer"]
```

```rust
use axum_conf::{role, WithRole};

role!(Admin => "admin");

async fn admin_only(admin: WithRole<Admin>) -> String {
    format!("Welcome, {}!", admin.user)
}
```

See [Role-Based Access Control](role-based-access.md) for the full guide on `WithRole`, `AnyRole`, and `AllRoles` extractors.

## Custom API Key Header

Change the header name for API key authentication:

```toml
[http.basic_auth]
mode = "api_key"
api_key_header = "Authorization"  # Use Authorization header
```

Then use Bearer format:

```bash
curl -H "Authorization: Bearer sk-your-api-key" http://localhost:3000/api
```

Or use a completely custom header:

```toml
[http.basic_auth]
mode = "api_key"
api_key_header = "X-Service-Token"
```

```bash
curl -H "X-Service-Token: sk-your-api-key" http://localhost:3000/api
```

## Environment Variables

Store credentials securely:

```bash
# .env (don't commit this!)
ADMIN_PASSWORD=super-secret-password
SERVICE_API_KEY=sk-1234567890abcdef
```

```toml
[[http.basic_auth.users]]
username = "admin"
password = "{{ ADMIN_PASSWORD }}"

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_API_KEY }}"
name = "my-service"
```

## Error Responses

```bash
# Missing credentials
curl http://localhost:3000/protected
# 401 Unauthorized
# WWW-Authenticate: Basic realm="Protected"

# Invalid credentials
curl -u wrong:wrong http://localhost:3000/protected
# 401 Unauthorized

# Invalid API key
curl -H "X-API-Key: invalid" http://localhost:3000/protected
# 401 Unauthorized
```

## Security Considerations

1. **Always use HTTPS** - Basic auth sends credentials base64-encoded (not encrypted)
2. **Use strong passwords** - Generate random passwords for service accounts
3. **Rotate API keys** - Implement key rotation procedures
4. **Prefer OIDC for users** - Basic auth is better for service-to-service
5. **Limit scope** - Create separate credentials for different services

## When to Use Basic Auth vs OIDC

| Scenario | Recommended |
|----------|-------------|
| Human users with browser | OIDC (Keycloak) |
| Service-to-service | API Keys |
| Simple internal tools | Basic Auth |
| Public APIs | API Keys |
| Multi-tenant | OIDC |
| Quick prototyping | Basic Auth |

## Combining with OIDC

Basic Auth and OIDC (`keycloak` feature) can coexist, in two different ways depending on what
you need:

- **App-wide coexistence** — when OIDC's Authorization Code flow is enabled (`redirect_uri` set),
  Basic Auth and OIDC both cover *every* route, and whichever credential a request carries decides
  which one handles it (no `Authorization` header at all falls through to OIDC session auth). This
  is for apps that serve both browser users (OIDC login) and API clients (Basic Auth/API keys) on
  the same routes. See [Keycloak/OIDC: OIDC + Basic Auth Coexistence](keycloak.md#oidc--basic-auth-coexistence).
- **Per-route scoping** — a specific route (or group of routes) requires Basic Auth while the rest
  of the app requires OIDC exclusively, with no credential leakage in either direction. This is
  what you want to, for example, secure the Prometheus `/metrics` endpoint with a scrape credential
  while every other route enforces OIDC bearer tokens. See below.

In **bearer-only** OIDC mode (no `redirect_uri`), plain `[http.basic_auth]` and `[http.oidc]`
remain mutually exclusive app-wide — both compete for the `Authorization` header on every route.
Per-route scoping (below) is the way around that: `[http.metrics_basic_auth]` (and the general
`secure_with_basic_auth()` builder method it's built on) is a config field the mutual-exclusion
check never inspects, so it coexists with bearer-only OIDC without needing auth-code flow at all.

### Securing built-in endpoints with a different method (e.g. `/metrics`)

`FluentRouter::secure_with_basic_auth()` wraps a set of routes in their own, independent Basic
Auth `route_layer`, merged into the app — separate from whatever `[http.basic_auth]` / `[http.oidc]`
govern the rest of the application. A route secured this way requires exactly this credential;
an OIDC bearer token (or the app's own `[http.basic_auth]` credentials, if configured) will not
satisfy it, and vice versa.

The built-in Prometheus `/metrics` endpoint uses this internally: set `[http.metrics_basic_auth]`
and it's automatically wired up.

#### Configuration

```toml
# config/prod.toml
[http]
with_metrics = true
metrics_route = "/metrics"

# App-wide: bearer-only OIDC (no redirect_uri) — every ordinary route requires a valid JWT.
[http.oidc]
issuer_url    = "https://sso.example.com/idp"
client_id     = "my-service"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
audiences     = ["my-service"]

# Metrics-only: independent Basic Auth, scoped to /metrics.
[http.metrics_basic_auth]
mode = "basic"

[[http.metrics_basic_auth.users]]
username = "prometheus"
password = "{{ METRICS_SCRAPE_PASSWORD }}"
```

Omitting `[http.metrics_basic_auth]` keeps today's default: `/metrics` is registered plainly, not
wrapped by any auth layer.

#### Using `secure_with_basic_auth` for your own routes

The mechanism is a general-purpose builder method, not a metrics-only special case — use it for
any route group that needs its own Basic Auth credential, independent of the app's main auth:

```rust,no_run
use axum::routing::get;
use axum_conf::{Config, FluentRouter, HttpBasicAuthConfig, Result};

async fn debug_handler() -> &'static str {
    "debug info"
}

async fn example(admin_basic_auth: HttpBasicAuthConfig) -> Result<()> {
    let router = FluentRouter::without_state(Config::<()>::default())?
        .secure_with_basic_auth(&admin_basic_auth, |r| r.route("/admin/debug", get(debug_handler)))
        .setup_middleware()
        .await?; // every other route still goes through the app's own auth config
    Ok(())
}
```

#### Error Responses

```bash
# No credentials
curl http://localhost:3000/metrics
# 401 Unauthorized

# A valid OIDC bearer token does NOT satisfy /metrics's Basic Auth requirement
curl -H "Authorization: Bearer <valid-jwt>" http://localhost:3000/metrics
# 401 Unauthorized

# Correct Basic Auth credentials succeed
curl -u prometheus:scrape-password http://localhost:3000/metrics
# 200 OK — Prometheus exposition text
```

## Next Steps

- [Role-Based Access Control](role-based-access.md) - Gate routes by role
- [Keycloak/OIDC](keycloak.md) - Full OIDC authentication
- [Sessions](sessions.md) - Cookie-based sessions
- [Security Middleware](../middleware/security.md) - Rate limiting, CORS
