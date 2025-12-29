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
