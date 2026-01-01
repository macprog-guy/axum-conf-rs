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

## Static File Serving

axum-conf includes built-in static file serving for assets, SPAs, and protected downloads. Static files are served *inside* the middleware stack, so they benefit from logging, metrics, compression, and security headers.

### Basic Configuration

```toml
# Serve public assets at /static/*
[[http.directories]]
directory = "./public"
route = "/static"

# SPA fallback - serve index.html for unmatched routes
[[http.directories]]
directory = "./dist"
fallback = true
```

### Protected Directories

Serve files that require authentication (requires `keycloak` feature):

```toml
[[http.directories]]
directory = "./downloads"
route = "/downloads"
protected = true
```

### Caching

Add Cache-Control headers for better performance:

```toml
[[http.directories]]
directory = "./assets"
route = "/assets"
cache_max_age = 86400  # 1 day in seconds
```

### Features

- **Pre-compressed content**: Automatically serves `.br` and `.gz` files when available
- **Index files**: Serves `index.html` for directory requests
- **Protected directories**: Require OIDC authentication (cannot be used with fallback)
- **Cache headers**: Configurable Cache-Control max-age

> **Note**: Fallback directories cannot be protected. Only one fallback directory is allowed per application.

### Pre-Compressed Files

axum-conf uses `tower-http`'s `ServeDir` which automatically serves pre-compressed files when:
1. The client sends an `Accept-Encoding` header with `br` or `gzip`
2. A corresponding `.br` or `.gz` file exists

#### Creating Pre-Compressed Assets

Use your build pipeline to generate compressed versions:

```bash
# Brotli compression (best for web)
find ./dist -type f \( -name "*.js" -o -name "*.css" -o -name "*.html" -o -name "*.svg" \) \
    -exec brotli -f {} \;

# Gzip fallback
find ./dist -type f \( -name "*.js" -o -name "*.css" -o -name "*.html" -o -name "*.svg" \) \
    -exec gzip -kf {} \;
```

Result:
```
dist/
├── app.js
├── app.js.br      # Served when Accept-Encoding: br
├── app.js.gz      # Served when Accept-Encoding: gzip
├── styles.css
├── styles.css.br
└── styles.css.gz
```

#### Why Pre-Compress?

| Approach | CPU Usage | Latency | Best For |
|----------|-----------|---------|----------|
| Dynamic compression | High | Higher | Changing content |
| Pre-compressed | None | Lowest | Static assets |

Pre-compression is ideal for production deployments where assets don't change at runtime.

### Caching Strategies

#### Immutable Assets (Hashed Filenames)

For files with content hashes in names (e.g., `app.a1b2c3.js`):

```toml
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000  # 1 year
```

These can be cached forever because the filename changes when content changes.

#### Versioned Assets

For files that may change between deployments:

```toml
[[http.directories]]
directory = "./dist"
route = "/static"
cache_max_age = 86400  # 1 day
```

#### No-Cache for HTML

HTML files should typically not be cached so users get the latest version:

```toml
# Note: cache_max_age = 0 tells browsers to revalidate
[[http.directories]]
directory = "./dist"
fallback = true
# cache_max_age not set = no Cache-Control header
```

### SPA Configuration

For single-page applications with client-side routing:

```toml
# Static assets with long cache
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000

# SPA fallback - serves index.html for all unmatched routes
[[http.directories]]
directory = "./dist"
fallback = true
```

When a user navigates to `/dashboard`:
1. No `/dashboard` route in your API
2. Fallback directory serves `dist/index.html`
3. Client-side router handles the route

### CDN Integration

When using a CDN (CloudFront, Cloudflare, etc.):

```toml
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000  # CDN caches for 1 year
```

Configure your CDN to:
1. **Cache based on `Accept-Encoding`** - Vary responses by encoding
2. **Pass through `Cache-Control`** - Honor max-age headers
3. **Strip cookies** - Static files don't need cookies

Example CloudFront behavior:
- **Path Pattern**: `/assets/*`
- **Cache Policy**: CachingOptimized
- **Origin Request Policy**: CORS-S3Origin (if needed)

### Multiple Directory Example

A complete production setup:

```toml
# API routes handle /api/* (defined in code)

# Hashed assets - cache forever
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000

# Fonts - cache for 1 year
[[http.directories]]
directory = "./fonts"
route = "/fonts"
cache_max_age = 31536000

# Protected downloads - require auth
[[http.directories]]
directory = "./downloads"
route = "/downloads"
protected = true

# SPA fallback - no cache
[[http.directories]]
directory = "./dist"
fallback = true
```

### Performance Tips

1. **Pre-compress all text assets** - HTML, CSS, JS, SVG, JSON
2. **Use content hashes** - Enable aggressive caching
3. **Set appropriate max-age** - Balance freshness vs. performance
4. **Don't compress images** - JPEG, PNG, WebP are already compressed
5. **Use a CDN** - Serve static files from edge locations

### Troubleshooting

**Files not being served:**
- Check the `directory` path is correct (relative to working directory)
- Ensure files have read permissions
- Look for errors in startup logs

**Compression not working:**
- Verify `.br`/`.gz` files exist alongside originals
- Check `Accept-Encoding` header is being sent
- Enable the `compression` feature if using dynamic compression

**Cache not working:**
- Verify `cache_max_age` is set
- Check browser DevTools Network tab for Cache-Control header
- Clear browser cache for testing

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
