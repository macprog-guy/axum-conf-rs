# Observability Middleware

This guide covers logging, metrics, tracing, and request correlation for monitoring your service.

## Request Logging

All requests are logged with structured data.

### Log Formats

Configure the log format:

```toml
[logging]
format = "json"  # json, compact, pretty, or default
```

### Format Examples

**JSON (Production)**
```json
{"timestamp":"2024-01-15T10:30:00.123Z","level":"INFO","target":"axum_conf","message":"request","method":"POST","path":"/api/users","status":201,"duration_ms":45,"request_id":"0193ce12-3456-7890-abcd-ef1234567890"}
```

**Compact (Development)**
```
2024-01-15T10:30:00.123Z  INFO axum_conf: POST /api/users 201 45ms req_id=0193ce12-...
```

**Pretty (Development)**
```
  2024-01-15T10:30:00.123456Z
    INFO axum_conf
    at src/fluent/mod.rs:123
    in axum_conf::fluent::logging
    method: POST
    path: /api/users
    status: 201
    duration_ms: 45
    request_id: 0193ce12-3456-7890-abcd-ef1234567890
```

**Default**
```
2024-01-15T10:30:00.123456Z  INFO axum_conf: POST /api/users -> 201 (45ms)
```

### Log Levels with RUST_LOG

Control log verbosity:

```bash
# Show only errors
RUST_LOG=error cargo run

# Show info and above
RUST_LOG=info cargo run

# Show debug for your crate only
RUST_LOG=my_service=debug,axum_conf=info cargo run

# Show trace for specific module
RUST_LOG=my_service::handlers=trace cargo run

# Complex filter
RUST_LOG=warn,my_service=debug,axum_conf=info,sqlx=warn cargo run
```

### Log Levels

| Level | When to Use |
|-------|-------------|
| `error` | Things that should never happen |
| `warn` | Unexpected but handled situations |
| `info` | Important lifecycle events (default) |
| `debug` | Detailed operational information |
| `trace` | Very verbose, per-request details |

## Prometheus Metrics

Automatically exposes metrics at `/metrics`.

### Default Metrics

```bash
curl http://localhost:3000/metrics
```

Output:
```
# HELP axum_conf_http_requests_total Total number of HTTP requests
# TYPE axum_conf_http_requests_total counter
axum_conf_http_requests_total{method="GET",path="/api/users",status="200"} 1523

# HELP axum_conf_http_request_duration_seconds HTTP request duration
# TYPE axum_conf_http_request_duration_seconds histogram
axum_conf_http_request_duration_seconds_bucket{method="GET",path="/api/users",le="0.005"} 1200
axum_conf_http_request_duration_seconds_bucket{method="GET",path="/api/users",le="0.01"} 1450
axum_conf_http_request_duration_seconds_bucket{method="GET",path="/api/users",le="0.025"} 1510
axum_conf_http_request_duration_seconds_bucket{method="GET",path="/api/users",le="0.05"} 1520
axum_conf_http_request_duration_seconds_bucket{method="GET",path="/api/users",le="0.1"} 1523
axum_conf_http_request_duration_seconds_bucket{method="GET",path="/api/users",le="+Inf"} 1523
axum_conf_http_request_duration_seconds_sum{method="GET",path="/api/users"} 4.523
axum_conf_http_request_duration_seconds_count{method="GET",path="/api/users"} 1523

# HELP axum_conf_http_response_size_bytes HTTP response body size
# TYPE axum_conf_http_response_size_bytes histogram
axum_conf_http_response_size_bytes_bucket{method="GET",path="/api/users",le="100"} 50
axum_conf_http_response_size_bytes_bucket{method="GET",path="/api/users",le="1000"} 1400
axum_conf_http_response_size_bytes_bucket{method="GET",path="/api/users",le="+Inf"} 1523
```

### Configure Metrics Route

```toml
[http]
metrics_route = "/prometheus"  # Default: "/metrics"
with_metrics = true            # Default: true
```

### Disable Metrics

```toml
[http]
with_metrics = false
```

Or via middleware:

```toml
[http.middleware]
exclude = ["metrics"]
```

### Kubernetes Scraping

```yaml
apiVersion: v1
kind: Service
metadata:
  name: my-service
  annotations:
    prometheus.io/scrape: "true"
    prometheus.io/port: "8080"
    prometheus.io/path: "/metrics"
spec:
  ports:
  - port: 8080
    name: http
```

### Grafana Dashboard Example

```json
{
  "panels": [
    {
      "title": "Request Rate",
      "targets": [{
        "expr": "rate(axum_conf_http_requests_total[5m])"
      }]
    },
    {
      "title": "P99 Latency",
      "targets": [{
        "expr": "histogram_quantile(0.99, rate(axum_conf_http_request_duration_seconds_bucket[5m]))"
      }]
    },
    {
      "title": "Error Rate",
      "targets": [{
        "expr": "rate(axum_conf_http_requests_total{status=~\"5..\"}[5m])"
      }]
    }
  ]
}
```

## Request ID

Every request gets a unique UUIDv7 identifier for tracing.

### Generation

- If `x-request-id` header exists, that value is used
- Otherwise, a UUIDv7 is generated (time-ordered, unique)

### Access in Handlers

```rust
use axum::http::HeaderMap;

async fn handler(headers: HeaderMap) -> String {
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    format!("Request ID: {}", request_id)
}
```

### Response Header

The request ID is included in responses:

```bash
curl -I http://localhost:3000/

# x-request-id: 0193ce12-3456-7890-abcd-ef1234567890
```

### Correlation

Use request IDs to correlate:
- Logs across services
- Traces in distributed systems
- Support tickets to specific requests

## OpenTelemetry Tracing

For distributed tracing, see [OpenTelemetry](../features/opentelemetry.md).

### Quick Setup

```toml
[logging.opentelemetry]
endpoint = "http://localhost:4317"
service_name = "my-service"
```

### Trace Context

axum-conf automatically:
- Extracts W3C Trace Context from incoming requests
- Propagates trace context to downstream calls
- Links spans to the same trace

## Adding Custom Metrics

Add application-specific metrics:

```rust
use prometheus::{Counter, Histogram, register_counter, register_histogram};
use lazy_static::lazy_static;

lazy_static! {
    static ref ORDERS_CREATED: Counter = register_counter!(
        "orders_created_total",
        "Total number of orders created"
    ).unwrap();

    static ref ORDER_VALUE: Histogram = register_histogram!(
        "order_value_dollars",
        "Order value in dollars"
    ).unwrap();
}

async fn create_order(Json(order): Json<Order>) -> impl IntoResponse {
    ORDERS_CREATED.inc();
    ORDER_VALUE.observe(order.total as f64);

    // Create order...
}
```

## Structured Logging in Handlers

Add context to log messages:

```rust
use tracing::{info, warn, error, instrument};

#[instrument(skip(pool))]
async fn get_user(
    State(pool): State<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<User>> {
    info!(user_id = id, "Fetching user");

    match fetch_user(&pool, id).await {
        Ok(user) => {
            info!(user_id = id, user_name = %user.name, "User found");
            Ok(Json(user))
        }
        Err(e) => {
            warn!(user_id = id, error = %e, "User not found");
            Err(Error::NotFound)
        }
    }
}
```

Output:
```json
{"level":"INFO","message":"Fetching user","user_id":123,"request_id":"0193ce12-..."}
{"level":"INFO","message":"User found","user_id":123,"user_name":"Alice","request_id":"0193ce12-..."}
```

## Next Steps

- [OpenTelemetry](../features/opentelemetry.md) - Distributed tracing
- [Performance Middleware](performance.md) - Timeouts, compression
- [Troubleshooting](../troubleshooting.md) - Debugging tips
