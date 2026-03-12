# Role-Based Access Control

The role-based access extractors let you gate routes by the roles present in `AuthenticatedIdentity`. Instead of checking roles manually in every handler, declare requirements at the extractor level — the framework returns `403 Forbidden` automatically when roles are insufficient.

No additional feature flag is needed. Role extractors work with every authentication method (OIDC, Basic Auth, API Key, Proxy OIDC).

## Defining Roles

Use the `role!` and `roles!` macros to define role marker types:

```rust
use axum_conf::{role, roles};

// Single roles
role!(Admin => "admin");
role!(Editor => "editor");

// Role sets (for AnyRole / AllRoles)
roles!(EditorOrViewer => "editor", "viewer");
roles!(AdminAndEditor => "admin", "editor");
```

These macros create zero-sized structs that implement the `ApplicationRole` or `ApplicationRoles` trait. You can also implement the traits manually if you prefer:

```rust
use axum_conf::{ApplicationRole, ApplicationRoles};

struct Admin;
impl ApplicationRole for Admin {
    const ROLE: &'static str = "admin";
}

struct EditorOrViewer;
impl ApplicationRoles for EditorOrViewer {
    const ROLES: &'static [&'static str] = &["editor", "viewer"];
}
```

## Extractors

### WithRole — Require a Single Role

Returns `401` if not authenticated, `403` if the user lacks the role.

```rust
use axum_conf::{role, WithRole};

role!(Admin => "admin");

async fn admin_dashboard(WithRole(identity, _): WithRole<Admin>) -> String {
    format!("Welcome admin {}!", identity.user)
}
```

### AnyRole — Require Any of Several Roles

Returns `403` if the user has **none** of the listed roles.

```rust
use axum_conf::{roles, AnyRole};

roles!(EditorOrViewer => "editor", "viewer");

async fn read_content(AnyRole(identity, _): AnyRole<EditorOrViewer>) -> String {
    format!("Content for {}", identity.user)
}
```

### AllRoles — Require All Listed Roles

Returns `403` if the user is missing **any** of the listed roles.

```rust
use axum_conf::{roles, AllRoles};

roles!(AdminAndEditor => "admin", "editor");

async fn admin_edit(AllRoles(identity, _): AllRoles<AdminAndEditor>) -> String {
    format!("{} can admin-edit", identity.user)
}
```

## Deref Access

All three extractors implement `Deref<Target = AuthenticatedIdentity>`, so you can access identity fields directly without destructuring:

```rust
use axum_conf::{role, WithRole};

role!(Admin => "admin");

async fn handler(admin: WithRole<Admin>) -> String {
    // Access fields directly via Deref
    format!("Hello {}! Email: {:?}", admin.user, admin.email)
}
```

## Complete Example

```rust
use axum::{Json, routing::get};
use axum_conf::{role, roles, WithRole, AnyRole, AllRoles};
use axum_conf::{AuthenticatedIdentity, Config, FluentRouter, Result};
use serde::Serialize;

// Define application roles
role!(Admin => "admin");
role!(Editor => "editor");
roles!(ContentManager => "editor", "viewer");
roles!(SuperAdmin => "admin", "editor");

#[derive(Serialize)]
struct UserInfo {
    user: String,
    roles: Vec<String>,
}

/// Any authenticated user
async fn profile(identity: AuthenticatedIdentity) -> Json<UserInfo> {
    Json(UserInfo {
        user: identity.user,
        roles: identity.roles,
    })
}

/// Admin only
async fn admin_panel(admin: WithRole<Admin>) -> Json<UserInfo> {
    Json(UserInfo {
        user: admin.user.clone(),
        roles: admin.roles.clone(),
    })
}

/// Editor OR viewer
async fn view_content(user: AnyRole<ContentManager>) -> Json<UserInfo> {
    Json(UserInfo {
        user: user.user.clone(),
        roles: user.roles.clone(),
    })
}

/// Must be BOTH admin AND editor
async fn admin_edit(user: AllRoles<SuperAdmin>) -> Json<UserInfo> {
    Json(UserInfo {
        user: user.user.clone(),
        roles: user.roles.clone(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/profile", get(profile))
        .route("/admin", get(admin_panel))
        .route("/content", get(view_content))
        .route("/admin/edit", get(admin_edit))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Configuring Roles per Auth Method

Roles are populated differently depending on the authentication method. Each method stores roles in `AuthenticatedIdentity.roles`, so the extractors work identically regardless of how the user authenticated.

### OIDC (Keycloak)

Roles are extracted from a configurable JWT claim (default: `applicationRoles`):

```toml
[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "myrealm"
client_id = "my-app"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
roles_claim = "applicationRoles"  # Default; set your custom claim here
```

To set up the claim in Keycloak, create a **protocol mapper** on your client that maps user attributes or client roles to a custom JWT claim.

### Basic Auth

Roles are configured statically per user or API key:

```toml
[http.basic_auth]
mode = "either"

[[http.basic_auth.users]]
username = "admin"
password = "{{ ADMIN_PASSWORD }}"
roles = ["admin", "editor"]

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_API_KEY }}"
name = "content-service"
roles = ["viewer"]
```

### Proxy OIDC

Roles are read from a comma-separated HTTP header set by the reverse proxy:

```toml
[http.proxy_oidc]
roles_header = "X-Auth-Request-Roles"  # Default header
```

The proxy should set the header as: `X-Auth-Request-Roles: admin, editor`.

## Error Responses

```bash
# Not authenticated — no credentials provided
curl http://localhost:3000/admin
# 401 Unauthorized: "Authentication required"

# Authenticated but missing required role
curl -u viewer:password http://localhost:3000/admin
# 403 Forbidden: "Insufficient role"
```

## Extractor Reference

| Extractor | Trait | Behavior | Use Case |
|-----------|-------|----------|----------|
| `WithRole<R>` | `ApplicationRole` | User must have the single role | Admin-only routes |
| `AnyRole<R>` | `ApplicationRoles` | User must have at least one role | "Editor or Viewer" access |
| `AllRoles<R>` | `ApplicationRoles` | User must have every role | Composite permission checks |

## Next Steps

- [Basic Auth & API Keys](basic-auth.md) - Configure roles for users and API keys
- [Keycloak/OIDC](keycloak.md) - Configure the roles JWT claim
- [Architecture](../architecture.md) - Middleware stack and request flow
