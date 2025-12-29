# Performance Middleware

This guide covers compression, timeouts, payload limits, and concurrency control.

## Compression

Compress responses to reduce bandwidth.

### Enable Compression

```toml
[http]
support_compression = true
```

### Supported Algorithms

| Algorithm | Content-Encoding | Notes |
|-----------|------------------|-------|
| Brotli | `br` | Best compression ratio |
| Gzip | `gzip` | Most compatible |
| Deflate | `deflate` | Wide support |
| Zstandard | `zstd` | Fast compression |

### Client Request

```bash
# Request compressed response
curl -H "Accept-Encoding: gzip, br" http://localhost:3000/large-data

# Response headers show compression
# Content-Encoding: br
```

### When Compression Helps

- JSON responses > 1KB
- HTML pages
- Large text payloads

### When to Skip

- Already compressed (images, video)
- Small responses (< 1KB overhead exceeds benefit)
- Real-time/streaming responses

### Disable Compression

```toml
[http]
support_compression = false
```

Or via middleware:

```toml
[http.middleware]
exclude = ["compression"]
```

## Request Timeout

Limit how long a request can take.

### Configuration

```toml
[http]
request_timeout = "30s"  # Humantime format
```

### Behavior

- Timer starts when request begins
- Returns `408 Request Timeout` if exceeded
- Applies to entire request lifecycle

### Timeout Format

| Format | Duration |
|--------|----------|
| `"30s"` | 30 seconds |
| `"1m"` | 1 minute |
| `"5m30s"` | 5 minutes 30 seconds |
| `"500ms"` | 500 milliseconds |

### Error Response

```bash
# Slow endpoint
curl http://localhost:3000/slow-operation

# If exceeds timeout:
# HTTP/1.1 408 Request Timeout
# {"error":"timeout","message":"Request took too long"}
```

### Disable Timeout

Don't set `request_timeout` or exclude the middleware:

```toml
[http.middleware]
exclude = ["timeout"]
```

### Per-Route Timeout

For different timeouts per route, use manual middleware setup:

```rust
use std::time::Duration;
use tower::timeout::TimeoutLayer;

FluentRouter::without_state(config)?
    .route("/fast", get(fast_handler))
    .route("/slow", get(slow_handler))
    .route_layer(TimeoutLayer::new(Duration::from_secs(5)))  // 5s for /slow only
    // ... other middleware
```

## Payload Size Limit

Reject oversized request bodies.

### Configuration

```toml
[http]
max_payload_size_bytes = "32KiB"
```

### Size Format

| Format | Bytes |
|--------|-------|
| `"1KiB"` | 1,024 |
| `"32KiB"` | 32,768 |
| `"1MiB"` | 1,048,576 |
| `"10MiB"` | 10,485,760 |

### Error Response

```bash
# Oversized request
curl -X POST \
  -H "Content-Type: application/json" \
  -d "$(head -c 50000 /dev/urandom | base64)" \
  http://localhost:3000/api/data

# If exceeds limit:
# HTTP/1.1 413 Payload Too Large
# {"error":"payload_too_large","message":"Request body exceeds 32KiB limit"}
```

### Guidelines

| Use Case | Recommended Limit |
|----------|-------------------|
| JSON APIs | 32KiB - 1MiB |
| File uploads | 10MiB - 100MiB |
| Microservices | 1KiB - 32KiB |
| GraphQL | 100KiB - 1MiB |

## Concurrency Limit

Limit simultaneous requests to prevent overload.

### Configuration

```toml
[http]
max_concurrent_requests = 4096  # Default
```

### Behavior

- Tracks in-flight requests
- Returns `503 Service Unavailable` when limit reached
- Queues don't build up; fast fail

### Error Response

```bash
# When at capacity:
# HTTP/1.1 503 Service Unavailable
# Retry-After: 1
# {"error":"overloaded","message":"Server at capacity, try again"}
```

### Guidelines

| Scenario | Recommended Limit |
|----------|-------------------|
| CPU-bound work | 2-4 × CPU cores |
| IO-bound work | 100-1000 |
| Mixed workload | 256-4096 |
| Unknown | Start with 4096 |

## Graceful Shutdown

Handle in-flight requests during shutdown.

### Configuration

```toml
[http]
shutdown_timeout = "30s"  # Grace period for in-flight requests
```

### Shutdown Sequence

```
1. SIGTERM received
2. Stop accepting new connections
3. Wait for in-flight requests (up to shutdown_timeout)
4. Force close remaining connections
5. Exit
```

### Kubernetes Alignment

```yaml
spec:
  terminationGracePeriodSeconds: 35  # > shutdown_timeout
```

See [Graceful Shutdown](../kubernetes/graceful-shutdown.md) for details.

## Path Normalization

Removes trailing slashes for consistent routing.

### Configuration

```toml
[http]
trim_trailing_slash = true  # Default
```

### Effect

```
/api/users/ → /api/users
/api/users  → /api/users (unchanged)
```

### Disable

```toml
[http]
trim_trailing_slash = false
```

## Performance Tuning Checklist

### High Throughput

```toml
[http]
max_concurrent_requests = 8192
max_requests_per_sec = 10000
support_compression = true

[database]
max_pool_size = 50
```

### Low Latency

```toml
[http]
request_timeout = "5s"
max_payload_size_bytes = "10KiB"
support_compression = false  # Skip compression overhead

[database]
max_pool_size = 20
max_idle_time = "1m"
```

### Resource Constrained

```toml
[http]
max_concurrent_requests = 256
max_payload_size_bytes = "10KiB"
support_compression = false

[database]
max_pool_size = 5
```

## Monitoring Performance

### Key Metrics to Watch

From Prometheus:

```promql
# Request rate
rate(axum_conf_http_requests_total[5m])

# P99 latency
histogram_quantile(0.99, rate(axum_conf_http_request_duration_seconds_bucket[5m]))

# Error rate
rate(axum_conf_http_requests_total{status=~"5.."}[5m])

# Response size
histogram_quantile(0.99, rate(axum_conf_http_response_size_bytes_bucket[5m]))
```

### Alerting Rules

```yaml
groups:
- name: performance
  rules:
  - alert: HighLatency
    expr: histogram_quantile(0.99, rate(axum_conf_http_request_duration_seconds_bucket[5m])) > 1
    for: 5m
    annotations:
      summary: "P99 latency above 1 second"

  - alert: HighErrorRate
    expr: rate(axum_conf_http_requests_total{status=~"5.."}[5m]) / rate(axum_conf_http_requests_total[5m]) > 0.01
    for: 5m
    annotations:
      summary: "Error rate above 1%"

  - alert: TooManyConcurrentRequests
    expr: axum_conf_concurrent_requests > 3500
    for: 1m
    annotations:
      summary: "Approaching concurrency limit"
```

## Next Steps

- [Observability](observability.md) - Metrics and logging
- [Kubernetes Deployment](../kubernetes/deployment.md) - Production setup
- [Troubleshooting](../troubleshooting.md) - Common issues
