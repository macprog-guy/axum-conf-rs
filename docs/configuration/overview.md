# Configuration Overview

axum-conf provides three ways to configure your service. Choose the approach that best fits your deployment model.

## Configuration Methods

### 1. TOML Files (Recommended)

Best for: **Most deployments**, especially Kubernetes

```rust
// Loads from config/{RUST_ENV}.toml
// e.g., RUST_ENV=prod loads config/prod.toml
let config = Config::default();
```

```bash
RUST_ENV=dev cargo run   # Loads config/dev.toml
RUST_ENV=prod cargo run  # Loads config/prod.toml
```

**Pros:**
- Environment-specific settings
- Easy to version control
- Supports environment variable substitution
- Clear separation of code and config

### 2. Inline TOML Strings

Best for: **Tests** and **embedded configuration**

```rust
let config: Config = r#"
    [http]
    bind_port = 3000
    max_payload_size_bytes = "1MiB"

    [logging]
    format = "json"
"#.parse()?;
```

**Pros:**
- Self-contained tests
- No external files needed
- Easy to customize per test

### 3. Builder Pattern

Best for: **Programmatic configuration** and **overrides**

```rust
let config = Config::default()
    .with_bind_port(8080)
    .with_compression(true)
    .with_request_timeout(Duration::from_secs(30))
    .with_log_format(LogFormat::Json);
```

**Pros:**
- Type-safe configuration
- IDE autocompletion
- Can combine with TOML (load file, then override)

## Decision Matrix

| Scenario | Recommended Approach |
|----------|---------------------|
| Production deployment | TOML files |
| Development | TOML files (config/dev.toml) |
| Unit tests | Inline TOML strings |
| Integration tests | Inline TOML with overrides |
| Docker/K8s | TOML files + env var substitution |
| Library embedding | Builder pattern |
| Quick prototyping | Builder pattern |

## Combining Approaches

You can load from TOML and then override with builder methods:

```rust
// Load base configuration
let config = Config::from_toml_file("prod")?
    // Override specific values
    .with_bind_port(9000)
    .with_max_concurrent_requests(2000);
```

Or use different configurations for different environments:

```rust
let config = if cfg!(test) {
    // Test configuration
    r#"
        [http]
        bind_port = 0
        max_requests_per_sec = 0
        with_metrics = false
    "#.parse()?
} else {
    // Production configuration
    Config::default()
};
```

## File Organization

Recommended project structure:

```
my-service/
├── Cargo.toml
├── src/
│   └── main.rs
└── config/
    ├── dev.toml      # Development settings
    ├── test.toml     # Test settings
    ├── staging.toml  # Staging settings
    └── prod.toml     # Production settings
```

## Configuration Sections

axum-conf configuration is organized into sections:

```toml
[http]           # Server binding, limits, middleware
[http.cors]      # Cross-origin resource sharing
[http.oidc]      # OpenID Connect / Keycloak
[http.middleware] # Middleware include/exclude
[database]       # PostgreSQL connection (requires postgres feature)
[logging]        # Log format and tracing
```

## Validation

Configuration is validated when:
1. Creating a `FluentRouter` - calls `config.validate()`
2. Explicitly - `config.validate()?`

Validation checks:
- Database URL format (if postgres feature enabled)
- OIDC configuration completeness (if keycloak feature enabled)
- Static directory constraints
- Bind address format
- Required fields are present

```rust
let config = Config::default();

// Validation happens automatically here
let router = FluentRouter::without_state(config)?;

// Or validate explicitly
config.validate()?;
```

## Next Steps

- [TOML Reference](toml-reference.md) - Complete configuration schema
- [Environment Variables](environment-vars.md) - Using `{{ VAR }}` substitution
