# Middleware Overview

axum-conf provides a comprehensive middleware stack that handles common concerns like security, observability, and performance. This guide explains how the middleware system works and how to customize it.

## Default Setup

The simplest way to use middleware is with `setup_middleware()`:

```rust
use axum::routing::get;
use axum_conf::{Config, FluentRouter, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();

    FluentRouter::without_state(config)?
        .route("/", get(|| async { "Hello" }))
        .setup_middleware()  // Applies all middleware in correct order
        .await?
        .start()
        .await
}
```

This one line applies 17+ middleware layers in the correct order.

## Middleware Stack

When a request arrives, it flows through middleware from **outside to inside**, then your handler runs, and the response flows back **inside to outside**:

```
                         CLIENT REQUEST
                              │
                              ▼
    ┌────────────────────────────────────────────────────────────────┐
    │                                                                │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  17. PANIC CATCHING                                      │  │
    │  │      Catches all panics, returns 500, server continues   │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  16. RATE LIMITING                                       │  │
    │  │      Per-IP rate limiting, returns 429 if exceeded       │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  15. TIMEOUT                                             │  │
    │  │      Enforces request timeout, returns 408               │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  14. METRICS (Prometheus)                                │  │
    │  │      Records request count, duration, status, size       │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  13. LOGGING                                             │  │
    │  │      Creates trace span with method, path, request ID    │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  12. SECURITY HEADERS (Helmet)                           │  │
    │  │      X-Content-Type-Options, X-Frame-Options             │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  11. CORS                                                │  │
    │  │      Cross-origin requests, preflight handling           │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │  10. API VERSIONING                                      │  │
    │  │      Extracts version from path/header/query             │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   9. REQUEST ID                                          │  │
    │  │      Generates UUIDv7 or extracts from header            │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   8. SENSITIVE HEADERS                                   │  │
    │  │      Marks Authorization header as sensitive             │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   7. PATH NORMALIZATION                                  │  │
    │  │      Removes trailing slashes                            │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   6. COMPRESSION                                         │  │
    │  │      gzip, brotli, deflate, zstd                         │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   5. PAYLOAD SIZE                                        │  │
    │  │      Rejects oversized requests (413)                    │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   4. CONCURRENCY LIMIT                                   │  │
    │  │      Limits concurrent requests (503 if exceeded)        │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   3. REQUEST DEDUPLICATION                               │  │
    │  │      Prevents duplicate request processing               │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   2. AUTHENTICATION (OIDC/Basic Auth)                    │  │
    │  │      Validates JWT or credentials                        │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                           │                                    │
    │  ┌──────────────────────────────────────────────────────────┐  │
    │  │   1. HEALTH ENDPOINTS                                    │  │
    │  │      /live and /ready routes (always accessible)         │  │
    │  └──────────────────────────────────────────────────────────┘  │
    │                                                                │
    └────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                     ┌────────────────┐
                     │  YOUR HANDLER  │
                     └────────────────┘
                              │
                              ▼
                    (Response flows back up)
```

## Controlling Middleware

### Exclude Specific Middleware

Disable certain middleware while keeping others:

```toml
[http.middleware]
exclude = [
    "rate-limiting",     # Disable rate limiting
    "compression",       # Disable compression
]
```

### Include Only Specific Middleware

Enable only the middleware you list:

```toml
[http.middleware]
include = [
    "logging",
    "metrics",
    "catch-panic",
    "liveness",
    "readiness"
]
```

### Available Middleware Names

| Name | Description | Default |
|------|-------------|---------|
| `catch-panic` | Panic recovery | Enabled |
| `rate-limiting` | Per-IP rate limiting | Enabled |
| `timeout` | Request timeout | Enabled if configured |
| `metrics` | Prometheus metrics | Enabled |
| `logging` | Request logging | Enabled |
| `security-headers` | X-Frame-Options, etc. | Enabled |
| `cors` | Cross-origin handling | Enabled |
| `api-versioning` | Version extraction | Enabled |
| `request-id` | UUIDv7 generation | Enabled |
| `sensitive-headers` | Header protection | Enabled |
| `path-normalization` | Trailing slash removal | Enabled |
| `compression` | Response compression | Configurable |
| `max-payload-size` | Request size limit | Enabled |
| `concurrency-limit` | Request throttling | Enabled |
| `request-deduplication` | Duplicate prevention | Enabled if configured |
| `oidc` | JWT authentication | Enabled if configured |
| `basic-auth` | Basic/API key auth | Enabled if configured |
| `liveness` | /live endpoint | Enabled |
| `readiness` | /ready endpoint | Enabled |
| `session` | Cookie sessions | Enabled if feature on |
| `opentelemetry` | Distributed tracing | Enabled if configured |

## Manual Middleware Setup

For complete control, call individual `setup_*` methods:

```rust
use axum_conf::{Config, FluentRouter, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();

    FluentRouter::without_state(config)?
        .route("/", get(handler))
        // Apply middleware manually in YOUR order
        .setup_catch_panic()
        .setup_logging()
        .setup_metrics()
        .setup_request_id()
        .setup_liveness_readiness()
        .start()
        .await
}
```

> **Warning**: When using manual setup, you must ensure correct ordering. Incorrect ordering can cause security vulnerabilities or unexpected behavior.

## Middleware Categories

### Security Middleware
- Rate limiting
- Authentication (OIDC, Basic Auth)
- Security headers (Helmet)
- CORS

See [Security Middleware](security.md) for details.

### Observability Middleware
- Request logging
- Prometheus metrics
- OpenTelemetry tracing
- Request ID generation

See [Observability Middleware](observability.md) for details.

### Performance Middleware
- Compression
- Timeout
- Payload size limits
- Concurrency limits

See [Performance Middleware](performance.md) for details.

## Why This Order?

The middleware order is carefully designed:

| Layer | Position | Reason |
|-------|----------|--------|
| Panic catching | Outermost | Catches panics from ALL layers |
| Rate limiting | Early | Reject excess before expensive processing |
| Timeout | Early | Set deadline before work begins |
| Metrics/Logging | Early | Observe ALL requests, including rejected |
| Security headers | Middle | Add to all responses |
| CORS | Middle | Handle preflight before auth |
| Auth | Late | After infrastructure, before business logic |
| Health checks | Innermost | Always accessible, even during issues |

## Next Steps

- [Security Middleware](security.md) - Rate limiting, auth, CORS
- [Observability Middleware](observability.md) - Logging, metrics, tracing
- [Performance Middleware](performance.md) - Compression, timeouts
