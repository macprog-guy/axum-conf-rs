//! JSON API Example
//!
//! Demonstrates a simple REST API with JSON request/response handling.
//!
//! Run with:
//! ```bash
//! RUST_ENV=dev cargo run --example json_api
//! ```
//!
//! Then test:
//! ```bash
//! # List users
//! curl http://localhost:3000/users
//!
//! # Create a user
//! curl -X POST http://localhost:3000/users \
//!   -H "Content-Type: application/json" \
//!   -d '{"name": "Alice", "email": "alice@example.com"}'
//!
//! # Get a user
//! curl http://localhost:3000/users/1
//! ```

use axum::{
    extract::{Path, State},
    routing::get,
    Json,
};
use axum_conf::{Config, Error, FluentRouter, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

// Data models
#[derive(Clone, Serialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[derive(Deserialize)]
struct CreateUser {
    name: String,
    email: String,
}

// Application state with in-memory user storage
struct AppState {
    users: RwLock<Vec<User>>,
    next_id: RwLock<u64>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            users: RwLock::new(vec![]),
            next_id: RwLock::new(1),
        }
    }
}

// Handlers
async fn list_users(State(state): State<Arc<AppState>>) -> Json<Vec<User>> {
    let users = state.users.read().unwrap();
    Json(users.clone())
}

async fn get_user(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<u64>,
) -> Result<Json<User>> {
    let users = state.users.read().unwrap();
    users
        .iter()
        .find(|u| u.id == user_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| Error::invalid_input(format!("User {} not found", user_id)))
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateUser>,
) -> Result<Json<User>> {
    // Validate input
    if payload.name.trim().is_empty() {
        return Err(Error::invalid_input("name cannot be empty"));
    }
    if !payload.email.contains('@') {
        return Err(Error::invalid_input("invalid email format"));
    }

    // Generate ID
    let id = {
        let mut next_id = state.next_id.write().unwrap();
        let id = *next_id;
        *next_id += 1;
        id
    };

    // Create user
    let user = User {
        id,
        name: payload.name,
        email: payload.email,
    };

    // Store user
    state.users.write().unwrap().push(user.clone());

    Ok(Json(user))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config: Config = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "64KiB"
request_timeout = "30s"

[logging]
format = "default"
"#
    .parse()?;

    config.setup_tracing();

    let state = Arc::new(AppState::default());

    println!("Starting JSON API on http://127.0.0.1:3000");
    println!("Try: curl http://localhost:3000/users");

    FluentRouter::<Arc<AppState>>::with_state(config, state)?
        .route("/users", get(list_users).post(create_user))
        .route("/users/:id", get(get_user))
        .setup_middleware()
        .await?
        .start()
        .await
}
