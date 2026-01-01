# OpenAPI Documentation

Generate interactive API documentation with OpenAPI spec and Scalar UI.

## Quick Start

Enable the feature:

```toml
[dependencies]
axum-conf = { version = "0.3", features = ["openapi"] }
utoipa = { version = "5", features = ["axum_extras"] }
```

Define your API schema and serve documentation:

```rust
use axum::{Json, routing::get};
use axum_conf::{Config, FluentRouter, Result};
use axum_conf::openapi::OpenApiExt;
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

#[derive(Serialize, Deserialize, ToSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

/// List all users
#[utoipa::path(
    get,
    path = "/users",
    responses(
        (status = 200, description = "List of users", body = [User])
    )
)]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![
        User { id: 1, name: "Alice".into(), email: "alice@example.com".into() }
    ])
}

#[derive(OpenApi)]
#[openapi(
    paths(list_users),
    components(schemas(User))
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();

    FluentRouter::without_state(config)?
        .route("/users", get(list_users))
        .with_openapi::<ApiDoc>("/docs")  // Scalar UI at /docs
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Visit `http://localhost:3000/docs` to see interactive API documentation.

## What Gets Created

| Endpoint | Description |
|----------|-------------|
| `/docs` | Scalar UI - interactive API explorer |
| `/openapi.json` | Raw OpenAPI JSON specification |

## Annotating Handlers

Use `#[utoipa::path]` to document your endpoints:

```rust
use axum::{Json, extract::Path};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
struct User {
    /// Unique user identifier
    id: u64,
    /// User's display name
    name: String,
}

/// Get a specific user by ID
#[utoipa::path(
    get,
    path = "/users/{id}",
    params(
        ("id" = u64, Path, description = "User ID to fetch")
    ),
    responses(
        (status = 200, description = "User found", body = User),
        (status = 404, description = "User not found")
    ),
    tag = "users"
)]
async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(User { id, name: "Alice".into() })
}
```

## Request Body Documentation

```rust
use axum::Json;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
struct CreateUser {
    /// User's display name (required)
    name: String,
    /// User's email address
    email: String,
}

#[derive(Serialize, ToSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

/// Create a new user
#[utoipa::path(
    post,
    path = "/users",
    request_body = CreateUser,
    responses(
        (status = 201, description = "User created", body = User),
        (status = 400, description = "Invalid input")
    ),
    tag = "users"
)]
async fn create_user(Json(payload): Json<CreateUser>) -> Json<User> {
    Json(User {
        id: 1,
        name: payload.name,
        email: payload.email,
    })
}
```

## Organizing with Tags

Group related endpoints using tags:

```rust
#[derive(OpenApi)]
#[openapi(
    paths(
        list_users,
        get_user,
        create_user,
        list_orders,
        get_order,
    ),
    components(schemas(User, Order, CreateUser)),
    tags(
        (name = "users", description = "User management"),
        (name = "orders", description = "Order operations")
    )
)]
struct ApiDoc;
```

## Adding API Metadata

```rust
use axum_conf::openapi::{info_full, Contact, License};
use utoipa::openapi::InfoBuilder;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "My API",
        version = "1.0.0",
        description = "REST API for managing users and orders",
        contact(
            name = "API Support",
            email = "support@example.com"
        ),
        license(
            name = "MIT",
            url = "https://opensource.org/licenses/MIT"
        )
    ),
    paths(list_users, get_user),
    components(schemas(User))
)]
struct ApiDoc;
```

## Custom Documentation Path

Use `ScalarConfig` for advanced configuration:

```rust
use axum_conf::openapi::{OpenApiExt, ScalarConfig};

let config = ScalarConfig::new("/api-docs")
    .with_spec_path("/api/openapi.json")
    .with_title("My Custom API");

FluentRouter::without_state(config)?
    .route("/users", get(list_users))
    .with_openapi_config::<ApiDoc>(config)
    // ...
```

This creates:
- Scalar UI at `/api-docs`
- OpenAPI spec at `/api/openapi.json`

## Documenting Error Responses

Use `axum_conf::ErrorResponse` for consistent error documentation:

```rust
use axum_conf::ErrorResponse;

/// Get a user by ID
#[utoipa::path(
    get,
    path = "/users/{id}",
    responses(
        (status = 200, description = "User found", body = User),
        (status = 400, description = "Invalid input", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse)
    )
)]
async fn get_user(Path(id): Path<u64>) -> Result<Json<User>, Error> {
    // ...
}
```

## Security Schemes

Document authentication requirements:

```rust
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

#[derive(OpenApi)]
#[openapi(
    paths(list_users),
    components(
        schemas(User),
        // Removed security_schemes - use modifiers instead
    ),
    modifiers(&SecurityAddon)
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build()
            )
        );
    }
}
```

Then mark endpoints as requiring authentication:

```rust
#[utoipa::path(
    get,
    path = "/users",
    security(
        ("bearer_auth" = [])
    ),
    responses(...)
)]
async fn list_users() -> Json<Vec<User>> { ... }
```

## Complete Example

```rust
use axum::{Json, extract::Path, routing::{get, post}};
use axum_conf::{Config, FluentRouter, Result, Error, ErrorResponse};
use axum_conf::openapi::OpenApiExt;
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

// === Models ===

#[derive(Serialize, Deserialize, ToSchema)]
struct User {
    /// Unique user identifier
    id: u64,
    /// User's display name
    name: String,
    /// User's email address
    email: String,
}

#[derive(Deserialize, ToSchema)]
struct CreateUser {
    /// User's display name (required)
    name: String,
    /// User's email address
    email: String,
}

// === Handlers ===

/// List all users
#[utoipa::path(
    get,
    path = "/users",
    tag = "users",
    responses(
        (status = 200, description = "List of users", body = [User])
    )
)]
async fn list_users() -> Json<Vec<User>> {
    Json(vec![])
}

/// Get a user by ID
#[utoipa::path(
    get,
    path = "/users/{id}",
    tag = "users",
    params(
        ("id" = u64, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User found", body = User),
        (status = 404, description = "Not found", body = ErrorResponse)
    )
)]
async fn get_user(Path(id): Path<u64>) -> Result<Json<User>, Error> {
    Ok(Json(User {
        id,
        name: "Alice".into(),
        email: "alice@example.com".into(),
    }))
}

/// Create a new user
#[utoipa::path(
    post,
    path = "/users",
    tag = "users",
    request_body = CreateUser,
    responses(
        (status = 201, description = "User created", body = User),
        (status = 400, description = "Invalid input", body = ErrorResponse)
    )
)]
async fn create_user(Json(payload): Json<CreateUser>) -> Result<Json<User>, Error> {
    if payload.name.is_empty() {
        return Err(Error::invalid_input("name is required"));
    }
    Ok(Json(User {
        id: 1,
        name: payload.name,
        email: payload.email,
    }))
}

// === OpenAPI Definition ===

#[derive(OpenApi)]
#[openapi(
    info(
        title = "User Service API",
        version = "1.0.0",
        description = "API for managing users"
    ),
    paths(list_users, get_user, create_user),
    components(schemas(User, CreateUser, ErrorResponse)),
    tags(
        (name = "users", description = "User management operations")
    )
)]
struct ApiDoc;

// === Main ===

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/users", get(list_users).post(create_user))
        .route("/users/:id", get(get_user))
        .with_openapi::<ApiDoc>("/docs")
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Tips

1. **Use `ToSchema` derive** - All types in request/response bodies need `#[derive(ToSchema)]`
2. **Document all responses** - Include error responses for complete API contracts
3. **Use tags** - Group related endpoints for better organization
4. **Add descriptions** - Doc comments become OpenAPI descriptions
5. **Version your API** - Include version in the OpenAPI info

## Troubleshooting

### Schema not appearing

Ensure your types derive `ToSchema` and are listed in `components(schemas(...))`:

```rust
#[derive(Serialize, ToSchema)]  // Need ToSchema
struct User { ... }

#[derive(OpenApi)]
#[openapi(
    paths(list_users),
    components(schemas(User))  // Must be listed here
)]
struct ApiDoc;
```

### Path parameters not matching

The `path` in `#[utoipa::path]` must match your axum route:

```rust
// Route uses :id
.route("/users/:id", get(get_user))

// OpenAPI path uses {id}
#[utoipa::path(path = "/users/{id}", ...)]
```

### Missing feature errors

Ensure you have the correct utoipa features:

```toml
utoipa = { version = "5", features = ["axum_extras"] }
```

## Next Steps

- [Error Handling](../error-handling.md) - Document error responses
- [Basic Auth](basic-auth.md) - Add authentication documentation
- [Keycloak](keycloak.md) - Document OIDC security
