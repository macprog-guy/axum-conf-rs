# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-06-08

### Added
- Pluggable application readiness hook: `FluentRouter::with_readiness_check` registers an app-supplied closure so `/ready` reflects application state (e.g. load shedding when a bounded worker pool is saturated), not just database connectivity. The hook is *composed* with the built-in checks — the endpoint is ready iff the application check returns `Readiness::Ready` **and** the built-in database/circuit-breaker check passes. A `NotReady(msg)` result returns `503 Service Unavailable` with `msg` in the body. New public `Readiness` type; available regardless of the `postgres` feature. See the `readiness_check` example.
- Optional `Strict-Transport-Security` and `Content-Security-Policy` response headers via new `[http]` options `hsts_max_age`, `hsts_include_subdomains` (default `true`), and `content_security_policy`. Both are off by default.
- `Error::is_transient()` to let callers distinguish retryable failures (I/O, database, circuit-breaker call) from deterministic ones without downcasting.

### Security
- HTTP error responses no longer leak internal detail: errors of internal kinds (database, configuration, TLS, I/O, internal) now return a generic `"An internal error occurred"` body while the full detail is still logged server-side. Client-facing kinds (authentication, invalid input, circuit-breaker) are unchanged.
- Session cookies are now `Secure` and `SameSite=Strict` by default. New `[http]` options `session_secure_cookie` (default `true`) and `session_same_site` (default `strict`) make this configurable; a warning is logged if `session_secure_cookie = false` while not bound to loopback.
- Basic Auth now compares the username in constant time (alongside the password), closing a timing channel that allowed username enumeration.
- Client-supplied `x-request-id` headers are validated (charset and length) before being preserved; malformed values are replaced with a fresh UUID to prevent log injection / correlation poisoning.

### Changed
- **Breaking (defaults):** `bind_addr` now defaults to `"0.0.0.0"` (was `"127.0.0.1"`) so services are reachable by Kubernetes probes and other pods; a warning is logged when bound to loopback. `request_timeout` now defaults to `60s` (was unset) to bound hung handlers — set it empty in TOML to disable.
- Circuit breaker state machine rewritten around a single lock, fixing a race under concurrent load where the failure threshold and half-open call limit could be exceeded, and recovering from lock poisoning instead of cascading panics.

### Fixed
- OpenTelemetry traces are now exported correctly: the OTel layer is composed with the active tracing subscriber instead of a separate, disconnected registry, and buffered spans are flushed on graceful shutdown. When OpenTelemetry is enabled, `setup_opentelemetry()` initializes logging — do not also call `Config::setup_tracing()`.
- Static fallback directories are wired once (by `setup_middleware`) instead of also during `with_state`, removing redundant work and a `merge`-ordering footgun.
- Eliminated permanent per-call memory leaks (`Box::leak`) in the metrics and OpenAPI route setup.
- The initial JWKS fetch retries with bounded exponential backoff (3 attempts) so a transient network hiccup at startup no longer fails the whole service.

### Performance
- Bearer JWT validation no longer rebuilds the decoding key and validation parameters per request; they are cached per-`kid` when the JWKS is loaded/refreshed.
- The authenticated identity is stored as `Arc<AuthenticatedIdentity>` in request extensions, avoiding repeated deep clones across role checks and span recording.
- Request deduplication evicts the oldest entry in O(1) amortized time via an insertion-order index, replacing an O(n) scan over the tracker at capacity.

## [0.4.1] - 2026-03-25

### Security
- Resolved 5 `cargo audit` advisories and 1 unmaintained-dependency warning

## [0.4.0] - 2026-03-25

### Changed
- Replaced the `axum-keycloak-auth` dependency with an in-house Bearer JWT validator: OIDC bearer-only mode now validates `Authorization: Bearer` tokens directly (signature via JWKS, plus issuer/audience/expiry checks)

## [0.3.25] - 2026-03-12

### Added
- `WithRole`, `AnyRole`, and `AllRoles` extractors for role-based route gating, plus the `role!` and `roles!` helper macros

## [0.3.24] - 2026-03-12

### Added
- Configurable `roles` field on `AuthenticatedIdentity`

## [0.3.23] - 2026-03-12

### Fixed
- Pass configured audiences to the OIDC auth code flow ID token verifier

## [0.3.22] - 2026-03-04

### Added
- Browser login redirect: unauthenticated browser requests can be auto-redirected to the OIDC login route
- OIDC and Basic Auth can coexist when the OIDC auth code flow is enabled (browser users via OIDC, API clients via Basic Auth/API keys)

## [0.3.21] - 2026-03-04

### Added
- OIDC Authorization Code Flow with PKCE (SHA-256), CSRF state, and nonce validation, including auto-registered login, callback, and logout routes when `redirect_uri` is configured
- Session-based token storage with transparent access token refresh
- Proxy OIDC authentication — reads identity from reverse proxy headers (e.g., oauth2-proxy)
- `AuthenticatedIdentity` unified extractor for all authentication methods (Basic Auth, OIDC, Proxy OIDC); Basic Auth users and API keys carry unified identity fields
- `setup_tracing_with()` to install a custom tracing layer (e.g. OpenTelemetry, file appenders)
- Middleware stack expanded to 19 layers (added Proxy OIDC)

### Changed
- OIDC middleware uses passthrough mode (`PassthroughMode::Pass`) when auth code flow is enabled, allowing unauthenticated requests to fall through to session-based identity
- Narrowed default OIDC scopes to `["openid"]`
- Updated dependencies to latest versions

### Fixed
- `map_keycloak_to_identity` now handles both `PassthroughMode::Block` (bare `KeycloakToken`) and `PassthroughMode::Pass` (`KeycloakAuthStatus::Success`)

## [0.3.15] - 2026-01-20

### Added
- Trace-level logging throughout middleware setup

### Documentation
- Static file serving documentation

## [0.3.14] - 2026-01-15

### Added
- Generic application configuration: `Config<T>` carries an app-specific config section alongside the framework configuration

## [0.3.12] - 2026-01-05

### Documentation
- Expanded documentation for the `deduplication` and `rustls` features; expanded README

## [0.3.9] - 2026-01-03

### Fixed
- Static file directories are now automatically set up in `setup_middleware()` (calls `setup_public_files()`, `setup_protected_files()`, and `setup_fallback_files()`)

## [0.3.8] - 2026-01-03

### Fixed
- Graceful shutdown now returns immediately after emitting shutdown phases instead of waiting the full timeout regardless of connection state

## [0.3.7] - 2026-01-02

### Added
- Circuit breaker check to readiness endpoint (`/ready` now returns unhealthy when circuit breaker is open)

## [0.3.6] - 2026-01-02

### Added
- Graceful shutdown notification system

### Changed
- Split liveness and readiness endpoints into separate setup methods for optimal middleware positioning
- Middleware stack expanded from 17 to 18 layers

### Deprecated
- `setup_liveness_readiness()` method (use `setup_liveness()` and `setup_readiness()` instead)

## [0.3.5] - 2026-01-01

### Fixed
- Regression in doctests

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

[Unreleased]: https://github.com/emethot/axum-conf/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/emethot/axum-conf/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/emethot/axum-conf/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/emethot/axum-conf/compare/v0.3.25...v0.4.0
[0.3.25]: https://github.com/emethot/axum-conf/compare/v0.3.24...v0.3.25
[0.3.24]: https://github.com/emethot/axum-conf/compare/v0.3.23...v0.3.24
[0.3.23]: https://github.com/emethot/axum-conf/compare/v0.3.22...v0.3.23
[0.3.22]: https://github.com/emethot/axum-conf/compare/v0.3.21...v0.3.22
[0.3.21]: https://github.com/emethot/axum-conf/compare/v0.3.15...v0.3.21
[0.3.15]: https://github.com/emethot/axum-conf/compare/v0.3.14...v0.3.15
[0.3.14]: https://github.com/emethot/axum-conf/compare/v0.3.12...v0.3.14
[0.3.12]: https://github.com/emethot/axum-conf/compare/v0.3.9...v0.3.12
[0.3.9]: https://github.com/emethot/axum-conf/compare/v0.3.8...v0.3.9
[0.3.8]: https://github.com/emethot/axum-conf/compare/v0.3.7...v0.3.8
[0.3.7]: https://github.com/emethot/axum-conf/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/emethot/axum-conf/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/emethot/axum-conf/compare/v0.3.3...v0.3.5
[0.3.3]: https://github.com/emethot/axum-conf/compare/v0.3.0...v0.3.3
[0.3.0]: https://github.com/emethot/axum-conf/releases/tag/v0.3.0
