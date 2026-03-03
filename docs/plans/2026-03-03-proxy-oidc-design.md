# ProxyOidc Authentication & Unified AuthenticatedIdentity

**Date**: 2026-03-03
**Status**: Approved

## Summary

Add ProxyOidc authentication support for services behind an authenticating reverse proxy (e.g., oauth2-proxy with Nginx `auth_request`). The proxy performs OIDC authentication and passes identity via HTTP headers. Simultaneously, unify all auth methods under a single `AuthenticatedIdentity` type with an idiomatic Axum extractor.

## Design Decisions

1. **Mutually exclusive** with Basic Auth and OIDC (only one auth method at a time)
2. **No feature flag** required (zero external dependencies, always available)
3. **Pass-through** when proxy headers are absent (no identity set, no 401)
4. **Unified AuthenticatedIdentity** across all auth methods (Basic Auth, API Key, OIDC, ProxyOidc)
5. **OIDC unified** via a post-Keycloak mapper middleware
6. **Axum extractor** for `AuthenticatedIdentity` (401 when required, `Option<>` for optional)
7. **Access token** included as `Option<Sensitive<String>>`

## Unified AuthenticatedIdentity

```rust
#[derive(Debug, Clone)]
pub enum AuthMethod {
    BasicAuth,
    ApiKey,
    Oidc,
    ProxyOidc,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    pub method: AuthMethod,
    pub user: String,
    pub email: Option<String>,
    pub groups: Vec<String>,
    pub preferred_username: Option<String>,
    pub access_token: Option<Sensitive<String>>,
}
```

**Breaking change**: `name` renamed to `user`, `AuthMethod` gains `Oidc` and `ProxyOidc` variants.

### Axum Extractor

```rust
impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedIdentity {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<AuthenticatedIdentity>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "Authentication required"))
    }
}
```

- `handler(identity: AuthenticatedIdentity)` -> 401 if unauthenticated
- `handler(identity: Option<AuthenticatedIdentity>)` -> None if unauthenticated

## Configuration

### ProxyOidc (`[http.proxy_oidc]`)

```toml
[http.proxy_oidc]
user_header = "X-Auth-Request-User"                      # default
email_header = "X-Auth-Request-Email"                    # default
groups_header = "X-Auth-Request-Groups"                  # default
preferred_username_header = "X-Auth-Request-Preferred-Username"  # default
access_token_header = "X-Auth-Request-Access-Token"      # default
```

All fields optional with sensible defaults matching oauth2-proxy conventions.
Presence of `[http.proxy_oidc]` enables the middleware.

### Extended Basic Auth Config

```toml
[[http.basic_auth.users]]
username = "admin"
password = "{{ ADMIN_PASSWORD }}"
email = "admin@example.com"           # new, optional
groups = ["admin", "operators"]       # new, optional
preferred_username = "admin"          # new, optional

[[http.basic_auth.api_keys]]
key = "{{ API_KEY_1 }}"
name = "frontend-service"
email = "service@example.com"         # new, optional
groups = ["services"]                 # new, optional
preferred_username = "frontend"       # new, optional
```

### Mutual Exclusion Validation

In `HttpConfig::validate()`:
- `basic_auth` + `oidc` = error (existing)
- `basic_auth` + `proxy_oidc` = error (new)
- `oidc` + `proxy_oidc` = error (new)

### HttpMiddleware Enum

Add `ProxyOidc` variant (not feature-gated).

## Middleware Integration

### Stack Order

```
setup_protected_files()
setup_oidc()          // 1a. Keycloak JWT validation (route_layer)
setup_basic_auth()    // 1b. Basic/API Key auth (route_layer)
setup_proxy_oidc()    // 1c. ProxyOidc header extraction (route_layer) -- NEW
setup_public_files()
setup_user_span()     // 1d. Record user to span
```

### ProxyOidc Middleware (`src/fluent/proxy_oidc.rs`)

1. Read configured header names from config
2. If user header present: build `AuthenticatedIdentity` with all available fields
3. If user header absent: pass through (no identity, no error)
4. Insert identity into request extensions

### Post-OIDC Mapper (in `src/fluent/auth.rs`)

After KeycloakAuthLayer runs, a middleware reads `KeycloakToken` and maps to `AuthenticatedIdentity`:
- `user` <- `token.subject`
- `email` <- `token.extra.profile.email`
- `groups` <- `token.extra.roles`
- `preferred_username` <- `token.extra.profile.preferred_username`
- `method` <- `AuthMethod::Oidc`

KeycloakToken remains in extensions for handlers needing full JWT data.

### user_span Simplification

```rust
fn get_username_from_request(request: &Request<Body>) -> Option<String> {
    request.extensions().get::<AuthenticatedIdentity>()
        .map(|id| id.preferred_username.clone().unwrap_or_else(|| id.user.clone()))
}
```

No more feature-gated branches.

## Files Changed

| File | Change |
|------|--------|
| `src/config/http/proxy_oidc.rs` | New: ProxyOidc config struct + validation |
| `src/config/http/basic_auth.rs` | Extend BasicAuthUser, BasicAuthApiKey with email/groups/preferred_username |
| `src/config/http/middleware.rs` | Add `ProxyOidc` variant to HttpMiddleware |
| `src/config/http/mod.rs` | Add `proxy_oidc: Option<HttpProxyOidcConfig>` to HttpConfig, update validation |
| `src/fluent/proxy_oidc.rs` | New: ProxyOidc middleware |
| `src/fluent/basic_auth.rs` | Update to populate new AuthenticatedIdentity fields |
| `src/fluent/auth.rs` | Add setup_proxy_oidc(), post-OIDC mapper, update AuthenticatedIdentity + extractor |
| `src/fluent/user_span.rs` | Simplify to single AuthenticatedIdentity check |
| `src/fluent/builder.rs` | Add setup_proxy_oidc() to middleware stack |
| `src/fluent/mod.rs` | Export new module |
| `src/lib.rs` | Update exports |
| `Cargo.toml` | Add `proxy-oidc` to `full` feature group (no deps needed) |
| `tests/proxy_oidc_tests.rs` | New: integration tests |
| `tests/basic_auth_tests.rs` | Update for new AuthenticatedIdentity shape |

## Testing Strategy

### Unit Tests

- ProxyOidc: headers present/absent, partial headers, groups parsing, access token
- Basic Auth: extended config fields, backward compatibility
- Extractor: present -> extracted, missing -> 401
- OIDC mapper: full profile, empty preferred_username fallback

### Integration Tests

- ProxyOidc: full headers -> 200 + identity, no headers -> pass-through, health endpoints accessible
- Basic Auth: existing tests updated for new identity shape, new tests for extended fields
