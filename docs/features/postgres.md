# PostgreSQL Integration

The `postgres` feature adds PostgreSQL database support with connection pooling via sqlx.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["postgres"] }
```

## Configuration

```toml
# config/dev.toml
[database]
url = "{{ DATABASE_URL }}"
min_pool_size = 1
max_pool_size = 10
max_idle_time = "5m"
```

Set the environment variable:

```bash
export DATABASE_URL="postgres://user:password@localhost:5432/mydb"
```

## Basic Usage

```rust
use axum::{Json, extract::State, routing::get};
use axum_conf::{Config, FluentRouter, Result};
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize)]
struct User {
    id: i32,
    name: String,
}

async fn get_users(State(pool): State<PgPool>) -> Result<Json<Vec<User>>> {
    let users = sqlx::query_as!(User, "SELECT id, name FROM users")
        .fetch_all(&pool)
        .await?;

    Ok(Json(users))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    // Create the connection pool
    let pool = config.create_pgpool()?;

    FluentRouter::with_state(config, pool)?
        .route("/users", get(get_users))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Using db_pool()

If you need the pool but also have other state:

```rust
use axum::{Json, extract::State, routing::get};
use axum_conf::{Config, FluentRouter, Result};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    cache_ttl: u64,
}

async fn get_data(State(state): State<AppState>) -> Result<Json<Vec<String>>> {
    let rows = sqlx::query_scalar!("SELECT name FROM items")
        .fetch_all(&state.db)
        .await?;

    Ok(Json(rows))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    let state = AppState {
        db: config.create_pgpool()?,
        cache_ttl: 3600,
    };

    FluentRouter::with_state(config, state)?
        .route("/data", get(get_data))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

## Transactions

```rust
use axum::{Json, extract::State};
use axum_conf::Result;
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
struct Transfer {
    from_account: i32,
    to_account: i32,
    amount: i64,
}

async fn transfer_funds(
    State(pool): State<PgPool>,
    Json(transfer): Json<Transfer>,
) -> Result<&'static str> {
    let mut tx = pool.begin().await?;

    // Debit from source account
    sqlx::query!(
        "UPDATE accounts SET balance = balance - $1 WHERE id = $2",
        transfer.amount,
        transfer.from_account
    )
    .execute(&mut *tx)
    .await?;

    // Credit to destination account
    sqlx::query!(
        "UPDATE accounts SET balance = balance + $1 WHERE id = $2",
        transfer.amount,
        transfer.to_account
    )
    .execute(&mut *tx)
    .await?;

    // Commit transaction
    tx.commit().await?;

    Ok("Transfer complete")
}
```

## Health Checks

When `postgres` is enabled, the readiness probe (`/ready`) automatically checks database connectivity:

```bash
# If database is available
curl http://localhost:3000/ready
# Output: OK

# If database is unavailable
curl http://localhost:3000/ready
# Output: Service Unavailable (503)
```

The health check runs `SELECT 1` to verify connectivity without loading data.

## Connection Pool Configuration

| Option | Description | Default |
|--------|-------------|---------|
| `url` | PostgreSQL connection URL | Required |
| `min_pool_size` | Minimum connections to maintain | 1 |
| `max_pool_size` | Maximum connections allowed | 2 |
| `max_idle_time` | Close idle connections after this duration | None |

### Connection URL Format

```
postgres://[user[:password]@][host][:port][/database][?param=value]
```

Examples:
```bash
# Local development
DATABASE_URL="postgres://localhost/mydb"

# With credentials
DATABASE_URL="postgres://myuser:mypass@localhost:5432/mydb"

# With SSL
DATABASE_URL="postgres://myuser:mypass@db.example.com:5432/mydb?sslmode=require"

# Docker Compose
DATABASE_URL="postgres://postgres:postgres@db:5432/app"
```

## Production Configuration

```toml
[database]
url = "{{ DATABASE_URL }}"
min_pool_size = 5
max_pool_size = 20
max_idle_time = "10m"
```

### Pool Sizing Guidelines

| Workload | min_pool_size | max_pool_size |
|----------|---------------|---------------|
| Light (< 100 req/s) | 1 | 5 |
| Medium (100-500 req/s) | 5 | 20 |
| Heavy (> 500 req/s) | 10 | 50 |

Rule of thumb: `max_pool_size` should be less than your PostgreSQL `max_connections` divided by the number of application instances.

## Testing

For tests, use a separate database:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum_conf::Config;

    #[tokio::test]
    async fn test_database_query() {
        let config: Config = r#"
            [http]
            bind_port = 0
            max_payload_size_bytes = "1KiB"

            [database]
            url = "postgres://localhost/mydb_test"
            max_pool_size = 2
        "#.parse().unwrap();

        let pool = config.create_pgpool().unwrap();

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .unwrap();

        // Test your queries
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(count.0, 0);
    }
}
```

## Next Steps

- [Keycloak/OIDC](keycloak.md) - Add authentication
- [OpenTelemetry](opentelemetry.md) - Add distributed tracing
- [Kubernetes Deployment](../kubernetes/deployment.md) - Production deployment
