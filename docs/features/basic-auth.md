# Basic Auth & API Keys

The `basic-auth` feature adds HTTP Basic Authentication and API key authentication for simple use cases where OIDC is overkill.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["basic-auth"] }
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

## Role-Based Logic

Since basic auth doesn't have built-in roles, implement them in your handler:

```rust
use axum::{Json, http::StatusCode};
use axum_conf::{AuthenticatedIdentity, Result, Error};

const ADMINS: &[&str] = &["admin", "superuser"];

async fn admin_only(identity: AuthenticatedIdentity) -> Result<Json<&'static str>> {
    if !ADMINS.contains(&identity.name()) {
        return Err(Error::Unauthorized(
            "Admin access required".to_string()
        ));
    }

    Ok(Json("Welcome, admin!"))
}
```

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

You cannot use both `basic-auth` and `keycloak` features together. Choose one authentication method per service.

If you need both user auth and service auth:
1. Use OIDC for users
2. Use client credentials flow in OIDC for services
3. Or run separate services for different auth needs

## Next Steps

- [Keycloak/OIDC](keycloak.md) - Full OIDC authentication
- [Sessions](sessions.md) - Cookie-based sessions
- [Security Middleware](../middleware/security.md) - Rate limiting, CORS
