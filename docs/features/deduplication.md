# Request Deduplication

The `deduplication` feature prevents duplicate request processing by tracking request IDs and rejecting duplicates within a configurable time window.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["deduplication"] }
```

## Configuration

```toml
# config/dev.toml
[http.deduplication]
ttl = "60s"           # How long to track request IDs (default: 60s)
max_entries = 10000   # Maximum IDs to track (default: 10000)
```

## How It Works

The middleware uses the `x-request-id` header as an idempotency key:

1. **First request**: The request ID is tracked and the request is processed normally
2. **Duplicate request**: If a request with the same ID arrives within the TTL window, the middleware returns `409 Conflict` immediately
3. **Expired request**: After the TTL expires, the request ID is removed and new requests with that ID are processed normally

```
Request 1 (id: abc-123) ──▶ Processed ──▶ 200 OK
                              │
                              ▼
                    Tracked for 60s
                              │
Request 2 (id: abc-123) ──▶ Duplicate! ──▶ 409 Conflict
        (within TTL)          │
                              │
        (after TTL) ──────────┘
                              │
Request 3 (id: abc-123) ──▶ Processed ──▶ 200 OK
```

### Request ID Requirement

This middleware requires the `x-request-id` header to be present. Requests without this header pass through without deduplication.

The request ID is typically set by the `RequestId` middleware (enabled by default), which generates UUIDv7 IDs for incoming requests that don't already have one.

### Duplicate Response

When a duplicate is detected, the response includes:
- Status: `409 Conflict`
- Header: `x-duplicate-request: true`
- Body: `Duplicate request detected`

## Basic Usage

```rust
use axum::{Json, routing::post};
use axum_conf::{Config, FluentRouter, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct CreateOrder {
    product_id: i32,
    quantity: i32,
}

#[derive(Serialize)]
struct Order {
    id: i32,
    product_id: i32,
    quantity: i32,
}

async fn create_order(Json(input): Json<CreateOrder>) -> Json<Order> {
    // This handler is protected from duplicate submissions
    // If the same x-request-id arrives twice, only one creates an order
    Json(Order {
        id: 1,
        product_id: input.product_id,
        quantity: input.quantity,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/orders", post(create_order))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Use Cases

### Idempotent APIs

Ensure that retried requests don't create duplicate resources:

```bash
# Client sends request with idempotency key
curl -X POST http://localhost:3000/orders \
  -H "Content-Type: application/json" \
  -H "x-request-id: order-12345" \
  -d '{"product_id": 1, "quantity": 2}'
# Response: 201 Created

# Network timeout, client retries with same key
curl -X POST http://localhost:3000/orders \
  -H "Content-Type: application/json" \
  -H "x-request-id: order-12345" \
  -d '{"product_id": 1, "quantity": 2}'
# Response: 409 Conflict (duplicate rejected)
```

### Webhook Deduplication

Prevent processing the same webhook event multiple times:

```rust
async fn handle_webhook(
    headers: axum::http::HeaderMap,
    Json(event): Json<WebhookEvent>,
) -> impl axum::response::IntoResponse {
    // The webhook provider's event ID can be passed as x-request-id
    // Duplicate deliveries are automatically rejected
    process_event(event).await;
    axum::http::StatusCode::OK
}
```

### Form Submission Protection

Prevent accidental double-submissions from users clicking submit twice:

```javascript
// Client-side: generate unique ID per form submission
const requestId = crypto.randomUUID();
fetch('/api/submit', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'x-request-id': requestId
  },
  body: JSON.stringify(formData)
});
```

## Configuration Reference

| Option | Description | Default |
|--------|-------------|---------|
| `ttl` | Time to track request IDs | `60s` |
| `max_entries` | Maximum IDs to track | `10000` |

### TTL (Time-to-Live)

The TTL determines how long a request ID is remembered after a request completes. Use human-readable durations:

```toml
[http.deduplication]
ttl = "30s"     # 30 seconds
ttl = "5m"      # 5 minutes
ttl = "1h"      # 1 hour
```

Longer TTLs provide better protection against late duplicates but use more memory.

### Max Entries

When the cache reaches `max_entries`, older entries are evicted using LRU (Least Recently Used):

```toml
[http.deduplication]
max_entries = 50000  # Support high-traffic APIs
```

## Memory Considerations

Each tracked request ID consumes approximately:
- 36 bytes for UUIDv7 string
- 16 bytes for expiration timestamp
- ~50 bytes total per entry

For 10,000 entries: ~500 KB memory usage.

### Sizing Guidelines

| Traffic | max_entries | TTL | Memory |
|---------|-------------|-----|--------|
| Low (< 100 req/s) | 10,000 | 60s | ~500 KB |
| Medium (100-1000 req/s) | 50,000 | 60s | ~2.5 MB |
| High (> 1000 req/s) | 100,000 | 30s | ~5 MB |

## Disabling for Specific Routes

Use the middleware include/exclude configuration:

```toml
[http.middleware]
exclude = ["deduplication"]  # Disable globally

# Or include only on specific routes (future feature)
```

## Background Cleanup

A background task runs every 60 seconds to remove expired entries, preventing memory leaks from TTL-expired entries that haven't been accessed.

## Next Steps

- [Middleware Overview](../middleware/overview.md) - Understanding the middleware stack
- [Security](../middleware/security.md) - Rate limiting and other protections
- [Performance](../middleware/performance.md) - Compression and timeouts
