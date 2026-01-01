# Keycloak/OIDC Authentication

The `keycloak` feature adds OpenID Connect (OIDC) authentication with JWT validation, role-based access control, and token claim extraction.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["keycloak"] }
```

## Configuration

```toml
# config/prod.toml
[http.oidc]
issuer_url = "https://keycloak.example.com/realms/myrealm"
realm = "myrealm"
client_id = "my-service"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
audiences = ["my-service", "account"]
```

Set the secret via environment variable:

```bash
export OIDC_CLIENT_SECRET="your-client-secret"
```

## Basic Protected Route

```rust
use axum::{Json, routing::get};
use axum_conf::{Config, FluentRouter, Result, KeycloakToken};
use serde::Serialize;

#[derive(Serialize)]
struct UserInfo {
    subject: String,
    email: Option<String>,
    name: Option<String>,
}

async fn whoami(token: KeycloakToken) -> Json<UserInfo> {
    Json(UserInfo {
        subject: token.subject().to_string(),
        email: token.email().map(String::from),
        name: token.name().map(String::from),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/me", get(whoami))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Test with a token:

```bash
curl -H "Authorization: Bearer $ACCESS_TOKEN" http://localhost:3000/me
# Output: {"subject":"user-uuid","email":"user@example.com","name":"John Doe"}
```

## Extracting Token Claims

The `KeycloakToken` extractor provides access to JWT claims:

```rust
use axum::Json;
use axum_conf::KeycloakToken;
use serde::Serialize;

#[derive(Serialize)]
struct TokenInfo {
    subject: String,
    email: Option<String>,
    name: Option<String>,
    preferred_username: Option<String>,
    realm_roles: Vec<String>,
    client_roles: Vec<String>,
}

async fn token_info(token: KeycloakToken) -> Json<TokenInfo> {
    Json(TokenInfo {
        subject: token.subject().to_string(),
        email: token.email().map(String::from),
        name: token.name().map(String::from),
        preferred_username: token.preferred_username().map(String::from),
        realm_roles: token.realm_roles().iter().map(|r| r.to_string()).collect(),
        client_roles: token.client_roles("my-service")
            .iter()
            .map(|r| r.to_string())
            .collect(),
    })
}
```

## Role-Based Access Control

Check user roles before allowing access:

```rust
use axum::{Json, http::StatusCode};
use axum_conf::{KeycloakToken, Result};

async fn admin_only(token: KeycloakToken) -> Result<Json<&'static str>> {
    // Check realm roles
    if !token.realm_roles().contains(&"admin".to_string()) {
        return Err(axum_conf::Error::Unauthorized(
            "Admin role required".to_string()
        ));
    }

    Ok(Json("Welcome, admin!"))
}

async fn service_action(token: KeycloakToken) -> Result<Json<&'static str>> {
    // Check client-specific roles
    let client_roles = token.client_roles("my-service");

    if !client_roles.contains(&"write".to_string()) {
        return Err(axum_conf::Error::Unauthorized(
            "Write permission required".to_string()
        ));
    }

    Ok(Json("Action performed"))
}
```

## Optional Authentication

For routes that work with or without authentication:

```rust
use axum::Json;
use axum_conf::KeycloakToken;
use serde::Serialize;

#[derive(Serialize)]
struct Greeting {
    message: String,
}

async fn greet(token: Option<KeycloakToken>) -> Json<Greeting> {
    let message = match token {
        Some(t) => format!("Hello, {}!", t.name().unwrap_or("user")),
        None => "Hello, anonymous!".to_string(),
    };

    Json(Greeting { message })
}
```

## Service-to-Service Authentication

For backend services using client credentials:

```rust
use axum::Json;
use axum_conf::KeycloakToken;

async fn internal_api(token: KeycloakToken) -> Json<&'static str> {
    // Verify this is a service account (no user, just client)
    if token.preferred_username().is_some() {
        // This is a user token, might want different handling
    }

    // Check service has required scope/role
    let client_roles = token.client_roles("my-service");
    if client_roles.contains(&"internal-api".to_string()) {
        Json("Internal data")
    } else {
        Json("Forbidden")
    }
}
```

## Keycloak Setup

### Create Client in Keycloak

1. Go to your Keycloak realm → Clients → Create client
2. Set Client ID: `my-service`
3. Enable "Client authentication"
4. Set Valid redirect URIs (for browser flows)
5. Copy the client secret to your configuration

### Configure Roles

1. Realm roles: Go to Realm → Realm roles → Create
2. Client roles: Go to Client → Roles → Create

### Assign Roles to Users

1. Go to Users → Select user → Role mapping
2. Add realm roles or client roles as needed

## Configuration Options

| Option | Description | Required |
|--------|-------------|----------|
| `issuer_url` | Full URL to realm (e.g., `https://kc.example.com/realms/myrealm`) | Yes |
| `realm` | Realm name | Yes |
| `client_id` | Client identifier | Yes |
| `client_secret` | Client secret for validation | Yes |
| `audiences` | Expected JWT audiences (aud claim) | No |

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

    // Axum applies middleware from the outside in (last to first).
    // Protected routes have auth middleware applied
    FluentRouter::without_state(config)?
        .route("/protected", get(protected_data))
        .setup_middleware()
        .route("/public", get(public_health))
        .await?
        .start()
        .await
}
```

## Error Responses

When authentication fails:

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

- [PostgreSQL](postgres.md) - Add database support
- [Sessions](sessions.md) - Cookie-based sessions
- [Security Middleware](../middleware/security.md) - Rate limiting, CORS
