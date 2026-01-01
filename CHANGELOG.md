# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3] - 2025-01-01

### Changed
- Improved crate description for crates.io
- Added keywords (`axum`, `postgres`, `configuration`, `tokio`) to Cargo.toml

## [0.3.0] - 2025-01-01

### Added

#### Core
- `FluentRouter` builder for constructing production-ready Axum applications
- `Config` struct with TOML file loading from `config/{RUST_ENV}.toml`
- Environment variable substitution in TOML using `{{ VAR_NAME }}` syntax
- Graceful shutdown with SIGTERM/SIGINT handling
- Human-readable duration and byte size parsing (e.g., `"30s"`, `"1MiB"`)

#### Middleware (17-layer stack)
- **Observability**: Request logging with UUIDv7 correlation IDs, Prometheus metrics (`/metrics` endpoint), OpenTelemetry tracing support
- **Security**: Rate limiting per IP, security headers (X-Frame-Options, X-Content-Type-Options, etc.), CORS configuration, sensitive header redaction
- **Performance**: Compression (gzip, brotli, deflate, zstd), request timeouts, concurrency limits, payload size limits
- **Reliability**: Panic catching, request deduplication by request ID
- **Routing**: Path normalization (trailing slashes), API versioning extraction

#### Health Checks
- `/live` endpoint for Kubernetes liveness probes
- `/ready` endpoint for Kubernetes readiness probes

#### Features (Cargo feature flags)
- `postgres` - PostgreSQL connection pooling via sqlx with rustls TLS
- `keycloak` - OIDC/JWT authentication via Keycloak
- `session` - Cookie-based session management with tower-sessions
- `opentelemetry` - Distributed tracing with OTLP export
- `basic-auth` - HTTP Basic Authentication and API key support
- `circuit-breaker` - Per-target circuit breaker pattern for external service resilience
- `openapi` - OpenAPI spec generation via utoipa

#### Convenience Feature Groups
- `full` - All features enabled
- `production` - Common production setup (metrics, rate-limiting, security-headers, compression, cors)

### Documentation
- Comprehensive getting started guide
- Architecture documentation with middleware ordering explanation
- Configuration reference with complete TOML schema
- Feature-specific guides for PostgreSQL, Keycloak, OpenTelemetry, sessions, basic-auth
- Kubernetes deployment guides with manifests
- Troubleshooting guide

[Unreleased]: https://github.com/emethot/axum-conf/compare/v0.3.3...HEAD
[0.3.3]: https://github.com/emethot/axum-conf/compare/v0.3.0...v0.3.3
[0.3.0]: https://github.com/emethot/axum-conf/releases/tag/v0.3.0
