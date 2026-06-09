# Session Management

The `session` feature adds cookie-based session management for server-side state. This is automatically enabled when using the `keycloak` feature.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.5", features = ["session"] }
# or implicitly via keycloak
axum-conf = { version = "0.5", features = ["keycloak"] }
```

The `session` feature uses an in-memory store. For a shared store across
replicas, enable `session-postgres` or `session-redis` instead — see
[Session Storage](#session-storage) below.

## Basic Usage

```rust
use axum::{Json, routing::{get, post}};
use axum_conf::{Config, FluentRouter, Result};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

#[derive(Serialize, Deserialize, Default)]
struct Counter {
    count: u32,
}

async fn get_count(session: Session) -> Json<Counter> {
    let counter: Counter = session
        .get("counter")
        .await
        .unwrap()
        .unwrap_or_default();

    Json(counter)
}

async fn increment(session: Session) -> Json<Counter> {
    let mut counter: Counter = session
        .get("counter")
        .await
        .unwrap()
        .unwrap_or_default();

    counter.count += 1;

    session.insert("counter", &counter).await.unwrap();

    Json(counter)
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/count", get(get_count))
        .route("/increment", post(increment))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Test the session:

```bash
# First request - creates session
curl -c cookies.txt http://localhost:3000/count
# Output: {"count":0}

# Increment - uses same session
curl -b cookies.txt -X POST http://localhost:3000/increment
# Output: {"count":1}

curl -b cookies.txt -X POST http://localhost:3000/increment
# Output: {"count":2}

# Read count - session persists
curl -b cookies.txt http://localhost:3000/count
# Output: {"count":2}
```

## Session Data Types

Store any serializable data:

```rust
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

#[derive(Serialize, Deserialize)]
struct UserPreferences {
    theme: String,
    language: String,
    notifications_enabled: bool,
}

async fn save_preferences(
    session: Session,
    Json(prefs): Json<UserPreferences>,
) -> &'static str {
    session.insert("preferences", &prefs).await.unwrap();
    "Preferences saved"
}

async fn get_preferences(session: Session) -> Json<Option<UserPreferences>> {
    let prefs = session.get("preferences").await.unwrap();
    Json(prefs)
}
```

## Session Operations

### Insert Data

```rust
session.insert("key", &value).await.unwrap();
```

### Get Data

```rust
// Returns Option<T>
let value: Option<MyType> = session.get("key").await.unwrap();

// With default
let value: MyType = session
    .get("key")
    .await
    .unwrap()
    .unwrap_or_default();
```

### Remove Data

```rust
// Remove and return value
let removed: Option<MyType> = session.remove("key").await.unwrap();
```

### Clear Session

```rust
// Remove all data, keep session ID
session.clear().await;
```

### Delete Session

```rust
// Completely destroy session
session.delete().await.unwrap();
```

## Shopping Cart Example

```rust
use axum::{Json, extract::Path, routing::{get, post, delete}};
use axum_conf::{Config, FluentRouter, Result};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

#[derive(Serialize, Deserialize, Clone)]
struct CartItem {
    product_id: String,
    quantity: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct Cart {
    items: Vec<CartItem>,
}

async fn get_cart(session: Session) -> Json<Cart> {
    let cart: Cart = session
        .get("cart")
        .await
        .unwrap()
        .unwrap_or_default();

    Json(cart)
}

async fn add_to_cart(
    session: Session,
    Json(item): Json<CartItem>,
) -> Json<Cart> {
    let mut cart: Cart = session
        .get("cart")
        .await
        .unwrap()
        .unwrap_or_default();

    // Check if item exists
    if let Some(existing) = cart.items.iter_mut()
        .find(|i| i.product_id == item.product_id)
    {
        existing.quantity += item.quantity;
    } else {
        cart.items.push(item);
    }

    session.insert("cart", &cart).await.unwrap();
    Json(cart)
}

async fn remove_from_cart(
    session: Session,
    Path(product_id): Path<String>,
) -> Json<Cart> {
    let mut cart: Cart = session
        .get("cart")
        .await
        .unwrap()
        .unwrap_or_default();

    cart.items.retain(|i| i.product_id != product_id);

    session.insert("cart", &cart).await.unwrap();
    Json(cart)
}

async fn clear_cart(session: Session) -> &'static str {
    session.remove::<Cart>("cart").await.unwrap();
    "Cart cleared"
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();

    FluentRouter::without_state(config)?
        .route("/cart", get(get_cart).post(add_to_cart).delete(clear_cart))
        .route("/cart/:product_id", delete(remove_from_cart))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

Test the cart:

```bash
# Add item
curl -b cookies.txt -c cookies.txt -X POST \
  -H "Content-Type: application/json" \
  -d '{"product_id":"SKU001","quantity":2}' \
  http://localhost:3000/cart
# Output: {"items":[{"product_id":"SKU001","quantity":2}]}

# Add another
curl -b cookies.txt -X POST \
  -H "Content-Type: application/json" \
  -d '{"product_id":"SKU002","quantity":1}' \
  http://localhost:3000/cart
# Output: {"items":[{"product_id":"SKU001","quantity":2},{"product_id":"SKU002","quantity":1}]}

# Get cart
curl -b cookies.txt http://localhost:3000/cart

# Remove item
curl -b cookies.txt -X DELETE http://localhost:3000/cart/SKU001

# Clear cart
curl -b cookies.txt -X DELETE http://localhost:3000/cart
```

## With Authentication

Combine sessions with authentication for user-specific data:

```rust
use axum_conf::AuthenticatedIdentity;
use tower_sessions::Session;

async fn save_user_data(
    identity: AuthenticatedIdentity,
    session: Session,
    Json(data): Json<UserData>,
) {
    // Store with user ID as prefix for isolation
    let key = format!("user:{}:data", identity.user);
    session.insert(&key, &data).await.unwrap();
}
```

> **Note**: When the OIDC Authorization Code Flow is enabled, sessions are also used internally to store authentication tokens. The session-to-identity middleware automatically converts stored tokens into `AuthenticatedIdentity`, with transparent refresh when tokens expire.

## Session Storage

By default, sessions are stored **in-memory**. This is per-process: each replica
keeps its own sessions, so a request routed to a different pod won't see them.
The library logs a warning when the in-memory store is used while bound to a
non-loopback address, since that usually means a multi-replica deployment.

For production with multiple replicas, select a shared backend via
`[http.session_store]`:

```toml
[http.session_store]
type = "memory"                       # default — per-process, no shared state

# PostgreSQL — requires the `session-postgres` feature.
# Reuses the connection pool from [database]; creates a `tower_sessions` table
# on startup and runs a background sweep to purge expired rows.
# type = "postgres"

# Redis — requires the `session-redis` feature. Relies on Redis key expiry,
# so no background sweep is needed.
# type = "redis"
# url  = "redis://127.0.0.1:6379"
```

Enable the matching feature in `Cargo.toml`:

```toml
axum-conf = { version = "0.5", features = ["session-postgres"] }
# or
axum-conf = { version = "0.5", features = ["session-redis"] }
```

Both `session-postgres` and `session-redis` imply `session`. The Postgres store
additionally requires the `postgres` feature (pulled in automatically) and a
configured `[database]` section.

### Signing key (required for external stores)

External stores persist session records outside the process, so the records are
**HMAC-SHA256 tagged** with an operator-supplied key and rejected on load if the
tag doesn't verify. This stops an attacker with write access to the database or
cache from forging a session (e.g. tampering with the stored OIDC ID-token claims
to escalate roles). Selecting a `postgres` or `redis` store **without** a key
fails startup validation.

```toml
[http]
# Required for postgres/redis stores. Must be >= 32 bytes and STABLE across
# replicas and restarts (a per-process key would reject other replicas' sessions
# and invalidate everything on restart). Keep it secret; supports env substitution.
session_signing_key = "{{ SESSION_SIGNING_KEY }}"
```

The in-memory store needs no key — its records never leave the process.

### Custom Stores

For a backend the library doesn't ship (e.g. Moka, DynamoDB, or your own),
implement `tower_sessions::SessionStore` and install it with
`FluentRouter::with_session_store`. This bypasses `[http.session_store]` while
still honoring the `session_*` cookie settings:

```rust
let router = FluentRouter::without_state(config)?
    .with_session_store(my_store);
```

> **Security note:** Identity is rebuilt from the stored ID token's claims on
> each request *without* re-verifying the signature (it was verified at callback
> time). An external store (Postgres/Redis/custom) is therefore trusted for
> integrity — ensure the backend can't be tampered with.

### Cookie Attributes

Cookie attributes apply to every store and come from `[http]`:

```toml
[http]
session_secure_cookie = true          # Secure attribute (default: true)
session_same_site = "strict"          # "strict" | "lax" | "none" (default: "strict")
```

Sessions expire after 1 hour of inactivity.

### Alternatives

If you'd rather avoid server-side session state entirely:

1. **Sticky sessions** - Route the same user to the same pod (in-memory store)
2. **Stateless tokens** - Use JWT claims instead of sessions

## Security Considerations

1. **HTTPS only** - Session cookies should only be sent over HTTPS
2. **Secure cookie settings** - HttpOnly, Secure, SameSite
3. **Session fixation** - Regenerate session ID after authentication
4. **Session timeout** - Sessions expire to limit exposure

## Disabling Sessions

If you don't need sessions:

```toml
[http.middleware]
exclude = ["session"]
```

## Next Steps

- [Keycloak/OIDC](keycloak.md) - Full authentication
- [Security Middleware](../middleware/security.md) - Additional security
