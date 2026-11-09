# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Optional database readiness check.** New `[http] readiness_check_database` (default `true`)
  and `FluentRouter::with_database_readiness(bool)` (overrides config). When `false`, `/ready`
  skips both `SELECT 1` and the `"database"` circuit-breaker check, so readiness reflects only the
  application check from `with_readiness_check` (or just "process is up" when none is set). The
  circuit breaker keeps protecting real queries.

### Security
- **OIDC login now rotates the session id (session fixation).** The auth-code callback calls
  `Session::cycle_id()` after the ID token validates and before any token is stored, so the
  pre-login session id (which an attacker may have planted) never becomes the authenticated one.
  Session data carries over and the old id is deleted from the store. If rotation fails the login
  is aborted. `axum_conf::Session` (`session` feature) is re-exported; apps with their own login
  paths should call `session.cycle_id()` on any privilege change.

## [0.8.3] - 2026-11-06

### Added
- **`Config::install_metrics()`** installs the Prometheus recorder up front and returns the metrics
  layer and `/metrics` handle (`PrometheusMetricLayer`, `PrometheusHandle`, both re-exported from
  the crate root). Returns `None` when `http.with_metrics` is `false`. Requires a Tokio runtime.

### Changed
- **MSRV raised from 1.92 to 1.94.** sqlx 0.9 (adopted in 0.8.0) requires rustc 1.94, so the
  declared `rust-version` had been understated since then and the CI MSRV job was failing. No
  other dependency needs more than 1.92.
- `setup_metrics()` now reuses the process-wide recorder (stored in a `OnceLock`) instead of
  installing a new one. Building several routers in one process, or calling `install_metrics()`
  before `setup_metrics()`, no longer panics or yields an empty `/metrics`. If a later call has
  different recorder settings (`metrics_buckets`, `metrics_global_labels`, `metrics_idle_timeout`,
  `metrics_upkeep_timeout`), a warning is logged and the first call's settings stay in effect.
- Updated dependencies (jsonwebtoken 11, utoipa 6, OpenTelemetry 0.33, base64 0.23, and others).

## [0.8.2] - 2026-09-02

### Added
- **Per-route authentication: secure `/metrics` (or any route group) with its own Basic Auth
  credential, independent of the app's main auth.** New `FluentRouter::secure_with_basic_auth()`
  builder method wraps a caller-supplied set of routes in their own independent Basic Auth
  `route_layer`, merged into the router — so a route group can require Basic Auth even while the
  rest of the app enforces OIDC bearer tokens exclusively, with no credential leakage in either
  direction (an OIDC token doesn't satisfy the secured group, and its credentials don't satisfy
  OIDC-protected routes). New `[http.metrics_basic_auth]` config field wires the built-in
  `/metrics` endpoint through it automatically; omitting it preserves today's default (`/metrics`
  registered plainly, unauthenticated). This coexists with bearer-only `[http.oidc]` without
  requiring auth code flow — `HttpConfig::validate()`'s existing basic-auth/OIDC mutual-exclusion
  check is unchanged and unaffected. See `docs/design/per-route-authentication.md` for the design
  rationale and `docs/features/basic-auth.md` for configuration and usage.

### Fixed
- **Stale "you cannot use both `basic-auth` and `keycloak` features together" claim** in
  `docs/features/basic-auth.md` — it contradicted `docs/features/keycloak.md`'s already-accurate
  auth-code-flow coexistence section and predated the per-route scoping mechanism above; replaced
  with a corrected summary of both coexistence modes.

## [0.8.0] - 2026-06-30

> ⚠️ **Dependency refresh with public-API impact.** This release upgrades several dependencies,
> some of which appear in axum-conf's public surface (e.g. `db_pool()` returns
> `sqlx_postgres::PgPool`, and `sqlx::Error` is re-exposed via `From`). Downstream code that uses
> these types must upgrade in lockstep. No axum-conf source API was renamed or removed; the break is
> purely the bumped transitive types. Hence the minor version bump under pre-1.0 SemVer.

### Changed
- **Dependencies updated to their latest releases**, with the full feature matrix re-verified
  (`build` / `clippy -D warnings` / 539 lib tests, all green) for `default`, `production`, and
  `--all-features`:
  - **`sqlx` / `sqlx-postgres` 0.8 → 0.9** *(breaking for consumers using the Postgres pool or
    `sqlx::Error`)*. No axum-conf code change required.
  - **`tower-http` 0.6 → 0.7** (middleware stack; no code change required).
  - **`opentelemetry` / `opentelemetry-otlp` / `opentelemetry_sdk` 0.31 → 0.32** and
    **`tracing-opentelemetry` 0.32 → 0.33** (OTLP trace pipeline; no code change required).
  - **`toml` 0.9 → 1.1** (config parsing; no code change required).
  - **`hmac` 0.12 → 0.13** and **`sha2` 0.10 → 0.11** (session-record HMAC tagging). The only source
    change in this release: `KeyInit` (which now provides `new_from_slice`) is imported explicitly in
    `session_store.rs`, per the digest 0.11 trait split.
  - Semver-compatible bumps picked up via `cargo update`: `axum` 0.8.9, `tokio` 1.52, `rustls`
    0.23.41, `uuid` 1.23, `zeroize` 1.9, `jsonwebtoken` 10.4, `utoipa` 5.5, `dashmap` 6.2,
    `axum-helmet` 1.0.2, `keycloak` 26.6 (dev), `testcontainers` 0.27.3 (dev), and others.

- **`reqwest-system-roots` intentionally held at 0.12.** `openidconnect` 4.0 → `oauth2` 5.0 still
  depend on reqwest 0.12, so 0.12 is the only version that feature-unifies with the actual OIDC/JWKS
  client. Bumping to 0.13 would attach native-root trust to an unrelated reqwest 0.13 build and
  silently strip system-CA trust from OIDC TLS (the behavior added in 0.7.0). Revisit when
  openidconnect/oauth2 adopt reqwest 0.13.

## [0.7.2] - 2026-06-14

> ⚠️ **Behavior change in a patch release.** This release changes the *default* OIDC issuer
> derivation. It is source-compatible (nothing fails to compile), but **Keycloak deployments
> that omitted `realm` will fail OIDC discovery at startup** until they set `realm` explicitly.
> See the migration note below. The failure is loud and fail-fast (startup error with an
> actionable hint), never a silent auth change.

### Changed
- **OIDC config is now provider-agnostic by default.** When `realm` is unset, `issuer_url` is
  used **verbatim** as the issuer (and discovery base), instead of the previous Keycloak-shaped
  default of `{issuer_url}/realms/my-app`. This removes a silent footgun: consumers pointing at
  a non-Keycloak provider (PingFederate, Entra ID, Auth0, a corporate gateway) no longer need to
  remember `realm = ""` to get a correct issuer.

  **Migration — Keycloak users must now set `realm` explicitly.** Add `realm = "your-realm"` to
  `[http.oidc]` to keep the `{issuer_url}/realms/{realm}` issuer shape. Consumers who already set
  `realm` are unaffected; non-Keycloak users can now simply omit it.

### Added
- **Actionable discovery-failure hint.** When OIDC discovery fails *and* a `realm` is configured,
  the error now appends `issuer resolved to \`{issuer}\`; if your provider is not Keycloak, unset
  \`realm\``. This is the exact failure non-Keycloak consumers hit during migration.

## [0.7.1] - 2026-06-13

### Added
- **Configurable Prometheus metrics recorder via `[http]` config** — all opt-in, with **no behavior
  change when unset** (`/metrics` output is byte-for-byte identical to 0.7.0 by default):
  - `[[http.metrics_buckets]]` (`metric`, optional `match` = `full`/`prefix`/`suffix`, `buckets`):
    per-metric histogram bucket overrides. Turns a metric recorded through the global `metrics`
    facade into a true bucketed **histogram** (`_bucket{le=…}`) instead of the default **summary**
    (`{quantile=…}`), so it can be aggregated across replicas. Metric names are the **raw** recorded
    names (facade metrics are not `axum_conf_`-prefixed).
  - `[http.metrics_global_labels]`: global constant labels added to every exported series.
  - `metrics_idle_timeout` / `metrics_upkeep_timeout`: idle-metric eviction and upkeep-loop interval.
  - Matching builder setters: `Config::with_metric_buckets`, `with_metric_buckets_matched`,
    `with_metrics_global_label`, `with_metrics_idle_timeout`, `with_metrics_upkeep_timeout`; new public
    types `MetricBucketsConfig` and `MetricMatch`.
  - New `custom_histogram` example.

## [0.7.0] - 2026-06-13

### Changed
- **Bearer-path JWKS is now resolved via OIDC discovery** (`{issuer}/.well-known/openid-configuration`
  → `jwks_uri`) instead of the hardcoded Keycloak template `{issuer}/realms/{realm}/protocol/openid-connect/certs`,
  making Bearer authentication provider-agnostic (PingFederate, Entra ID, Auth0, …). For Keycloak
  deployments behavior is identical (Keycloak's advertised `jwks_uri` equals the old templated URL);
  the only wire-level difference is one extra GET at startup, retried with the same transient-failure
  policy as the JWKS fetch. **Action may be required for downstream tests:** suites that stub only the
  `/protocol/openid-connect/certs` endpoint must either also stub `/.well-known/openid-configuration`
  or set the new `jwks_url` override to point directly at the stub.
- **The RP-initiated logout endpoint is now read from discovery** (typed `end_session_endpoint`
  field) instead of being templated from the Keycloak convention. When the provider omits it,
  the old Keycloak template is still used as a fallback if a `realm` is configured; otherwise
  logout simply clears the session and redirects to `post_logout_redirect`.

### Added
- Provider-agnostic OIDC issuer: setting `[http.oidc] realm = ""` now uses `issuer_url`
  verbatim as the OIDC issuer (no `/realms/{realm}` suffix). Omitting `realm` keeps the
  Keycloak behavior unchanged.
- New optional `[http.oidc] jwks_url` field: when set, the Bearer path fetches signing keys
  from this URL directly and skips OIDC discovery entirely (escape hatch for providers whose
  `/.well-known` endpoint is unreachable, or to avoid the startup discovery roundtrip).

### Fixed
- **The OIDC HTTP client (discovery + JWKS) now trusts the system CA store** — `SSL_CERT_FILE`,
  `SSL_CERT_DIR`, `/etc/ssl/certs/ca-certificates.crt` — in addition to the bundled webpki
  (Mozilla) roots. Corporate/private CAs baked into container images are now honored when the
  IdP serves a certificate signed by them. Additive only: everything that validated before
  still validates. (Implemented via reqwest's `rustls-tls-native-roots` on the shared
  reqwest 0.12 build, so it also applies to downstream binaries sharing that build.)

## [0.6.0] - 2026-06-11

### Breaking
- Replaced the crate's glob re-exports (`pub use config::*`, etc.) with an explicit, curated
  public surface, and enabled `#![deny(unreachable_pub)]` so a forgotten re-export is a compile
  error. As part of this, three types that were only ever exposed accidentally by the globs are
  no longer public: `RequestIdGenerator`, `DeduplicationLayer`, and `DeduplicationService`
  (all internal middleware plumbing). If you referenced them, switch to configuring the
  middleware via TOML/`HttpConfig`.
- Removed long-deprecated methods: `FluentRouter::build()` (a no-op since 0.2.2 — delete the call;
  `setup_middleware()` already installs every layer) and `FluentRouter::setup_liveness_readiness()`
  (call `setup_liveness()` and `setup_readiness()`, or just use `setup_middleware()`).

### Added
- `SharedIdentity` extractor: a cheaply-cloned (`Arc`-backed, `Deref`) handle to the
  authenticated identity for read-only handlers, avoiding the per-request deep clone that
  `AuthenticatedIdentity` extraction performs (~3.4× faster identity extraction in the bench).
- `resilience::retry_transient` + `RetryPolicy`: a reusable retry helper that re-runs an
  operation while it returns a transient error (`Error::is_transient`) using full-jitter
  exponential backoff. The JWKS fetch now uses it (adding jitter to de-correlate fleet-wide
  retries). `Config::with_production` lets programmatic configs set the production posture.
- `Sensitive::expose_secret()` returns a borrowed reference to the wrapped secret, the
  preferred accessor over the public field (which becomes private in a future release).

### Changed
- Role extractors (`WithRole`/`AnyRole`/`AllRoles`) now borrow the identity to check the role
  and only clone on success, so a `403` no longer pays a deep copy. Request deduplication's
  hot duplicate path is now a borrowed lookup (no allocation on a replay).
- Configuration builder methods (`Config`/`HttpConfig` `new`/`with_*`) are now `#[must_use]`,
  so accidentally discarding a builder result is a compile-time warning.
- The `/ready` probe now checks the database circuit breaker with a side-effect-free
  `CircuitBreakerState::is_open()` instead of `should_allow()`, so frequent probes no longer
  consume the breaker's half-open recovery budget.
- Database circuit-breaker calls (`GuardedPool::query`) only trip on *degradation*
  (connectivity / pool exhaustion). Normal outcomes — no rows, constraint violations, decode
  errors — no longer open the circuit (and count as a healthy response). New
  `guarded_call_with` exposes a custom failure classifier.
- `Error::into_response` logs 5xx at `error!` and 4xx at `debug!` (4xx detail can be
  attacker-supplied), and `Error::is_transient` now also treats an open circuit as transient.
- `CircuitBreakerError` gained `is_transient()` and a `From<CircuitBreakerError<E>>` bridge
  into the unified `Error` taxonomy.

### Fixed
- A circuit breaker no longer refreshes its open-window timestamp on late failures while
  already open, which could otherwise delay recovery indefinitely.
- Server-**generated** `x-request-id` values are now propagated onto responses, not just
  client-supplied ones. The `SetRequestId`/`PropagateRequestId` layers were applied in the
  wrong order for axum (Propagate ended up outer, capturing the id before it was generated),
  so responses to requests without an inbound request id carried no `x-request-id`. Clients can
  now correlate every response with its server-side request id.

### Security
- **Proxy OIDC headers are now fail-closed in production.** When `[http.proxy_oidc]` is
  enabled, the `X-Auth-Request-*` identity/role headers are only honored when the request
  comes from a trusted proxy — configure `trusted_proxies` (CIDR allow-list) and/or
  `shared_secret`/`shared_secret_header`. With neither configured, the headers are now
  **ignored in production** (`RUST_ENV` unset/`prod`/`production`/`release`) instead of
  trusted from any client; they remain honored (with a warning) outside production for dev
  convenience. **Action required:** production deployments relying on proxy headers must set
  a trust anchor. (Closes a header-spoofing identity/authorization bypass.)
- **External session stores are now integrity-protected.** Records persisted to Postgres or
  Redis are HMAC-SHA256 tagged with a new required `[http] session_signing_key` (≥32 bytes,
  stable across replicas) and rejected on load if the tag does not verify — preventing an
  attacker with store write access from forging session/identity claims. Selecting a
  `postgres`/`redis` session store without a `session_signing_key` now fails validation.
- **OIDC session identity is re-verified.** On the session path the stored ID token's
  signature is re-checked against the issuer JWKS (defense in depth), and a freshly-issued
  ID token is re-stored on token refresh so it stays valid.
- **OIDC Bearer audience validation fails closed in production.** If `[http.oidc]` has no
  `audiences`, Bearer tokens are now rejected in production (previously `aud` validation was
  silently disabled). Outside production it still warns and accepts. This applies to
  Bearer/API auth only — the browser auth-code login flow is unaffected (the ID token's
  audience is the client id, validated independently).
- Hardened the OIDC post-login open-redirect filter to reject protocol-relative (`//host`)
  and backslash (`/\host`) targets.
- `Cookie`, `Set-Cookie`, and the configured API-key header are now redacted from logs
  alongside `Authorization`.
- Full OIDC token claims (PII) are now logged only when `AXUM_CONF_LOG_TOKEN_CLAIMS` is set,
  not merely at `DEBUG` level.
- A malformed (deprecated) `X-Frame-Options: ALLOW-FROM` URL now falls back to `DENY` rather
  than dropping all security headers.
- Credential and API-key comparisons now use the audited `subtle` crate's constant-time
  equality, hardening the existing timing-safe comparison against compiler optimization.

### Fixed
- OIDC token refresh responses that omit `expires_in` no longer store an expiry of `0`
  (which forced a token refresh on every request).

### Changed
- Removed nine unused dependencies (`prometheus`, `axum-metrics`, `utoipa-axum`,
  `rustls-native-certs`, `http-body`, `http-body-util`, `humantime`, `rust_decimal`,
  `opentelemetry-semantic-conventions`), trimming compile time and supply-chain surface.
  The `metrics`, `rustls`, `openapi`, and `opentelemetry` features are unaffected.
- `docs.rs` now builds with all features and renders per-feature availability badges
  (`[package.metadata.docs.rs]` + `doc(cfg)`), so feature-gated APIs are visible online.

### Fixed
- The test suite no longer mutates the process-global `RUST_ENV`, removing a data race
  that could make unrelated tests flaky under parallel execution.

## [0.5.0] - 2026-06-08

### Added
- Pluggable application readiness hook: `FluentRouter::with_readiness_check` registers an app-supplied closure so `/ready` reflects application state (e.g. load shedding when a bounded worker pool is saturated), not just database connectivity. The hook is *composed* with the built-in checks — the endpoint is ready iff the application check returns `Readiness::Ready` **and** the built-in database/circuit-breaker check passes. A `NotReady(msg)` result returns `503 Service Unavailable` with `msg` in the body. New public `Readiness` type; available regardless of the `postgres` feature. See the `readiness_check` example.
- Configurable session store for multi-replica deployments. `[http.session_store]` selects the backend — `memory` (default), `postgres` (new `session-postgres` feature; reuses the configured `[database]` pool and runs a background expiry sweep), or `redis` (new `session-redis` feature). `FluentRouter::with_session_store` accepts any `tower_sessions::SessionStore` for custom backends. The in-memory store now logs a warning when used on a non-loopback bind, since sessions aren't shared across replicas.
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
