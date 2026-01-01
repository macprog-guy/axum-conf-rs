# Middleware Features

This guide covers the individual middleware features that can be enabled via Cargo features.

## Concurrency Limit

**Feature:** `concurrency-limit`

Limits the number of requests being processed simultaneously. When the limit is reached, new requests receive a `503 Service Unavailable` response.

### Configuration

```toml
[http]
max_concurrent_requests = 4096  # Default
```

### How It Works

```
Request #4097 ──► [Concurrency Limit: 4096] ──► 503 Service Unavailable
                         │
                         └─ "Too many concurrent requests"

Requests #1-4096 ──► [Concurrency Limit: 4096] ──► Handler
```

### Use Cases

- **Prevent resource exhaustion** - Limit memory/CPU usage under heavy load
- **Protect databases** - Avoid overwhelming connection pools
- **Maintain latency** - Keep response times predictable
- **Graceful degradation** - Shed load instead of crashing

### Example

```rust
use axum_conf::{Config, FluentRouter};

let mut config = Config::default()
    .with_max_concurrent_requests(1000);  // Limit to 1000 concurrent requests

FluentRouter::without_state(config)?
    .route("/api", get(handler))
    .setup_middleware()
    .await?
    .start()
    .await
```

### Tuning Guidelines

| Workload | Suggested Limit |
|----------|-----------------|
| CPU-bound | 2-4x CPU cores |
| I/O-bound (DB) | 2-4x connection pool size |
| Memory-constrained | Based on per-request memory |
| Default | 4096 |

---

## Payload Limit

**Feature:** `payload-limit`

Rejects requests with bodies larger than the configured limit with `413 Payload Too Large`.

### Configuration

```toml
[http]
max_payload_size_bytes = "1MiB"  # Supports KiB, MiB, GiB
```

### Supported Units

| Unit | Example | Bytes |
|------|---------|-------|
| Bytes | `"1024"` | 1,024 |
| KiB | `"32KiB"` | 32,768 |
| MiB | `"1MiB"` | 1,048,576 |
| GiB | `"1GiB"` | 1,073,741,824 |

### How It Works

```
10 MiB Request ──► [Payload Limit: 1 MiB] ──► 413 Payload Too Large

500 KiB Request ──► [Payload Limit: 1 MiB] ──► Handler
```

### Use Cases

- **Prevent DoS attacks** - Block oversized uploads
- **Protect memory** - Avoid OOM from large bodies
- **API contracts** - Enforce reasonable request sizes

### Example

```rust
let config = Config::default()
    .with_max_payload_size_bytes(5 * 1024 * 1024);  // 5 MiB

// Or via TOML
// max_payload_size_bytes = "5MiB"
```

### Per-Route Limits

For different limits per endpoint, disable the global middleware and use Axum's extractor:

```toml
[http.middleware]
exclude = ["max-payload-size"]
```

```rust
use axum::extract::DefaultBodyLimit;

let app = FluentRouter::without_state(config)?
    .route("/upload", post(upload).layer(DefaultBodyLimit::max(50 * 1024 * 1024)))  // 50 MiB
    .route("/api", post(api_handler));  // Uses default
```

---

## Path Normalization

**Feature:** `path-normalization`

Automatically normalizes request paths by removing trailing slashes, ensuring consistent routing regardless of how clients format URLs.

### Configuration

```toml
[http]
trim_trailing_slash = true  # Default
```

### How It Works

```
GET /users/ ──► [Path Normalization] ──► /users ──► Handler
GET /users  ──► [Path Normalization] ──► /users ──► Handler
```

### Benefits

- **Consistent routing** - `/users` and `/users/` route to same handler
- **Clean URLs** - No duplicate routes needed
- **SEO friendly** - Prevents duplicate content issues

### Example

```rust
// With path normalization enabled, you only need one route
FluentRouter::without_state(config)?
    .route("/users", get(list_users))  // Handles both /users and /users/
```

### Disabling

If you need to distinguish between `/path` and `/path/`:

```toml
[http]
trim_trailing_slash = false
```

---

## Sensitive Headers

**Feature:** `sensitive-headers`

Marks sensitive headers (like `Authorization`) to prevent them from appearing in logs, protecting credentials from exposure.

### Protected Headers

By default, protects:
- `Authorization` - Bearer tokens, Basic auth credentials, API keys

### How It Works

When logging middleware logs headers, sensitive values are redacted:

```
# Without sensitive-headers feature:
headers: {"authorization": "Bearer eyJhbGciOiJIUzI1NiIs..."}

# With sensitive-headers feature:
headers: {"authorization": "[REDACTED]"}
```

### Configuration

This middleware is automatically enabled when the feature is active. To disable:

```toml
[http.middleware]
exclude = ["sensitive-headers"]
```

### Security Benefits

- **Prevent credential leaks** - Tokens never appear in logs
- **Audit compliance** - Helps meet security standards
- **Safe debugging** - Can enable verbose logging without risk

---

## API Versioning

**Feature:** `api-versioning`

Automatically extracts API version from requests and makes it available to handlers. Supports multiple version detection methods.

### Configuration

```toml
[http]
default_api_version = 1  # Fallback when no version specified
```

### Version Detection Order

The middleware checks for version in this order:

1. **Path-based**: `/v1/users`, `/api/v2/users`
2. **Header-based**: `X-API-Version: 2` or `Accept: application/vnd.api+json;version=2`
3. **Query parameter**: `/users?version=1`
4. **Default**: Uses `default_api_version` from config

### How It Works

```
GET /v2/users ──► [API Versioning] ──► Extension(ApiVersion(2)) ──► Handler

GET /users
X-API-Version: 3 ──► [API Versioning] ──► Extension(ApiVersion(3)) ──► Handler

GET /users?version=4 ──► [API Versioning] ──► Extension(ApiVersion(4)) ──► Handler

GET /users ──► [API Versioning] ──► Extension(ApiVersion(1)) ──► Handler (default)
```

### Using in Handlers

```rust
use axum::Extension;
use axum_conf::ApiVersion;

async fn get_users(Extension(version): Extension<ApiVersion>) -> String {
    match version.as_u32() {
        1 => handle_v1(),
        2 => handle_v2(),
        _ => format!("Unsupported version: {}", version),
    }
}

fn handle_v1() -> String {
    // V1 response format
    r#"{"users": [...]}"#.to_string()
}

fn handle_v2() -> String {
    // V2 response format with pagination
    r#"{"data": {"users": [...]}, "meta": {"page": 1}}"#.to_string()
}
```

### Version Routing

Route to different handlers based on version:

```rust
async fn users_handler(Extension(version): Extension<ApiVersion>) -> Response {
    match version.as_u32() {
        1 => v1::list_users().await.into_response(),
        2 => v2::list_users().await.into_response(),
        v => (
            StatusCode::BAD_REQUEST,
            format!("API version {} not supported", v)
        ).into_response(),
    }
}
```

### Separate Route Modules

For complex APIs, use nested routers:

```rust
mod v1 {
    pub fn routes() -> Router {
        Router::new()
            .route("/users", get(list_users))
            .route("/users/:id", get(get_user))
    }
}

mod v2 {
    pub fn routes() -> Router {
        Router::new()
            .route("/users", get(list_users_paginated))
            .route("/users/:id", get(get_user_extended))
    }
}

FluentRouter::without_state(config)?
    .nest("/v1", v1::routes())
    .nest("/v2", v2::routes())
    .setup_middleware()
    .await?
```

---

## Enabling Features

Add to your `Cargo.toml`:

```toml
[dependencies]
axum-conf = { version = "0.3", features = [
    "concurrency-limit",
    "payload-limit",
    "path-normalization",
    "sensitive-headers",
    "api-versioning",
]}
```

Or use a convenience feature group:

```toml
[dependencies]
axum-conf = { version = "0.3", features = ["production"] }
# Includes: metrics, rate-limiting, security-headers, compression, cors
```

## Middleware Control

### Excluding Specific Middleware

```toml
[http.middleware]
exclude = ["path-normalization", "api-versioning"]
```

### Including Only Specific Middleware

```toml
[http.middleware]
include = ["concurrency-limit", "payload-limit", "request-id"]
```

## Next Steps

- [Security Middleware](security.md) - Rate limiting, CORS, security headers
- [Performance Middleware](performance.md) - Compression, timeouts
- [Observability Middleware](observability.md) - Logging, metrics, tracing
