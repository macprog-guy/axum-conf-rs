# TOML Configuration Reference

This document covers all configuration options available in axum-conf.

## Minimal Configuration

The smallest valid configuration:

```toml
[http]
max_payload_size_bytes = "1KiB"
```

All other values have sensible defaults.

## Complete Configuration

Every available option with explanations:

```toml
# =============================================================================
# HTTP Server Configuration
# =============================================================================
[http]
# Network binding
bind_addr = "127.0.0.1"              # IP address to bind (default: "127.0.0.1")
bind_port = 3000                      # Port to listen on (default: 3000)

# Request limits
max_payload_size_bytes = "32KiB"      # Max request body size (required)
max_concurrent_requests = 4096        # Max simultaneous requests (default: 4096)
max_requests_per_sec = 100            # Rate limit per IP (default: 100, 0 = disabled)

# Timeouts
request_timeout = "30s"               # Request timeout (optional, humantime format)
shutdown_timeout = "30s"              # Graceful shutdown timeout (default: 30s)

# Features
support_compression = false           # Enable gzip/brotli/zstd (default: false)
trim_trailing_slash = true            # Normalize paths (default: true)
with_metrics = true                   # Enable /metrics endpoint (default: true)

# Health check routes
liveness_route = "/live"              # Liveness probe path (default: "/live")
readiness_route = "/ready"            # Readiness probe path (default: "/ready")
metrics_route = "/metrics"            # Prometheus metrics path (default: "/metrics")

# API versioning
default_api_version = 1               # Default API version (default: 1)

# Security headers
x_content_type_nosniff = true         # X-Content-Type-Options: nosniff (default: true)
x_frame_options = "DENY"              # X-Frame-Options (default: "DENY")
                                      # Options: "DENY", "SAMEORIGIN", or URL

# =============================================================================
# CORS Configuration
# =============================================================================
[http.cors]
allow_credentials = false             # Allow cookies/auth headers (default: false)
allowed_origins = [                   # Allowed origins (omit for permissive in dev)
    "https://app.example.com",
    "https://admin.example.com"
]
allowed_methods = [                   # Allowed HTTP methods
    "GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"
]
allowed_headers = [                   # Allowed request headers
    "content-type",
    "authorization",
    "x-request-id"
]
exposed_headers = [                   # Headers exposed to JavaScript
    "x-request-id"
]
max_age = "1h"                        # Preflight cache duration (humantime format)

# =============================================================================
# OIDC/Keycloak Configuration (requires 'keycloak' feature)
# =============================================================================
[http.oidc]
issuer_url = "https://keycloak.example.com"
realm = "myrealm"
client_id = "my-service"
client_secret = "{{ OIDC_CLIENT_SECRET }}"  # Use env var substitution
audiences = ["my-service", "account"]        # Expected JWT audiences

# Authorization Code Flow (optional — enabled by setting redirect_uri)
redirect_uri = "https://myapp.example.com/auth/callback"  # Enables login flow
scopes = ["openid", "profile", "email"]                   # OAuth2 scopes (default shown)
post_login_redirect = "/"                                  # After login (default: "/")
post_logout_redirect = "/"                                 # After logout (default: "/")
login_route = "/auth/login"                                # Login path (default shown)
callback_route = "/auth/callback"                          # Callback path (default shown)
logout_route = "/auth/logout"                              # Logout path (default shown)

# =============================================================================
# Proxy OIDC Configuration (no feature flag required)
# =============================================================================
# Reads identity from HTTP headers set by an authenticating reverse proxy
# (e.g., oauth2-proxy with Nginx auth_request). All headers have sensible
# defaults matching oauth2-proxy conventions.
[http.proxy_oidc]
user_header = "X-Auth-Request-User"                        # Required header (default shown)
email_header = "X-Auth-Request-Email"                      # Email header (default shown)
groups_header = "X-Auth-Request-Groups"                    # Comma-separated groups (default shown)
preferred_username_header = "X-Auth-Request-Preferred-Username"  # Display name (default shown)
access_token_header = "X-Auth-Request-Access-Token"        # Access token (default shown)

# =============================================================================
# Basic Auth Configuration (requires 'basic-auth' feature)
# =============================================================================
[http.basic_auth]
mode = "either"                       # "basic", "api_key", or "either"
api_key_header = "X-API-Key"          # Header for API key auth (default)

[[http.basic_auth.users]]             # Basic auth users
username = "admin"
password = "{{ ADMIN_PASSWORD }}"

[[http.basic_auth.users]]
username = "readonly"
password = "{{ READONLY_PASSWORD }}"

[[http.basic_auth.api_keys]]          # API key credentials
key = "{{ SERVICE_A_API_KEY }}"
name = "service-a"

[[http.basic_auth.api_keys]]
key = "{{ SERVICE_B_API_KEY }}"
name = "service-b"

# =============================================================================
# Request Deduplication
# =============================================================================
[http.deduplication]
ttl = "30s"                           # How long to remember request IDs
max_entries = 10000                   # Maximum cache size

# =============================================================================
# Static File Serving
# =============================================================================
# Public static files (no authentication required)
[[http.directories]]
directory = "./public"                # Local directory path
route = "/static"                     # URL path prefix
protected = false                     # Require authentication (default: false)
cache_max_age = 3600                  # Cache-Control max-age in seconds (optional)

# Protected files (authentication required)
[[http.directories]]
directory = "./private"
route = "/downloads"
protected = true                      # Requires auth middleware

# SPA fallback (only one allowed, cannot be protected)
[[http.directories]]
directory = "./dist"
fallback = true                       # Serve for unmatched routes

# =============================================================================
# Middleware Control
# =============================================================================
[http.middleware]
# Option 1: Exclude specific middleware (all others enabled)
exclude = [
    "rate-limiting",
    "compression"
]

# Option 2: Include only specific middleware (all others disabled)
# include = [
#     "logging",
#     "metrics",
#     "liveness",
#     "readiness"
# ]

# Available middleware names:
# - oidc
# - basic-auth
# - request-deduplication
# - rate-limiting
# - concurrency-limit
# - max-payload-size
# - compression
# - path-normalization
# - sensitive-headers
# - request-id
# - api-versioning
# - cors
# - security-headers
# - logging
# - metrics
# - liveness
# - readiness
# - timeout
# - catch-panic
# - session (requires 'session' feature)
# - proxy-oidc

# =============================================================================
# Database Configuration (requires 'postgres' feature)
# =============================================================================
[database]
url = "{{ DATABASE_URL }}"            # PostgreSQL connection URL
min_pool_size = 1                     # Minimum pool connections (default: 1)
max_pool_size = 10                    # Maximum pool connections (default: 2)
max_idle_time = "5m"                  # Connection idle timeout (optional)

# =============================================================================
# Logging Configuration
# =============================================================================
[logging]
format = "json"                       # Log format (default: "default")
                                      # Options: "json", "compact", "pretty", "default"

# OpenTelemetry Configuration (requires 'opentelemetry' feature)
[logging.opentelemetry]
endpoint = "http://localhost:4317"    # OTLP collector endpoint
service_name = "my-service"           # Service name in traces (optional)
```

## Production Configuration Example

A recommended production configuration:

```toml
[http]
bind_addr = "0.0.0.0"
bind_port = 8080
max_payload_size_bytes = "32KiB"
request_timeout = "30s"
shutdown_timeout = "30s"
max_requests_per_sec = 1000
max_concurrent_requests = 4096
support_compression = true

[http.cors]
allow_credentials = true
allowed_origins = ["https://app.example.com"]
allowed_methods = ["GET", "POST", "PUT", "DELETE"]
allowed_headers = ["content-type", "authorization"]

[database]
url = "{{ DATABASE_URL }}"
max_pool_size = 20
max_idle_time = "10m"

[logging]
format = "json"

[logging.opentelemetry]
endpoint = "{{ OTEL_ENDPOINT }}"
service_name = "my-service"
```

## Development Configuration Example

A recommended development configuration:

```toml
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "10MiB"
max_requests_per_sec = 0  # Disable rate limiting

[logging]
format = "pretty"
```

## Test Configuration Example

For unit and integration tests:

```toml
[http]
bind_port = 0                         # OS assigns random port
max_payload_size_bytes = "1MiB"
max_requests_per_sec = 0              # Disable rate limiting
with_metrics = false                  # Avoid Prometheus conflicts

[http.middleware]
exclude = ["rate-limiting"]

[logging]
format = "compact"
```

## Byte Size Format

The `max_payload_size_bytes` field accepts human-readable sizes:

| Format | Bytes |
|--------|-------|
| `"100"` | 100 |
| `"1KB"` or `"1KiB"` | 1,024 |
| `"1MB"` or `"1MiB"` | 1,048,576 |
| `"1GB"` or `"1GiB"` | 1,073,741,824 |

## Duration Format

Duration fields use humantime format:

| Format | Duration |
|--------|----------|
| `"30s"` | 30 seconds |
| `"5m"` | 5 minutes |
| `"1h"` | 1 hour |
| `"1h30m"` | 1 hour 30 minutes |
| `"500ms"` | 500 milliseconds |

## Next Steps

- [Environment Variables](environment-vars.md) - Using `{{ VAR }}` substitution
- [Configuration Overview](overview.md) - Choosing configuration approaches
