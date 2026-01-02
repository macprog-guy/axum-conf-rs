# Architecture

This guide explains how axum-conf works under the hood. Understanding the architecture helps you make better configuration decisions and debug issues effectively.

## Overview

axum-conf wraps [Axum](https://github.com/tokio-rs/axum) with a configuration-driven builder pattern. It manages middleware ordering, health endpoints, and production concerns so you can focus on your application logic.

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Your Application                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────────┐                                               │
│  │   config/*.toml  │  Configuration files per environment          │
│  └────────┬─────────┘                                               │
│           │                                                         │
│           ▼                                                         │
│  ┌──────────────────┐                                               │
│  │     Config       │  Parsed configuration with validation         │
│  └────────┬─────────┘                                               │
│           │                                                         │
│           ▼                                                         │
│  ┌──────────────────┐                                               │
│  │  FluentRouter    │  Builder that configures Router + Middleware  │
│  └────────┬─────────┘                                               │
│           │ .route() .merge() .nest()                               │
│           │ .setup_middleware()                                     │
│           ▼                                                         │
│  ┌──────────────────┐                                               │
│  │  Axum Router     │  Standard Axum router with layers applied     │
│  └────────┬─────────┘                                               │
│           │ .start()                                                │
│           ▼                                                         │
│  ┌──────────────────┐                                               │
│  │  Tokio Server    │  HTTP/1.1 and HTTP/2 with graceful shutdown   │
│  └──────────────────┘                                               │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Components

### Config

The `Config` struct holds all configuration values. It can be loaded from:

1. **TOML files** - `Config::default()` loads from `config/{RUST_ENV}.toml`
2. **TOML strings** - `config_str.parse::<Config>()`
3. **Builder pattern** - `Config::default().with_bind_port(8080)`

```rust
// From file (recommended)
let config = Config::default();  // Uses RUST_ENV

// From string
let config: Config = r#"
    [http]
    bind_port = 3000
"#.parse()?;

// From builder
let config = Config::default()
    .with_bind_port(3000)
    .with_compression(true);
```

### FluentRouter

`FluentRouter` is a builder that wraps Axum's `Router`. It provides:

- **Middleware setup methods** - `setup_*()` methods for each middleware type
- **Configuration access** - Reads from `Config` to determine behavior
- **State management** - Handles generic application state

```rust
// Lifecycle
FluentRouter::without_state(config)?    // 1. Create
    .route("/api", get(handler))        // 2. Add routes
    .setup_middleware().await?          // 3. Apply middleware
    .start().await                      // 4. Run server
```

### Middleware Stack

axum-conf applies middleware in a specific order. Understanding this order is crucial for debugging and customization.

## Request/Response Flow

When a request arrives, it flows through the middleware stack from **outside to inside**, then your handler runs, and the response flows back **inside to outside**:

```
                         CLIENT REQUEST
                              │
                              ▼
    ┌────────────────────────────────────────────────────────────────┐
    │                    OUTERMOST LAYERS                            │
    │                 (execute first on request)                     │
    ├────────────────────────────────────────────────────────────────┤
    │                                                                │
    │  18. Panic Catching ──────── Catches panics, returns 500       │
    │           │                                                    │
    │           ▼                                                    │
    │  17. Liveness (/live) ────── Simple health check               │
    │           │                                                    │
    │           ▼                                                    │
    │  16. Request ID ──────────── Generates/extracts UUIDv7         │
    │           │                                                    │
    │           ▼                                                    │
    │  15. Rate Limiting ───────── Rejects if over limit (429)       │
    │           │                                                    │
    │           ▼                                                    │
    │  14. Timeout ─────────────── Starts timeout timer              │
    │           │                                                    │
    │           ▼                                                    │
    │  13. Readiness (/ready) ──── Database health check             │
    │           │                                                    │
    │           ▼                                                    │
    │  12. Metrics ─────────────── Records request start             │
    │           │                                                    │
    │           ▼                                                    │
    │  11. Logging ─────────────── Creates trace span                │
    │                                                                │
    ├────────────────────────────────────────────────────────────────┤
    │                     MIDDLE LAYERS                              │
    │                  (transform request/response)                  │
    ├────────────────────────────────────────────────────────────────┤
    │                                                                │
    │  10. Security Headers ────── Prepares response headers         │
    │           │                                                    │
    │           ▼                                                    │
    │   9. CORS ────────────────── Handles OPTIONS preflight         │
    │           │                                                    │
    │           ▼                                                    │
    │   8. API Versioning ──────── Extracts version to extensions    │
    │           │                                                    │
    │           ▼                                                    │
    │   7. Sensitive Headers ───── Marks Authorization as sensitive  │
    │           │                                                    │
    │           ▼                                                    │
    │   6. Path Normalization ──── Removes trailing slashes          │
    │           │                                                    │
    │           ▼                                                    │
    │   5. Compression ─────────── Decompresses request body         │
    │           │                                                    │
    │           ▼                                                    │
    │   4. Payload Size ────────── Rejects if too large (413)        │
    │           │                                                    │
    │           ▼                                                    │
    │   3. Concurrency Limit ───── Rejects if at capacity (503)      │
    │           │                                                    │
    │           ▼                                                    │
    │   2. Deduplication ───────── Checks for duplicate request ID   │
    │                                                                │
    ├────────────────────────────────────────────────────────────────┤
    │                    INNERMOST LAYERS                            │
    │                 (execute last on request)                      │
    ├────────────────────────────────────────────────────────────────┤
    │                                                                │
    │   1. Authentication ──────── Validates JWT/credentials         │
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

## Why This Order?

The middleware order is intentional:

| Layer | Position | Reasoning |
|-------|----------|-----------|
| **Panic Catching** | Outermost | Must catch ALL panics from any layer |
| **Liveness** (`/live`) | Very early | Simple check, always accessible, even during panics |
| **Request ID** | Very early | All requests get IDs, even rejected ones |
| **Rate Limiting** | Very early | Reject excess traffic before expensive work |
| **Timeout** | Early | Set deadline before work begins |
| **Readiness** (`/ready`) | After timeout | Database check benefits from timeout/rate limiting |
| **Metrics/Logging** | Middle | Measure/log ALL requests with request IDs |
| **CORS** | Middle | Handle preflight before authentication |
| **Authentication** | Innermost | After infrastructure, before business logic |

## Configuration Loading

```
┌─────────────────────────────────────────────────────────────────┐
│                    Configuration Sources                        │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
    ┌───────────┐      ┌───────────┐      ┌───────────────┐
    │   TOML    │      │  String   │      │    Builder    │
    │   File    │      │  Inline   │      │    Pattern    │
    │           │      │           │      │               │
    │ config/   │      │ r#"..."#  │      │ .with_*()     │
    │ dev.toml  │      │ .parse()  │      │ methods       │
    └─────┬─────┘      └─────┬─────┘      └───────┬───────┘
          │                  │                    │
          └──────────────────┼────────────────────┘
                             ▼
    ┌────────────────────────────────────────────────────────────┐
    │              Environment Variable Substitution             │
    │                                                            │
    │         {{ DATABASE_URL }}  →  postgres://...              │
    │         {{ API_SECRET }}    →  sk-xxxxx                    │
    │                                                            │
    └────────────────────────────────────────────────────────────┘
                             │
                             ▼
    ┌────────────────────────────────────────────────────────────┐
    │                     Config Struct                          │
    │  ┌─────────────┬─────────────────┬───────────────────┐     │
    │  │    http     │    database     │     logging       │     │
    │  │             │   (optional)    │                   │     │
    │  │ • bind_port │ • url           │ • format          │     │
    │  │ • timeout   │ • pool_size     │ • opentelemetry   │     │
    │  │ • cors      │ • idle_timeout  │                   │     │
    │  │ • middleware│                 │                   │     │
    │  └─────────────┴─────────────────┴───────────────────┘     │
    └────────────────────────────────────────────────────────────┘
                             │
                             ▼
    ┌────────────────────────────────────────────────────────────┐
    │                      Validation                            │
    │                                                            │
    │  • Database URL format (if postgres feature)               │
    │  • OIDC configuration completeness (if keycloak feature)   │
    │  • Static directory constraints                            │
    │  • Bind address format                                     │
    │                                                            │
    └────────────────────────────────────────────────────────────┘
```

## Graceful Shutdown

When the server receives SIGTERM or SIGINT:

```
┌────────────────┐     ┌────────────────┐     ┌────────────────┐
│  Signal        │     │  Stop          │     │  Grace         │
│  Received      │────▶│  Accepting     │────▶│  Period        │
│  (SIGTERM)     │     │  New Conns     │     │  (30s default) │
└────────────────┘     └────────────────┘     └────────┬───────┘
                                                       │
                       ┌───────────────────────────────┘
                       ▼
              ┌────────────────┐
              │  In-flight     │
              │  requests      │
              │  complete      │
              └────────┬───────┘
                       │
                       ▼
              ┌────────────────┐
              │  Server        │
              │  exits         │
              └────────────────┘
```

## Feature Flags

Features enable additional functionality:

```
┌─────────────────────────────────────────────────────────────────┐
│                        Feature Graph                            │
└─────────────────────────────────────────────────────────────────┘

    postgres ──────────▶ rustls (TLS for database)
        │
        └──────────────▶ sqlx-postgres (connection pooling)

    keycloak ──────────▶ session (cookie management)
        │
        └──────────────▶ axum-keycloak-auth (JWT validation)

    opentelemetry ─────▶ tracing-opentelemetry
        │
        └──────────────▶ opentelemetry-otlp (OTLP export)

    basic-auth ────────▶ base64 (credential encoding)
```

## Next Steps

- [Configuration Overview](configuration/overview.md) - Deep dive into configuration
- [Middleware Overview](middleware/overview.md) - Detailed middleware reference
- [Troubleshooting](troubleshooting.md) - Common issues and solutions
