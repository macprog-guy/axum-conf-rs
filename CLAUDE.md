# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

axum-conf is a batteries-included Rust library for building production-ready web services with Axum, designed for Kubernetes deployments. It provides middleware, configuration, health checks, and observability out of the box.

## Build & Test Commands

```bash
# Run all tests (requires all features enabled)
make test
# or: cargo test --all-features

# Run only unit tests (faster, no external dependencies)
cargo test --all-features --lib

# Run only integration tests (requires Docker for testcontainers)
cargo test --all-features --test '*'

# Run a single test by name
cargo test --all-features test_name

# Lint with clippy
make lint
# or: cargo clippy --all-targets --all-features -- -D warnings

# Run benchmarks
make bench

# Generate documentation
make docs
# or: cargo doc --no-deps --open

# Security audit
make audit

# Generate test coverage report
make coverage
```

## Cargo Features

The library uses feature flags to enable optional functionality:

### Core Features
- `postgres` - PostgreSQL with connection pooling (enables `rustls`)
- `keycloak` - OIDC/Keycloak authentication (enables `session`)
- `session` - Cookie-based session management (in-memory store)
- `session-postgres` - PostgreSQL-backed session store, reusing the `[database]` pool (enables `session` + `postgres`)
- `session-redis` - Redis-backed session store (enables `session`)
- `opentelemetry` - Distributed tracing
- `rustls` - TLS support
- `basic-auth` - HTTP Basic Auth and API key authentication
- `circuit-breaker` - Per-target circuit breaker pattern for external service resilience
- `openapi` - OpenAPI spec generation via utoipa

### Middleware Features (High Impact)
- `metrics` - Prometheus metrics endpoint
- `rate-limiting` - Per-IP rate limiting with tower_governor
- `security-headers` - Security headers via axum-helmet
- `deduplication` - Request deduplication by request ID

### Middleware Features (Medium Impact)
- `compression` - gzip/brotli/zstd compression
- `cors` - CORS handling
- `api-versioning` - API version extraction from path/header/query

### Middleware Features (Low Impact)
- `concurrency-limit` - Max concurrent requests
- `path-normalization` - Trailing slash normalization
- `sensitive-headers` - Authorization header redaction in logs
- `payload-limit` - Request body size limits

### Convenience Feature Groups
- `full` - All features enabled
- `production` - metrics, rate-limiting, security-headers, compression, cors

Default features are disabled. Most tests require `--all-features`.

## Architecture

### Core Components

- **`FluentRouter`** (`src/fluent/mod.rs`): Main entry point. Builder-pattern wrapper around `axum::Router` that applies middleware in the correct order via `setup_middleware()`.

- **`Config`** (`src/config/mod.rs`): Configuration management loaded from TOML files (`config/{RUST_ENV}.toml`) with environment variable substitution using `{{ VAR_NAME }}` syntax.

### Middleware Stack Order

Middleware is added **innermost-to-outermost** by `setup_middleware()` (`src/fluent/builder.rs`):
the last layer added is the outermost and executes **first** on an incoming request. The table
below lists the application order (position 1 = innermost). It is the single source of truth and is
generated from `MIDDLEWARE_ORDER` in `src/fluent/tests/middleware/ordering.rs`; the
`claude_md_table_matches_generated` test fails if this table drifts from that list, so regenerate it
(copy the test's expected output) rather than editing it by hand.

<!-- BEGIN GENERATED: middleware-order -->
| # | Setup step | Responsibility | Feature |
| --: | --- | --- | --- |
| 1 | `setup_protected_files` | Protected static files (added before auth so the auth `route_layer` covers them) | — |
| 2 | `setup_browser_login_redirect` | Redirect unauthenticated browsers to the login route | `keycloak` |
| 3 | `setup_oidc` | OIDC authentication (bearer JWT and/or auth-code identity) | `keycloak` |
| 4 | `setup_basic_auth` | HTTP Basic Auth and API-key authentication | `basic-auth` |
| 5 | `setup_proxy_oidc` | Reverse-proxy header authentication (fail-closed in production) | — |
| 6 | `setup_public_files` | Public static files (added after auth so they need no credentials) | — |
| 7 | `setup_oidc_routes` | OIDC login / callback / logout routes | `keycloak` |
| 8 | `setup_user_span` | Record the authenticated username on the tracing span | — |
| 9 | `setup_session_handling` | Session cookie store (wraps the auth layers) | `session` |
| 10 | `setup_deduplication` | Request deduplication by request id | `deduplication` |
| 11 | `setup_concurrency_limit` | Max concurrent in-flight requests | `concurrency-limit` |
| 12 | `setup_max_payload_size` | Request body size limit | `payload-limit` |
| 13 | `setup_compression` | Response compression / request decompression | `compression` |
| 14 | `setup_path_normalization` | Trailing-slash path normalization | `path-normalization` |
| 15 | `setup_sensitive_headers` | Mark sensitive headers for redaction in logs | `sensitive-headers` |
| 16 | `setup_api_versioning` | Extract the API version from path / header / query | `api-versioning` |
| 17 | `setup_cors` | CORS preflight handling and response headers | `cors` |
| 18 | `setup_helmet` | Security headers (Helmet) | `security-headers` |
| 19 | `setup_logging` | Request / response logging | — |
| 20 | `setup_metrics` | Prometheus metrics layer and the `/metrics` endpoint | `metrics` |
| 21 | `setup_readiness` | Readiness probe endpoint (benefits from timeout / rate limiting) | — |
| 22 | `setup_timeout` | Request timeout boundary | — |
| 23 | `setup_rate_limiting` | Per-IP rate limiting (rejects excess load early) | `rate-limiting` |
| 24 | `setup_request_id` | Generate / propagate the `x-request-id` header (early, for tracing) | — |
| 25 | `setup_liveness` | Liveness probe endpoint (always reachable, very early) | — |
| 26 | `setup_catch_panic` | Panic recovery — catches panics from every inner layer (outermost) | — |
| 27 | `setup_fallback_files` | Fallback static files (must be installed last) | — |
<!-- END GENERATED: middleware-order -->

### Test Organization

| Location | Type | Method | Speed |
|----------|------|--------|-------|
| `src/fluent/tests/` | Unit tests | `oneshot()` | Fast |
| `tests/` | Integration tests | Real TCP + Docker | Slow |

Integration tests in `tests/` require Docker (testcontainers for Keycloak/PostgreSQL) and real network connections.

### Authentication

All authentication methods produce a unified `AuthenticatedIdentity` (defined in `src/config/http/identity.rs`), available as an Axum extractor:

- **Basic Auth** (`basic-auth` feature) - HTTP Basic Auth and API Key authentication
- **OIDC** (`keycloak` feature) - Two modes:
  - **Bearer-only** (default) - Validates JWT tokens in `Authorization: Bearer` headers
  - **Authorization Code Flow** - Full login/callback/logout flow when `redirect_uri` is configured; uses PKCE, CSRF state, nonce validation; stores tokens in session with transparent refresh
- **Proxy OIDC** (no feature flag) - Identity from reverse proxy headers (e.g., oauth2-proxy)

OIDC and Basic Auth can coexist when OIDC auth code flow is enabled (`redirect_uri` set), allowing browser users (OIDC) and API clients (Basic Auth/API keys) in the same app. In bearer-only mode (no `redirect_uri`), they remain mutually exclusive since both compete for the `Authorization` header. When `auto_redirect_to_login = true`, unauthenticated browser requests are automatically redirected to the login route. Proxy OIDC passes through without setting identity when headers are absent (no 401).

### Key Patterns

- **Feature-gated code**: Use `#[cfg(feature = "...")]` for optional dependencies
- **Middleware configuration**: Use `[http.middleware]` with `include`/`exclude` arrays in TOML
- **Environment variables**: Reference in TOML as `{{ DATABASE_URL }}` - substituted at load time

## Configuration

Configuration loads from `config/{RUST_ENV}.toml`. Set `RUST_ENV=dev` for development.

Minimal config:
```toml
[http]
bind_port = 3000
max_payload_size_bytes = "1KiB"
```

## Running the Service

```bash
RUST_ENV=dev cargo run
```
