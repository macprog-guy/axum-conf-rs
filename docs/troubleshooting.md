# Troubleshooting

Common issues and their solutions.

## Configuration Issues

### "Could not find configuration file"

**Problem:**
```
Error: Could not load configuration from config/dev.toml
```

**Solution:**
```bash
# Check RUST_ENV is set
echo $RUST_ENV

# Set it
export RUST_ENV=dev

# Verify file exists
ls -la config/
```

### "Environment variable not found"

**Problem:**
```
Error: Environment variable DATABASE_URL not set
```

**Solution:**
```bash
# Set the variable
export DATABASE_URL="postgres://localhost/mydb"

# Or use .env file
echo 'DATABASE_URL=postgres://localhost/mydb' >> .env
source .env
```

### "Invalid configuration value"

**Problem:**
```
Error: invalid value: string "30seconds", expected a duration
```

**Solution:**
```toml
# Wrong
request_timeout = "30seconds"

# Correct (humantime format)
request_timeout = "30s"
```

## Database Issues

### "Connection refused"

**Problem:**
```
Error: Connection refused (os error 111)
```

**Solution:**
```bash
# Check database is running
pg_isready -h localhost -p 5432

# Check connection string
echo $DATABASE_URL

# Test connection
psql $DATABASE_URL -c "SELECT 1"
```

### "Pool exhausted"

**Problem:**
```
Error: pool timed out while waiting for an open connection
```

**Solution:**
```toml
# Increase pool size
[database]
max_pool_size = 20
max_idle_time = "5m"
```

```rust
// Ensure connections are returned
// Use .fetch_one() not .fetch()
// Avoid holding connections across await points
```

### "Readiness probe failing"

**Problem:**
```
kubectl describe pod: Readiness probe failed
```

**Solution:**
```bash
# Check database connectivity from pod
kubectl exec my-service-xxx -- curl localhost:8080/ready

# Check database DNS
kubectl exec my-service-xxx -- nslookup postgres

# Check database secret
kubectl get secret my-service-secrets -o yaml
```

## Authentication Issues

### "Token validation failed"

**Problem:**
```
Error: JWT validation failed: InvalidSignature
```

**Solution:**
```toml
# Verify issuer URL matches Keycloak exactly
[http.oidc]
issuer_url = "https://keycloak.example.com/realms/myrealm"  # No trailing slash
```

```bash
# Test token
curl -s https://keycloak.example.com/realms/myrealm/.well-known/openid-configuration
```

### "Audience validation failed"

**Problem:**
```
Error: JWT validation failed: InvalidAudience
```

**Solution:**
```toml
[http.oidc]
audiences = ["my-service", "account"]  # Include expected audiences
```

```bash
# Decode token and check aud claim
echo $TOKEN | cut -d. -f2 | base64 -d | jq .aud
```

### "CORS preflight failing"

**Problem:**
```
Access to fetch at 'https://api.example.com' has been blocked by CORS policy
```

**Solution:**
```toml
[http.cors]
allowed_origins = ["https://app.example.com"]  # Exact origin
allowed_methods = ["GET", "POST", "PUT", "DELETE", "OPTIONS"]
allowed_headers = ["content-type", "authorization"]
allow_credentials = true
```

## Metrics Issues

### "Prometheus metrics conflict in tests"

**Problem:**
```
Error: Duplicate metrics collector registration
```

**Solution:**
```toml
# In test configuration
[http]
with_metrics = false
```

```rust
#[tokio::test]
async fn my_test() {
    let config: Config = r#"
        [http]
        bind_port = 0
        max_payload_size_bytes = "1KiB"
        with_metrics = false
    "#.parse().unwrap();
}
```

### "Metrics endpoint returns 404"

**Problem:**
```bash
curl http://localhost:3000/metrics
# 404 Not Found
```

**Solution:**
```toml
[http]
with_metrics = true
metrics_route = "/metrics"  # Default
```

Check middleware isn't excluding metrics:
```toml
[http.middleware]
# Don't exclude metrics
exclude = ["rate-limiting"]  # Not "metrics"
```

## Rate Limiting Issues

### "Tests failing due to rate limiting"

**Problem:**
```
Test failed: received 429 Too Many Requests
```

**Solution:**
```toml
# In test configuration
[http]
max_requests_per_sec = 0  # Disable
```

Or exclude middleware:
```toml
[http.middleware]
exclude = ["rate-limiting"]
```

### "Legitimate requests getting 429"

**Problem:**
Production users hitting rate limits.

**Solution:**
```toml
[http]
max_requests_per_sec = 1000  # Increase limit
```

Check if all requests come from same IP (load balancer):
```bash
# Check X-Forwarded-For header handling
# Rate limiting uses client IP from connection
```

## Timeout Issues

### "Request timeout"

**Problem:**
```
Error: request timeout after 30s
```

**Solution:**
```toml
[http]
request_timeout = "60s"  # Increase timeout
```

Or optimize the slow operation:
```rust
// Add caching
// Use pagination
// Move to background job
```

### "Shutdown timeout exceeded"

**Problem:**
```
WARN: Forcing shutdown after timeout
```

**Solution:**
```toml
[http]
shutdown_timeout = "60s"  # Increase
```

```yaml
# Kubernetes
terminationGracePeriodSeconds: 70
```

## Compression Issues

### "Compression not working"

**Problem:**
Responses aren't compressed despite `Accept-Encoding`.

**Solution:**
```toml
[http]
support_compression = true
```

```bash
# Test with explicit header
curl -H "Accept-Encoding: gzip" -I http://localhost:3000/large-data
# Should see: Content-Encoding: gzip
```

### "Response already compressed"

Small responses may not be compressed (overhead exceeds benefit).

## Memory Issues

### "Out of memory"

**Problem:**
Pod killed due to OOM.

**Solution:**
```toml
# Reduce payload size
[http]
max_payload_size_bytes = "1MiB"

# Reduce pool size
[database]
max_pool_size = 5
```

```yaml
# Increase limits
resources:
  limits:
    memory: "1Gi"
```

### "Memory leak"

**Problem:**
Memory usage grows continuously.

**Diagnosis:**
```rust
// Check for:
// - Unbounded caches
// - Growing channels
// - Leaked connections
```

## Logging Issues

### "Logs not appearing"

**Problem:**
No log output.

**Solution:**
```bash
# Set log level
export RUST_LOG=info

# Or more verbose
export RUST_LOG=debug
```

```rust
// Ensure tracing is initialized
let config = Config::default();
config.setup_tracing();  // Call this first!
```

### "Logs too verbose"

**Problem:**
Too much noise in logs.

**Solution:**
```bash
export RUST_LOG=warn,my_service=info
```

### "Missing request context in logs"

**Problem:**
Logs don't show request ID.

**Solution:**
Use `tracing` macros within request handlers:
```rust
use tracing::info;

async fn handler() {
    info!("Processing");  // Request ID included automatically
}
```

## Startup Issues

### "Address already in use"

**Problem:**
```
Error: Address already in use (os error 98)
```

**Solution:**
```bash
# Find process using port
lsof -i :3000
# or
ss -tlnp | grep 3000

# Kill it
kill <PID>

# Or use different port
[http]
bind_port = 3001
```

### "Validation failed"

**Problem:**
```
Error: Configuration validation failed
```

**Solution:**
Check error message for specific field:
```
# Missing required field
Error: Database URL cannot be empty

# Solution:
export DATABASE_URL="postgres://localhost/mydb"
```

## Getting Help

If you're still stuck:

1. **Check logs** with `RUST_LOG=debug`
2. **Validate config** with `config.validate()?`
3. **Simplify** - remove features until it works
4. **Search issues** on GitHub
5. **Open an issue** with minimal reproduction

### Reporting Issues

Include:
- axum-conf version
- Rust version
- Minimal config that reproduces
- Full error message
- Steps to reproduce
