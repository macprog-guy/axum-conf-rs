# Session Management

The `session` feature adds cookie-based session management for server-side state. This is automatically enabled when using the `keycloak` feature.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.2", features = ["session"] }
# or implicitly via keycloak
axum-conf = { version = "0.2", features = ["keycloak"] }
```

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
use axum_conf::{KeycloakToken, Session};

async fn save_user_data(
    token: KeycloakToken,
    session: Session,
    Json(data): Json<UserData>,
) {
    // Store with user ID as prefix for isolation
    let key = format!("user:{}:data", token.subject());
    session.insert(&key, &data).await.unwrap();
}
```

## Session Storage

By default, sessions are stored in-memory. For production with multiple replicas, consider:

1. **Sticky sessions** - Route same user to same pod
2. **External session store** - Redis, PostgreSQL (requires custom setup)
3. **Stateless tokens** - Use JWT claims instead of sessions

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
