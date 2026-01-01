# Error Handling

axum-conf provides a structured error handling system with automatic HTTP status code mapping and JSON error responses.

## Overview

The error system consists of:
- `Error` - The main error type that wraps all errors
- `ErrorKind` - An enum categorizing error types
- `ErrorResponse` - A JSON-serializable error response

## Error Kinds

| Kind | HTTP Status | Error Code | Description |
|------|-------------|------------|-------------|
| `Database` | 503 Service Unavailable | `DATABASE_ERROR` | Database connection, query, or pool issues |
| `Authentication` | 401 Unauthorized | `AUTH_ERROR` | Invalid credentials or tokens |
| `Configuration` | 500 Internal Server Error | `CONFIG_ERROR` | Invalid configuration or missing values |
| `Tls` | 500 Internal Server Error | `TLS_ERROR` | TLS/certificate issues |
| `Io` | 500 Internal Server Error | `IO_ERROR` | File or network I/O errors |
| `InvalidInput` | 400 Bad Request | `INVALID_INPUT` | Invalid request data |
| `CircuitBreakerOpen` | 503 Service Unavailable | `CIRCUIT_BREAKER_OPEN` | Circuit breaker is rejecting requests |
| `CircuitBreakerFailed` | 502 Bad Gateway | `CIRCUIT_BREAKER_CALL_FAILED` | Upstream call failed |
| `Internal` | 500 Internal Server Error | `INTERNAL_ERROR` | Unexpected internal errors |

## Creating Errors

### Using Convenience Constructors

```rust
use axum_conf::Error;

// Create common error types
let err = Error::internal("unexpected state");
let err = Error::invalid_input("missing required field 'name'");
let err = Error::database("connection timeout");
let err = Error::authentication("invalid token");
let err = Error::config("missing DATABASE_URL");
```

### Using Error::new()

For full control over error creation:

```rust
use axum_conf::{Error, ErrorKind};

// Wrap an existing error
let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
let err = Error::new(ErrorKind::Io, io_err);
```

## Handling Errors in Handlers

Errors automatically convert to HTTP responses:

```rust
use axum::{Json, response::IntoResponse};
use axum_conf::{Error, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

async fn create_user(Json(payload): Json<CreateUser>) -> Result<Json<User>> {
    // Validate input
    if payload.name.is_empty() {
        return Err(Error::invalid_input("name cannot be empty"));
    }

    if !payload.email.contains('@') {
        return Err(Error::invalid_input("invalid email format"));
    }

    // Create user...
    Ok(Json(User { id: 1, name: payload.name }))
}
```

## JSON Error Response Format

When an error is returned from a handler, it's automatically serialized to JSON:

```json
{
  "error_code": "INVALID_INPUT",
  "message": "name cannot be empty"
}
```

With optional details:

```json
{
  "error_code": "DATABASE_ERROR",
  "message": "connection timeout",
  "details": "Failed to connect to postgres://localhost:5432/mydb after 30s"
}
```

## Matching on Error Kinds

Use `error.kind()` to determine how to handle errors:

```rust
use axum_conf::{Error, ErrorKind};

fn handle_error(err: Error) {
    match err.kind() {
        ErrorKind::Database => {
            // Maybe retry or use fallback
            eprintln!("Database issue, will retry: {}", err);
        }
        ErrorKind::InvalidInput => {
            // Client error, no retry
            eprintln!("Bad request: {}", err);
        }
        ErrorKind::CircuitBreakerOpen => {
            // Service degraded, use cached data
            eprintln!("Service unavailable: {}", err);
        }
        _ => {
            eprintln!("Unexpected error: {}", err);
        }
    }
}
```

## Automatic Error Conversions

Common error types automatically convert to `axum_conf::Error`:

```rust
use axum_conf::Result;

// std::io::Error -> Error (Io kind)
fn read_file(path: &str) -> Result<String> {
    Ok(std::fs::read_to_string(path)?) // ? converts io::Error
}

// url::ParseError -> Error (InvalidInput kind)
fn parse_url(s: &str) -> Result<url::Url> {
    Ok(s.parse()?)
}

// toml::de::Error -> Error (Configuration kind)
fn parse_config(s: &str) -> Result<Config> {
    Ok(s.parse()?)
}

// With postgres feature
#[cfg(feature = "postgres")]
fn query_db(pool: &PgPool) -> Result<Vec<User>> {
    // sqlx::Error -> Error (Database kind)
    Ok(sqlx::query_as!(User, "SELECT * FROM users").fetch_all(pool).await?)
}
```

## Custom Error Responses

For more control over error responses:

```rust
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_conf::ErrorResponse;

async fn custom_error_handler() -> Response {
    let error_response = ErrorResponse::new("CUSTOM_ERROR", "Something specific happened")
        .with_details("Additional context about the error");

    (StatusCode::UNPROCESSABLE_ENTITY, Json(error_response)).into_response()
}
```

## Circuit Breaker Errors

When using the circuit breaker feature:

```rust
use axum_conf::{Error, ErrorKind};

#[cfg(feature = "circuit-breaker")]
async fn call_external_service(breaker: &CircuitBreaker) -> Result<Response> {
    breaker
        .call("payment-api", async {
            // Make external call
            external_client.get("/api/payment").await
        })
        .await
        .map_err(|e| match e.kind() {
            ErrorKind::CircuitBreakerOpen => {
                // Circuit is open, service is degraded
                Error::circuit_breaker_open("payment-api")
            }
            ErrorKind::CircuitBreakerFailed => {
                // Call failed, circuit may open soon
                Error::circuit_breaker_failed("payment API timeout")
            }
            _ => e,
        })
}
```

## Error Logging

Errors are automatically logged when converted to responses:

```rust
// When Error::into_response() is called, this is logged:
// ERROR error_code="INVALID_INPUT" message="name cannot be empty" status=400 "Error occurred"
```

## Best Practices

1. **Use specific error kinds** - Choose the most appropriate error kind for better client handling
2. **Provide helpful messages** - Include context about what went wrong and how to fix it
3. **Don't expose internals** - Avoid leaking sensitive implementation details in error messages
4. **Log server errors** - Database and internal errors should be logged for debugging
5. **Use ErrorKind matching** - Handle errors appropriately based on their kind
6. **Add details sparingly** - Only include `details` when it adds value for debugging

## Example: Complete Handler with Error Handling

```rust
use axum::{
    Json,
    extract::{Path, State},
};
use axum_conf::{Error, Result};
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    db: PgPool,
}

async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
) -> Result<Json<User>> {
    // Validate input
    if user_id <= 0 {
        return Err(Error::invalid_input("user_id must be positive"));
    }

    // Query database
    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", user_id)
        .fetch_optional(&state.db)
        .await?  // Database errors automatically converted
        .ok_or_else(|| Error::invalid_input(format!("user {} not found", user_id)))?;

    Ok(Json(user))
}
```
