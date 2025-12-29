# Security Middleware

This guide covers security-focused middleware: rate limiting, authentication, CORS, and security headers.

## Rate Limiting

Per-IP rate limiting protects against abuse and DDoS attacks.

### Configuration

```toml
[http]
max_requests_per_sec = 100  # Requests per second per IP
```

### Behavior

- Uses token bucket algorithm
- Tracks requests per client IP
- Returns `429 Too Many Requests` when exceeded
- Resets each second

### Test Rate Limiting

```bash
# Send many requests quickly
for i in {1..150}; do
  curl -s -o /dev/null -w "%{http_code}\n" http://localhost:3000/
done

# Output shows 200s, then 429s when limit exceeded
```

### Disable for Development

```toml
[http]
max_requests_per_sec = 0  # 0 = disabled
```

### Disable via Middleware Config

```toml
[http.middleware]
exclude = ["rate-limiting"]
```

## Authentication

See dedicated guides:
- [Keycloak/OIDC](../features/keycloak.md) - JWT authentication
- [Basic Auth](../features/basic-auth.md) - Username/password and API keys

## CORS (Cross-Origin Resource Sharing)

Controls which domains can make browser requests to your API.

### Permissive CORS (Development)

```toml
# No CORS config = allow all origins in development
[http]
bind_port = 3000
```

### Restrictive CORS (Production)

```toml
[http.cors]
allow_credentials = true
allowed_origins = [
    "https://app.example.com",
    "https://admin.example.com"
]
allowed_methods = [
    "GET",
    "POST",
    "PUT",
    "DELETE",
    "PATCH"
]
allowed_headers = [
    "content-type",
    "authorization",
    "x-request-id"
]
exposed_headers = [
    "x-request-id"
]
max_age = "1h"  # Preflight cache duration
```

### CORS Options

| Option | Description | Default |
|--------|-------------|---------|
| `allow_credentials` | Allow cookies/auth | `false` |
| `allowed_origins` | List of allowed origins | All origins |
| `allowed_methods` | Allowed HTTP methods | Common methods |
| `allowed_headers` | Headers client can send | Common headers |
| `exposed_headers` | Headers client can read | None |
| `max_age` | Preflight cache time | `0` |

### CORS Preflight

Browser sends OPTIONS request before actual request:

```bash
# Preflight request
curl -X OPTIONS \
  -H "Origin: https://app.example.com" \
  -H "Access-Control-Request-Method: POST" \
  -H "Access-Control-Request-Headers: content-type" \
  http://localhost:3000/api/data

# Response headers
# Access-Control-Allow-Origin: https://app.example.com
# Access-Control-Allow-Methods: POST
# Access-Control-Allow-Headers: content-type
# Access-Control-Max-Age: 3600
```

### Debugging CORS

```bash
# Test with browser-like request
curl -v -X POST \
  -H "Origin: https://app.example.com" \
  -H "Content-Type: application/json" \
  http://localhost:3000/api/data

# Check response headers for:
# Access-Control-Allow-Origin: https://app.example.com
```

## Security Headers (Helmet)

Automatically adds security-related HTTP headers.

### Configuration

```toml
[http]
x_content_type_nosniff = true  # X-Content-Type-Options: nosniff
x_frame_options = "DENY"       # X-Frame-Options header
```

### X-Frame-Options Values

| Value | Effect |
|-------|--------|
| `"DENY"` | Page cannot be framed (default) |
| `"SAMEORIGIN"` | Can be framed by same origin |
| `"https://example.com"` | Can be framed by specific URL |

### Response Headers Added

```
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
```

### Verify Headers

```bash
curl -I http://localhost:3000/

# HTTP/1.1 200 OK
# x-content-type-options: nosniff
# x-frame-options: DENY
# ...
```

## Sensitive Headers

Prevents sensitive headers from appearing in logs.

### Default Behavior

The `Authorization` header is automatically marked as sensitive and redacted in logs:

```
2024-01-15 10:30:00 INFO request: method=POST path=/api headers={authorization: [REDACTED]}
```

### Configuration

This is enabled by default and cannot be configured. The following headers are protected:
- `Authorization`

## Security Best Practices

### Production Checklist

```toml
[http]
# Rate limiting
max_requests_per_sec = 1000

# Security headers
x_content_type_nosniff = true
x_frame_options = "DENY"

# HTTPS redirect (in reverse proxy)

[http.cors]
# Explicit allowed origins
allowed_origins = ["https://app.example.com"]
allow_credentials = true

# Explicit allowed methods
allowed_methods = ["GET", "POST", "PUT", "DELETE"]

# Explicit allowed headers
allowed_headers = ["content-type", "authorization"]
```

### Common Security Issues

**Issue: CORS allows all origins in production**
```toml
# BAD: No CORS config = permissive
[http]
bind_port = 8080

# GOOD: Explicit origins
[http.cors]
allowed_origins = ["https://app.example.com"]
```

**Issue: Rate limiting disabled**
```toml
# BAD: No protection
max_requests_per_sec = 0

# GOOD: Reasonable limit
max_requests_per_sec = 1000
```

**Issue: Weak X-Frame-Options**
```toml
# QUESTIONABLE: Allow framing
x_frame_options = "SAMEORIGIN"

# BETTER: Deny framing unless needed
x_frame_options = "DENY"
```

## Next Steps

- [Authentication](../features/keycloak.md) - Add user authentication
- [Observability](observability.md) - Logging and metrics
- [Troubleshooting](../troubleshooting.md) - Common issues
