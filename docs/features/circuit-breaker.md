# Circuit Breaker

Provides fail-fast behavior when downstream dependencies (database, external APIs) are degraded, preventing cascading failures.

## Quick Start

Enable the feature:

```toml
[dependencies]
axum-conf = { version = "0.3", features = ["circuit-breaker"] }
```

Configure targets in your TOML:

```toml
[circuit_breaker.targets.database]
failure_threshold = 5
reset_timeout = "30s"

[circuit_breaker.targets.payment-api]
failure_threshold = 3
reset_timeout = "60s"
call_timeout = "10s"
```

## How It Works

Circuit breakers track failures per target (not per route):

| State | Behavior |
|-------|----------|
| **Closed** | Normal operation. Requests pass through, failures are counted. |
| **Open** | Requests rejected immediately with 503. No calls to downstream. |
| **Half-Open** | Limited probe requests test if service has recovered. |

### State Transitions

```
CLOSED ──(failures >= threshold)──► OPEN
   ▲                                  │
   │                                  │ (reset_timeout expires)
   │                                  ▼
   └──(successes >= threshold)── HALF-OPEN
```

## Configuration Reference

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `failure_threshold` | u32 | 5 | Consecutive failures to trip circuit |
| `success_threshold` | u32 | 3 | Successes in half-open to close circuit |
| `reset_timeout` | Duration | 30s | Time in open state before half-open |
| `half_open_max_calls` | u32 | 3 | Probe requests allowed in half-open |
| `call_timeout` | Duration | none | Per-call timeout (counted as failure) |

## Usage

### Database Calls

```rust
use axum::extract::State;
use axum_conf::{Config, FluentRouter, Result};
use axum_conf::circuit_breaker::GuardedPool;

#[derive(Clone)]
struct AppState {
    db: GuardedPool,
}

async fn get_users(State(state): State<AppState>) -> Result<String> {
    let users: Vec<User> = state.db.query(|pool| async move {
        sqlx::query_as!(User, "SELECT * FROM users")
            .fetch_all(&pool)
            .await
    }).await.map_err(|e| match e {
        CircuitBreakerError::CircuitOpen { target } => {
            axum_conf::Error::CircuitBreakerOpen(target)
        }
        CircuitBreakerError::CallFailed(e) => {
            axum_conf::Error::Database(e)
        }
        CircuitBreakerError::Timeout { .. } => {
            axum_conf::Error::Custom("Database timeout".to_string())
        }
    })?;

    Ok(format!("Found {} users", users.len()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    let router = FluentRouter::without_state(config.clone())?;

    let state = AppState {
        db: router.guarded_db_pool("database"),
    };

    FluentRouter::with_state(config, state)?
        .route("/users", axum::routing::get(get_users))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

### HTTP Calls

```rust
use axum_conf::circuit_breaker::{GuardedHttpClient, CircuitBreakerError};

async fn call_payment_api(client: &GuardedHttpClient) -> Result<String, MyError> {
    client.request("payment-api", async {
        reqwest::get("https://api.stripe.com/v1/charges")
            .await?
            .text()
            .await
    })
    .await
    .map_err(|e| match e {
        CircuitBreakerError::CircuitOpen { .. } => MyError::ServiceUnavailable,
        CircuitBreakerError::CallFailed(e) => MyError::PaymentFailed(e),
        CircuitBreakerError::Timeout { .. } => MyError::Timeout,
    })
}
```

### Checking Circuit State

```rust
use axum_conf::circuit_breaker::{CircuitBreakerRegistry, CircuitState};

fn check_health(registry: &CircuitBreakerRegistry) {
    if let Some(breaker) = registry.get("database") {
        match breaker.current_state() {
            CircuitState::Closed => println!("Database: healthy"),
            CircuitState::Open => println!("Database: failing"),
            CircuitState::HalfOpen => println!("Database: recovering"),
        }
    }
}
```

## Error Handling

| Error | HTTP Status | When |
|-------|-------------|------|
| `CircuitBreakerOpen` | 503 Service Unavailable | Circuit is open |
| `CircuitBreakerCallFailed` | 502 Bad Gateway | Downstream call failed |
| Timeout | Counted as failure | Call exceeded `call_timeout` |

## Best Practices

1. **One circuit per dependency** - Use the same target name for all calls to a service
2. **Tune thresholds** - Start conservative, adjust based on observed behavior
3. **Set call timeouts** - Prevent hung connections from blocking workers
4. **Monitor state** - Log or expose circuit state for observability
5. **Handle gracefully** - Return cached data or fallback when circuit is open

## Architecture

Circuit breakers are per-target, not per-route:

```
Route /users      ──┐
Route /orders     ──┼──▶ PostgreSQL ──▶ [Single Circuit Breaker]
Route /products   ──┘

Route /payments   ──────▶ Stripe API ──▶ [Single Circuit Breaker]
Route /emails     ──────▶ SendGrid  ──▶ [Single Circuit Breaker]
```

This reflects actual failure domains - if PostgreSQL is down, all database routes fail together.

## Production Monitoring

### Logging

The circuit breaker emits structured logs on state transitions:

```
WARN Circuit breaker opened          state=open
INFO Circuit breaker transitioned    state=half-open
INFO Circuit breaker closed          state=closed
```

Configure your logging to capture these:

```rust
use tracing_subscriber::EnvFilter;

tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::new("axum_conf::circuit_breaker=info"))
    .init();
```

### Exposing Circuit State via Health Endpoint

Create a custom health endpoint that reports circuit breaker states:

```rust
use axum::{Json, extract::State};
use axum_conf::circuit_breaker::{CircuitBreakerRegistry, CircuitState};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    circuits: HashMap<String, CircuitStatus>,
}

#[derive(Serialize)]
struct CircuitStatus {
    state: String,
    failure_count: u32,
    success_count: u32,
}

async fn health_check(
    State(registry): State<CircuitBreakerRegistry>,
) -> Json<HealthResponse> {
    let mut circuits = HashMap::new();

    for target in ["database", "payment-api", "email-service"] {
        if let Some(breaker) = registry.get(target) {
            circuits.insert(target.to_string(), CircuitStatus {
                state: breaker.current_state().to_string(),
                failure_count: breaker.failure_count(),
                success_count: breaker.success_count(),
            });
        }
    }

    let all_closed = circuits.values()
        .all(|c| c.state == "closed");

    Json(HealthResponse {
        status: if all_closed { "healthy" } else { "degraded" },
        circuits,
    })
}
```

Response example:

```json
{
  "status": "degraded",
  "circuits": {
    "database": {
      "state": "closed",
      "failure_count": 0,
      "success_count": 15
    },
    "payment-api": {
      "state": "open",
      "failure_count": 5,
      "success_count": 0
    }
  }
}
```

### Prometheus Metrics

Expose circuit breaker metrics for Prometheus scraping:

```rust
use prometheus::{register_gauge_vec, GaugeVec};
use axum_conf::circuit_breaker::{CircuitBreakerRegistry, CircuitState};
use once_cell::sync::Lazy;

static CIRCUIT_STATE: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "circuit_breaker_state",
        "Circuit breaker state (0=closed, 1=half-open, 2=open)",
        &["target"]
    ).unwrap()
});

static CIRCUIT_FAILURES: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "circuit_breaker_failures",
        "Current failure count",
        &["target"]
    ).unwrap()
});

fn update_circuit_metrics(registry: &CircuitBreakerRegistry) {
    for target in ["database", "payment-api"] {
        if let Some(breaker) = registry.get(target) {
            let state_value = match breaker.current_state() {
                CircuitState::Closed => 0.0,
                CircuitState::HalfOpen => 1.0,
                CircuitState::Open => 2.0,
            };

            CIRCUIT_STATE
                .with_label_values(&[target])
                .set(state_value);

            CIRCUIT_FAILURES
                .with_label_values(&[target])
                .set(breaker.failure_count() as f64);
        }
    }
}
```

### Grafana Dashboard

Example Grafana panel queries:

**Circuit State Timeline:**
```promql
circuit_breaker_state{target="database"}
```

**Open Circuit Alert:**
```promql
circuit_breaker_state == 2
```

**Failure Rate:**
```promql
rate(circuit_breaker_failures[5m])
```

### Alert Rules

Example Prometheus alerting rules:

```yaml
groups:
  - name: circuit-breaker
    rules:
      - alert: CircuitBreakerOpen
        expr: circuit_breaker_state == 2
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Circuit breaker {{ $labels.target }} is open"
          description: "The circuit breaker for {{ $labels.target }} has been open for over 1 minute"

      - alert: CircuitBreakerHalfOpen
        expr: circuit_breaker_state == 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Circuit breaker {{ $labels.target }} is half-open"
          description: "The circuit breaker for {{ $labels.target }} has been in recovery mode for over 5 minutes"

      - alert: CircuitBreakerHighFailures
        expr: circuit_breaker_failures > 3
        labels:
          severity: warning
        annotations:
          summary: "Circuit breaker {{ $labels.target }} approaching threshold"
          description: "{{ $labels.target }} has {{ $value }} failures, threshold is 5"
```

### Kubernetes Integration

Use circuit state in readiness probes:

```rust
async fn readiness_probe(
    State(registry): State<CircuitBreakerRegistry>,
) -> impl IntoResponse {
    // Check critical dependencies
    let db_healthy = registry.get("database")
        .is_none_or(|b| b.current_state() != CircuitState::Open);

    if db_healthy {
        (StatusCode::OK, "OK")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "Database circuit open")
    }
}
```

Kubernetes manifest:

```yaml
readinessProbe:
  httpGet:
    path: /ready
    port: 3000
  initialDelaySeconds: 5
  periodSeconds: 10
  failureThreshold: 3
```

### Tracing Integration

Add circuit breaker spans to distributed traces:

```rust
use tracing::{instrument, Span};
use axum_conf::circuit_breaker::{CircuitBreakerRegistry, guarded_call};

#[instrument(skip(registry))]
async fn call_payment_api(registry: &CircuitBreakerRegistry) -> Result<String, Error> {
    let breaker = registry.get_or_default("payment-api");

    Span::current().record("circuit.state", &breaker.current_state().to_string());

    guarded_call(&breaker, "payment-api", async {
        // API call
        Ok("success".to_string())
    })
    .await
    .map_err(|e| {
        Span::current().record("circuit.error", &e.to_string());
        e.into()
    })
}
```

### Dashboard Example

A minimal monitoring dashboard should show:

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| Circuit State | Current state per target | state = open for > 1m |
| Failure Count | Failures since last reset | > 80% of threshold |
| State Changes | Transitions over time | > 5 changes/hour |
| Recovery Time | Time from open to closed | > 5 minutes |

### Observability Checklist

- [ ] Structured logging enabled for circuit breaker module
- [ ] Health endpoint exposes circuit states
- [ ] Prometheus metrics registered
- [ ] Grafana dashboard configured
- [ ] Alert rules defined for open circuits
- [ ] Readiness probe considers circuit state
- [ ] Trace spans include circuit context
