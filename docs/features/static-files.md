# Static File Serving

axum-conf includes built-in static file serving for assets, SPAs, and protected downloads. Static files are served inside the middleware stack, benefiting from logging, metrics, compression, and security headers.

## Quick Start

No feature flag required - static file serving is always available.

```toml
# config/dev.toml

# Serve files from ./public at /static/*
[[http.directories]]
directory = "./public"
route = "/static"
```

```rust
use axum::routing::get;
use axum_conf::{Config, FluentRouter, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();

    FluentRouter::without_state(config)?
        .route("/api/hello", get(|| async { "Hello" }))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Files in `./public/` are now accessible at `/static/*`:
- `./public/app.js` → `http://localhost:3000/static/app.js`
- `./public/css/style.css` → `http://localhost:3000/static/css/style.css`

## Configuration Options

Each directory entry supports these options:

| Option | Type | Description |
|--------|------|-------------|
| `directory` | String | Local filesystem path (relative or absolute) |
| `route` | String | URL path prefix (mutually exclusive with `fallback`) |
| `fallback` | Boolean | Serve for unmatched routes (mutually exclusive with `route`) |
| `protected` | Boolean | Require authentication (default: false) |
| `cache_max_age` | Integer | Cache-Control max-age in seconds |

## Route-Based Serving

Serve files at a specific URL prefix:

```toml
[[http.directories]]
directory = "./public"
route = "/static"
```

Requests to `/static/file.txt` serve `./public/file.txt`.

### Multiple Directories

Configure multiple directories with different routes:

```toml
# JavaScript/CSS assets
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000  # 1 year

# Fonts
[[http.directories]]
directory = "./fonts"
route = "/fonts"
cache_max_age = 31536000

# Images
[[http.directories]]
directory = "./images"
route = "/img"
cache_max_age = 86400  # 1 day
```

### Index Files

Directories automatically serve `index.html` when requested:

```toml
[[http.directories]]
directory = "./public"
route = "/docs"
```

- `/docs/` → `./public/index.html`
- `/docs/guide/` → `./public/guide/index.html`

## SPA Fallback

For single-page applications with client-side routing, use fallback mode:

```toml
# Static assets with long cache
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000

# SPA fallback - serves index.html for all unmatched routes
[[http.directories]]
directory = "./dist"
fallback = true
```

When a user navigates to `/dashboard`:
1. No `/dashboard` route exists in your API
2. Fallback directory serves `dist/index.html`
3. Client-side router handles the route

**Restrictions:**
- Only one fallback directory is allowed
- Fallback directories cannot be protected

## Protected Directories

Serve files that require authentication (requires `keycloak` feature):

```toml
[dependencies]
axum-conf = { version = "0.3", features = ["keycloak"] }
```

```toml
# config/prod.toml
[http.oidc]
issuer_url = "https://keycloak.example.com/realms/myrealm"
client_id = "my-app"
client_secret = "{{ OIDC_CLIENT_SECRET }}"

[[http.directories]]
directory = "./downloads"
route = "/downloads"
protected = true
```

Only authenticated users can access files in `/downloads/*`.

## Caching

Add Cache-Control headers for better performance:

```toml
[[http.directories]]
directory = "./assets"
route = "/assets"
cache_max_age = 86400  # 1 day in seconds
```

This adds `Cache-Control: public, max-age=86400` to all responses.

### Caching Strategies

#### Immutable Assets (Hashed Filenames)

For files with content hashes (e.g., `app.a1b2c3.js`):

```toml
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000  # 1 year
```

Safe to cache forever - filename changes when content changes.

#### Versioned Assets

For files that may change between deployments:

```toml
[[http.directories]]
directory = "./dist"
route = "/static"
cache_max_age = 86400  # 1 day
```

#### No-Cache for HTML

HTML files should typically not be cached:

```toml
[[http.directories]]
directory = "./dist"
fallback = true
# No cache_max_age = no Cache-Control header
```

## Pre-Compressed Files

axum-conf automatically serves pre-compressed files when available:
- `.br` (Brotli) - best compression for web
- `.gz` (gzip) - widely supported fallback

### Creating Pre-Compressed Assets

Add to your build pipeline:

```bash
# Brotli compression
find ./dist -type f \( -name "*.js" -o -name "*.css" -o -name "*.html" -o -name "*.svg" \) \
    -exec brotli -f {} \;

# Gzip fallback
find ./dist -type f \( -name "*.js" -o -name "*.css" -o -name "*.html" -o -name "*.svg" \) \
    -exec gzip -kf {} \;
```

Result:
```
dist/
├── app.js
├── app.js.br      # Served when Accept-Encoding: br
├── app.js.gz      # Served when Accept-Encoding: gzip
├── styles.css
├── styles.css.br
└── styles.css.gz
```

### Pre-Compression vs Dynamic

| Approach | CPU Usage | Latency | Best For |
|----------|-----------|---------|----------|
| Pre-compressed | None | Lowest | Static assets |
| Dynamic (`compression` feature) | High | Higher | Changing content |

Use pre-compression for production static assets.

## Complete Production Example

```toml
# config/prod.toml

[http]
bind_addr = "0.0.0.0"
bind_port = 8080
max_payload_size_bytes = "1MiB"

# Hashed assets - cache forever
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000

# Fonts - cache forever
[[http.directories]]
directory = "./fonts"
route = "/fonts"
cache_max_age = 31536000

# Protected downloads - require auth
[[http.directories]]
directory = "./downloads"
route = "/downloads"
protected = true

# SPA fallback - no cache
[[http.directories]]
directory = "./dist"
fallback = true
```

## CDN Integration

When using a CDN (CloudFront, Cloudflare, etc.):

```toml
[[http.directories]]
directory = "./dist/assets"
route = "/assets"
cache_max_age = 31536000  # CDN caches for 1 year
```

Configure your CDN to:
1. **Cache based on `Accept-Encoding`** - Vary responses by encoding
2. **Pass through `Cache-Control`** - Honor max-age headers
3. **Strip cookies** - Static files don't need cookies

Example CloudFront behavior:
- **Path Pattern**: `/assets/*`
- **Cache Policy**: CachingOptimized
- **Origin Request Policy**: CORS-S3Origin (if needed)

## Testing Static Files

```bash
# Basic file request
curl http://localhost:3000/static/app.js

# Check headers
curl -I http://localhost:3000/assets/app.js
# HTTP/1.1 200 OK
# Content-Type: application/javascript
# Cache-Control: public, max-age=31536000

# Test pre-compressed (Brotli)
curl -H "Accept-Encoding: br" http://localhost:3000/assets/app.js

# Test pre-compressed (gzip)
curl -H "Accept-Encoding: gzip" http://localhost:3000/assets/app.js

# Test SPA fallback
curl http://localhost:3000/dashboard
# Returns index.html for client-side routing
```

## Troubleshooting

### Files Not Being Served

- Verify `directory` path is correct (relative to working directory)
- Ensure files have read permissions
- Check startup logs for errors
- Confirm the route doesn't conflict with API routes

### Compression Not Working

- Verify `.br`/`.gz` files exist alongside originals
- Check `Accept-Encoding` header is being sent by client
- For dynamic compression, ensure `compression` feature is enabled

### Cache Not Working

- Verify `cache_max_age` is set in config
- Check browser DevTools Network tab for `Cache-Control` header
- Clear browser cache when testing
- Check for proxy/CDN overriding headers

### Protected Files Return 401

- Ensure `keycloak` feature is enabled
- Verify OIDC configuration is correct
- Check authentication token is valid
- Fallback directories cannot be protected

## Performance Tips

1. **Pre-compress all text assets** - HTML, CSS, JS, SVG, JSON
2. **Use content hashes** - Enable aggressive caching with hashed filenames
3. **Set appropriate max-age** - Balance freshness vs. performance
4. **Don't compress images** - JPEG, PNG, WebP are already compressed
5. **Use a CDN** - Serve static files from edge locations

## How It Works

Static files are served using `tower-http`'s `ServeDir`:

```rust
// Internal implementation
ServeDir::new(&directory)
    .append_index_html_on_directories(true)
    .precompressed_br()
    .precompressed_gzip()
```

The middleware stack order ensures:
- **Protected files** are checked by OIDC before serving
- **Public files** bypass authentication
- **Fallback files** catch all unmatched routes (must be outermost)

## Next Steps

- [Middleware Overview](../middleware/overview.md) - Full middleware stack details
- [Keycloak/OIDC](keycloak.md) - Authentication for protected directories
- [Performance Middleware](../middleware/performance.md) - Compression settings
