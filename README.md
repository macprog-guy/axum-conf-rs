# axum-conf

[![codecov](https://codecov.io/gh/emethot/axum-conf/graph/badge.svg)](https://codecov.io/gh/emethot/axum-conf)
[![Crates.io](https://img.shields.io/crates/v/axum-conf.svg)](https://crates.io/crates/axum-conf)
[![Documentation](https://docs.rs/axum-conf/badge.svg)](https://docs.rs/axum-conf)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Production-ready web services with Axum — batteries included.**

Build Kubernetes-native Rust services without the boilerplate. axum-conf gives you health probes, metrics, security headers, rate limiting, and more — all configured through simple TOML.

```
                              axum-conf
    ┌─────────────────────────────────────────────────────────┐
    │                                                         │
    │  ┌─────────────┐   ┌──────────────┐   ┌─────────────┐   │
    │  │   Config    │──▶│ FluentRouter │──▶│  Middleware │   │
    │  │   (TOML)    │   │   Builder    │   │    Stack    │   │
    │  └─────────────┘   └──────────────┘   └─────────────┘   │
    │                           │                             │
    │                           ▼                             │
    │  ┌─────────────────────────────────────────────────┐    │
    │  │              Production-Ready Server            │    │
    │  │  • Health probes  • Metrics  • Security headers │    │
    │  │  • Rate limiting  • CORS     • Graceful shutdown│    │
    │  └─────────────────────────────────────────────────┘    │
    │                                                         │
    └─────────────────────────────────────────────────────────┘
```

## Why axum-conf?

- **Zero boilerplate**: Get liveness probes, Prometheus metrics, and security headers without writing middleware
- **Kubernetes-native**: Built for container deployments with proper health checks and graceful shutdown
- **Configuration-driven**: Change behavior through TOML files, not code changes

## Quick Start

**1. Add to Cargo.toml:**
```toml
[dependencies]
axum-conf = "0.3"
axum = "0.8"
tokio = { version = "1", features = ["full"] }
```

**2. Create `config/dev.toml`:**
```toml
[http]
bind_port = 3000
max_payload_size_bytes = "1MiB"
```

**3. Write `src/main.rs`:**
```rust
use axum::{Router, routing::get};
use axum_conf::{Config, Result, FluentRouter};

async fn hello() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::default();
    config.setup_tracing();

    FluentRouter::without_state(config)?
        .route("/", get(hello))
        .setup_middleware()
        .await?
        .start()
        .await
}
```

**4. Run:**
```bash
RUST_ENV=dev cargo run
```

**5. Test it:**
```bash
curl http://localhost:3000/        # Your handler
curl http://localhost:3000/live    # Liveness probe
curl http://localhost:3000/ready   # Readiness probe
curl http://localhost:3000/metrics # Prometheus metrics
```

## What You Get

| Feature | What it does | Default |
|---------|--------------|---------|
| **Health probes** | `/live` and `/ready` endpoints for Kubernetes | Enabled |
| **Prometheus metrics** | Request counts, latencies at `/metrics` | Enabled |
| **Request logging** | Structured logs with UUIDv7 correlation IDs | Enabled |
| **Rate limiting** | Per-IP request throttling | 100 req/sec |
| **Security headers** | X-Frame-Options, X-Content-Type-Options | Enabled |
| **Panic recovery** | Catches panics, returns 500, keeps running | Enabled |
| **Graceful shutdown** | Handles SIGTERM, drains connections | 30s timeout |
| **Compression** | gzip, brotli, deflate, zstd | Available |

## Cargo Features

Enable optional capabilities by category:

### Core Features

| Feature | What it adds |
|---------|--------------|
| `postgres` | PostgreSQL connection pooling with sqlx |
| `keycloak` | OIDC/JWT authentication via Keycloak |
| `basic-auth` | HTTP Basic Auth and API key authentication |
| `session` | Cookie-based session management |
| `opentelemetry` | Distributed tracing with OTLP export |
| `rustls` | TLS support (auto-enabled by `postgres`) |
| `circuit-breaker` | Per-target circuit breaker for external services |
| `openapi` | OpenAPI spec generation via utoipa |

### Middleware Features

| Feature | What it adds |
|---------|--------------|
| `metrics` | Prometheus metrics at `/metrics` |
| `rate-limiting` | Per-IP request throttling |
| `security-headers` | Security headers (X-Frame-Options, etc.) |
| `deduplication` | Request deduplication by request ID |
| `compression` | gzip/brotli/deflate/zstd compression |
| `cors` | CORS handling |
| `api-versioning` | API version extraction (path/header/query) |
| `concurrency-limit` | Max concurrent request limiting |
| `path-normalization` | Trailing slash normalization |
| `sensitive-headers` | Authorization header redaction in logs |
| `payload-limit` | Request body size limits |

### Feature Groups

| Group | Includes |
|-------|----------|
| `production` | metrics, rate-limiting, security-headers, compression, cors |
| `full` | All features |

```toml
# Example: Production setup with PostgreSQL
axum-conf = { version = "0.3", features = ["production", "postgres"] }
```

### Feature Compatibility

Most features work together, with one important exception:

| Feature | Compatible With | Notes |
|---------|----------------|-------|
| `keycloak` | All except `basic-auth` | Automatically enables `session` |
| `basic-auth` | All except `keycloak` | Cannot be used with OIDC |
| `postgres` | All features | Independent database layer |
| `session` | All features | Required by `keycloak` |
| `opentelemetry` | All features | Independent tracing layer |

**Important:** `keycloak` and `basic-auth` are mutually exclusive. Choose one authentication method per application.

## Examples

Run the examples to see axum-conf in action:

```bash
# Basic hello world
cargo run --example hello_world

# Application state management
cargo run --example with_state

# JSON REST API
cargo run --example json_api

# Middleware configuration (requires features)
cargo run --example with_middleware --features "cors,compression,rate-limiting"
```

## Configuration Example

```toml
# config/prod.toml
[http]
bind_addr = "0.0.0.0"
bind_port = 8080
max_payload_size_bytes = "32KiB"
request_timeout = "30s"
max_requests_per_sec = 1000

[http.cors]
allowed_origins = ["https://app.example.com"]
allowed_methods = ["GET", "POST", "PUT", "DELETE"]

[database]
url = "{{ DATABASE_URL }}"
max_pool_size = 10

[logging]
format = "json"
```

## Documentation

| Guide | Description |
|-------|-------------|
| [Getting Started](docs/getting-started.md) | Build your first service step-by-step |
| [Architecture](docs/architecture.md) | How axum-conf works under the hood |
| **Configuration** | |
| [Overview](docs/configuration/overview.md) | Configuration methods and philosophy |
| [TOML Reference](docs/configuration/toml-reference.md) | Complete configuration schema |
| [Environment Variables](docs/configuration/environment-vars.md) | Using `{{ VAR }}` substitution |
| **Features** | |
| [PostgreSQL](docs/features/postgres.md) | Database integration guide |
| [Keycloak/OIDC](docs/features/keycloak.md) | Authentication setup |
| [OpenTelemetry](docs/features/opentelemetry.md) | Distributed tracing |
| [Basic Auth](docs/features/basic-auth.md) | Simple authentication |
| [Sessions](docs/features/sessions.md) | Session management |
| [Circuit Breaker](docs/features/circuit-breaker.md) | External service resilience |
| [OpenAPI](docs/features/openapi.md) | API documentation generation |
| [Deduplication](docs/features/deduplication.md) | Request deduplication |
| [TLS/rustls](docs/features/rustls.md) | TLS configuration |
| **Middleware** | |
| [Overview](docs/middleware/overview.md) | Middleware stack architecture |
| [Features](docs/middleware/features.md) | API versioning, limits, normalization |
| [Security](docs/middleware/security.md) | Rate limiting, CORS, headers |
| [Observability](docs/middleware/observability.md) | Logging, metrics, tracing |
| [Performance](docs/middleware/performance.md) | Compression, timeouts, limits |
| **Kubernetes** | |
| [Health Checks](docs/kubernetes/health-checks.md) | Liveness and readiness probes |
| [Graceful Shutdown](docs/kubernetes/graceful-shutdown.md) | Proper pod termination |
| [Deployment](docs/kubernetes/deployment.md) | Complete K8s manifests |
| **Reference** | |
| [Troubleshooting](docs/troubleshooting.md) | Common issues and solutions |
| [API Docs](https://docs.rs/axum-conf) | Rustdoc API reference |

## Minimal Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  template:
    spec:
      containers:
      - name: app
        image: my-service:latest
        ports:
        - containerPort: 8080
        env:
        - name: RUST_ENV
          value: "prod"
        livenessProbe:
          httpGet:
            path: /live
            port: 8080
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
      terminationGracePeriodSeconds: 35
```

## License

MIT
