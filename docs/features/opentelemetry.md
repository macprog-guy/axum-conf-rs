# OpenTelemetry Distributed Tracing

The `opentelemetry` feature adds distributed tracing with OTLP export to collectors like Jaeger, Tempo, or any OpenTelemetry-compatible backend.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["opentelemetry"] }
tracing = "0.1"
```

## Configuration

```toml
# config/prod.toml
[logging]
format = "json"

[logging.opentelemetry]
endpoint = "http://localhost:4317"
service_name = "my-service"
```

Or use an environment variable:

```toml
[logging.opentelemetry]
endpoint = "{{ OTEL_EXPORTER_OTLP_ENDPOINT }}"
service_name = "my-service"
```

## Basic Setup

```rust
use axum::routing::get;
use axum_conf::{Config, FluentRouter, Result};
use tracing::info;

async fn handler() -> &'static str {
    info!("Processing request");
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/", get(handler))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Adding Custom Spans

Create spans within your handlers for detailed tracing:

```rust
use axum::{Json, extract::State};
use axum_conf::Result;
use tracing::{info, instrument, Span};
use sqlx::PgPool;

#[instrument(skip(pool))]
async fn get_user(
    State(pool): State<PgPool>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<User>> {
    info!(user_id = id, "Fetching user");

    let user = fetch_from_db(&pool, id).await?;
    info!(user_name = %user.name, "User found");

    Ok(Json(user))
}

#[instrument(skip(pool))]
async fn fetch_from_db(pool: &PgPool, id: i32) -> Result<User> {
    // This creates a child span
    sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}
```

## Manual Span Creation

For more control over spans:

```rust
use tracing::{info_span, Instrument};

async fn complex_operation() {
    let span = info_span!("complex_operation", operation = "data_processing");

    async {
        // Work is traced within this span
        process_step_1().await;
        process_step_2().await;
    }
    .instrument(span)
    .await;
}
```

## Adding Span Attributes

Enrich spans with contextual data:

```rust
use tracing::Span;

async fn process_order(order_id: i32) {
    let span = Span::current();
    span.record("order_id", order_id);
    span.record("customer_type", "premium");

    // Processing logic...
}
```

## Trace Context Propagation

axum-conf automatically extracts and propagates W3C Trace Context headers (`traceparent`, `tracestate`).

### Incoming Request

When a request includes trace headers:

```bash
curl -H "traceparent: 00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01" \
     http://localhost:3000/api/users
```

The trace context is automatically extracted and used.

### Outgoing Requests

Propagate context to downstream services:

```rust
use reqwest::Client;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::Span;

async fn call_downstream(client: &Client, url: &str) -> Result<String> {
    let mut headers = reqwest::header::HeaderMap::new();

    // Inject current trace context into headers
    let propagator = TraceContextPropagator::new();
    let cx = Span::current().context();

    propagator.inject_context(&cx, &mut HeaderInjector(&mut headers));

    let response = client
        .get(url)
        .headers(headers)
        .send()
        .await?
        .text()
        .await?;

    Ok(response)
}

// Helper to inject into reqwest headers
struct HeaderInjector<'a>(&'a mut reqwest::header::HeaderMap);

impl<'a> opentelemetry::propagation::Injector for HeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&value) {
                self.0.insert(name, val);
            }
        }
    }
}
```

## Next Steps

- [Observability Middleware](../middleware/observability.md) - Logging and metrics
- [PostgreSQL](postgres.md) - Database integration with tracing
- [Kubernetes Deployment](../kubernetes/deployment.md) - Production setup
