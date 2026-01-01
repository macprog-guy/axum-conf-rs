//! Middleware Configuration Example
//!
//! Demonstrates how to configure and customize middleware.
//!
//! Run with:
//! ```bash
//! RUST_ENV=dev cargo run --example with_middleware --features "cors,compression,rate-limiting"
//! ```
//!
//! Then test:
//! ```bash
//! # Test CORS headers
//! curl -v -H "Origin: https://example.com" http://localhost:3000/api/data
//!
//! # Test rate limiting (make many requests quickly)
//! for i in {1..20}; do curl -s http://localhost:3000/api/data; done
//!
//! # Test compression
//! curl -H "Accept-Encoding: gzip" http://localhost:3000/api/data --compressed
//!
//! # Check metrics
//! curl http://localhost:3000/metrics
//! ```

use axum::{routing::get, Json};
use axum_conf::{Config, FluentRouter, Result};
use serde::Serialize;

#[derive(Serialize)]
struct ApiResponse {
    message: String,
    timestamp: String,
}

async fn get_data() -> Json<ApiResponse> {
    Json(ApiResponse {
        message: "Hello from the API!".into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let config: Config = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1MiB"
request_timeout = "30s"

# Rate limiting: 10 requests per second per IP
max_requests_per_sec = 10

# CORS configuration
[http.cors]
allowed_origins = ["https://example.com", "http://localhost:3000"]
allowed_methods = ["GET", "POST", "PUT", "DELETE"]
allowed_headers = ["content-type", "authorization"]
allow_credentials = true
max_age = "1h"

# Middleware configuration
# Uncomment to exclude specific middleware:
# [http.middleware]
# exclude = ["rate-limiting"]

[logging]
format = "default"
"#
    .parse()?;

    config.setup_tracing();

    println!("Starting server with middleware on http://127.0.0.1:3000");
    println!("\nEnabled middleware:");
    println!("  - CORS (allows example.com)");
    println!("  - Rate limiting (10 req/sec)");
    println!("  - Compression (gzip, brotli)");
    println!("  - Security headers");
    println!("  - Request logging with correlation IDs");
    println!("  - Prometheus metrics at /metrics");
    println!("\nTry: curl -H 'Origin: https://example.com' http://localhost:3000/api/data");

    FluentRouter::without_state(config)?
        .route("/api/data", get(get_data))
        .setup_middleware()
        .await?
        .start()
        .await
}

// Note: This example uses chrono for timestamps
// In a real project, add chrono to dependencies or use std::time
mod chrono {
    pub struct Utc;
    impl Utc {
        pub fn now() -> DateTime {
            DateTime
        }
    }
    pub struct DateTime;
    impl DateTime {
        pub fn to_rfc3339(&self) -> String {
            use std::time::{SystemTime, UNIX_EPOCH};
            let duration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap();
            format!("{}", duration.as_secs())
        }
    }
}
