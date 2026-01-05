# TLS Support (rustls)

The `rustls` feature enables TLS (Transport Layer Security) for secure connections using the pure-Rust `rustls` library.

## Enable the Feature

```toml
# Cargo.toml
[dependencies]
axum-conf = { version = "0.3", features = ["rustls"] }
```

## Automatic Enabling

The `rustls` feature is automatically enabled when you use:

| Feature | Why TLS is needed |
|---------|-------------------|
| `postgres` | Secure database connections via `sslmode=require` |

```toml
# This automatically enables rustls
axum-conf = { version = "0.3", features = ["postgres"] }
```

## How It Works

axum-conf uses `rustls` instead of OpenSSL for TLS:

- **Pure Rust**: No system dependencies or C libraries required
- **Memory safe**: Written entirely in Rust with no unsafe code in the core
- **Native certificates**: Loads system CA certificates via `rustls-native-certs`

### Certificate Loading

The `rustls-native-certs` crate automatically loads trusted CA certificates from your operating system:

| Platform | Certificate Source |
|----------|-------------------|
| Linux | `/etc/ssl/certs`, `/etc/pki/tls/certs` |
| macOS | System Keychain |
| Windows | Windows Certificate Store |

No manual certificate configuration is needed for most use cases.

## Database TLS

When using the `postgres` feature, TLS is available for secure database connections:

```toml
# config/prod.toml
[database]
url = "{{ DATABASE_URL }}"
```

### Connection String Options

Control TLS behavior through the connection URL:

```bash
# Require TLS (recommended for production)
DATABASE_URL="postgres://user:pass@db.example.com:5432/mydb?sslmode=require"

# Verify server certificate
DATABASE_URL="postgres://user:pass@db.example.com:5432/mydb?sslmode=verify-full"

# Disable TLS (development only)
DATABASE_URL="postgres://user:pass@localhost:5432/mydb?sslmode=disable"
```

### SSL Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| `disable` | No TLS | Local development |
| `prefer` | Use TLS if available | Flexible environments |
| `require` | Require TLS, don't verify certificate | Production (basic) |
| `verify-ca` | Require TLS, verify CA | Production (recommended) |
| `verify-full` | Require TLS, verify CA and hostname | Production (strict) |

## Cloud Database Examples

### AWS RDS

```bash
DATABASE_URL="postgres://user:pass@mydb.xxxxx.us-east-1.rds.amazonaws.com:5432/mydb?sslmode=require"
```

### Google Cloud SQL

```bash
DATABASE_URL="postgres://user:pass@/mydb?host=/cloudsql/project:region:instance&sslmode=disable"
# Note: Cloud SQL Proxy handles encryption, so sslmode=disable is safe
```

### Heroku

```bash
# Heroku provides DATABASE_URL with sslmode=require
DATABASE_URL="${DATABASE_URL}?sslmode=require"
```

## Troubleshooting

### Certificate Errors

If you see certificate verification errors:

1. **Check system certificates** are up to date
2. **Verify the hostname** matches the certificate
3. **For self-signed certificates**, use `sslmode=require` instead of `verify-full`

### Connection Timeouts

TLS handshakes add latency. If you experience timeouts:

```toml
[database]
url = "{{ DATABASE_URL }}"
max_idle_time = "10m"  # Keep connections alive longer
```

## Next Steps

- [PostgreSQL](postgres.md) - Database integration guide
- [Keycloak/OIDC](keycloak.md) - Authentication (also uses TLS)
