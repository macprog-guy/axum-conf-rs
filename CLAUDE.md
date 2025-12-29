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
- `session` - Cookie-based session management
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

Middleware is added innermost-to-outermost. The **last layer added executes first** on incoming requests:

1. Liveness/Readiness (innermost - health endpoints)
2. OIDC Authentication
3. Request Deduplication
4. Concurrency Limit
5. Max Payload Size
6. Compression
7. Path Normalization
8. Sensitive Headers
9. Request ID (UUIDv7)
10. API Versioning
11. CORS
12. Security Headers (Helmet)
13. Logging
14. Metrics (Prometheus)
15. Timeout
16. Rate Limiting
17. Panic Catching (outermost)

### Test Organization

| Location | Type | Method | Speed |
|----------|------|--------|-------|
| `src/fluent/tests/` | Unit tests | `oneshot()` | Fast |
| `tests/` | Integration tests | Real TCP + Docker | Slow |

Integration tests in `tests/` require Docker (testcontainers for Keycloak/PostgreSQL) and real network connections.

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
