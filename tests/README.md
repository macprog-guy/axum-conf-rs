# Integration Tests

This directory contains **external integration tests** that require real network connections
and/or external services (via testcontainers). These tests are separate from the unit tests
in `src/fluent/tests/` by design.

## Test Organization

| Location | Type | Method | Speed |
|----------|------|--------|-------|
| `src/fluent/tests/` | Unit tests | `oneshot()` in-process | Fast (~ms) |
| `tests/` | Integration tests | Real TCP server + HTTP client | Slow (seconds) |

## Why These Tests Are Here

### `rate_limit_tests.rs`

Rate limiting middleware requires `ConnectInfo<SocketAddr>` to identify clients by IP address.
This information is only available when making real network requests through a TCP listener.
The `oneshot()` method used in unit tests doesn't provide socket information.

These tests:
- Start an actual HTTP server on a random port
- Make real HTTP requests using `reqwest`
- Verify rate limiting behavior with actual network traffic

### `oidc_tests.rs` + `keycloak.rs`

OIDC authentication requires a real identity provider to:
- Issue valid JWT tokens
- Provide JWKS endpoints for token validation
- Handle the full OAuth2/OIDC flow

These tests use [testcontainers](https://testcontainers.com/) to spin up:
- A real Keycloak instance for authentication
- A PostgreSQL database (when `postgres` feature is enabled)

## Running Tests

```bash
# Run all tests (unit + integration)
cargo test --all-features

# Run only unit tests (faster)
cargo test --all-features --lib

# Run only integration tests
cargo test --all-features --test '*'

# Run specific integration test
cargo test --all-features --test rate_limit_tests
cargo test --all-features --test oidc_tests
```

## Prerequisites for Integration Tests

- **Docker**: Required for testcontainers (Keycloak, PostgreSQL)
- **Network access**: Tests bind to localhost ports

## Adding New Integration Tests

Place tests in `tests/` when they:
1. Require real network connections (TCP listeners, HTTP clients)
2. Need external services (databases, auth providers, message queues)
3. Test behavior that can't be simulated with `oneshot()`
4. Are slow by nature and shouldn't run on every save

For fast, isolated tests that don't need external dependencies, add them to
`src/fluent/tests/` instead.
