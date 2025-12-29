# Health Checks

axum-conf provides Kubernetes-compatible health endpoints for liveness and readiness probes.

## Endpoints

| Endpoint | Purpose | Default Response |
|----------|---------|------------------|
| `/live` | Liveness probe | `200 OK` |
| `/ready` | Readiness probe | `200 OK` or `503` if unhealthy |

## Liveness Probe

**Purpose**: Tells Kubernetes the process is alive and not deadlocked.

### Behavior

- Always returns `200 OK` if the process is running
- No external dependencies checked
- If this fails, Kubernetes restarts the container

### Test Locally

```bash
curl http://localhost:3000/live
# Output: OK
```

### Kubernetes Configuration

```yaml
livenessProbe:
  httpGet:
    path: /live
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 10
  timeoutSeconds: 5
  failureThreshold: 3
```

## Readiness Probe

**Purpose**: Tells Kubernetes the service is ready to receive traffic.

### Behavior

- Returns `200 OK` if all dependencies are healthy
- With `postgres` feature: checks database connectivity
- If this fails, Kubernetes removes pod from Service endpoints

### Test Locally

```bash
# When healthy
curl http://localhost:3000/ready
# Output: OK

# When database is down
curl http://localhost:3000/ready
# HTTP/1.1 503 Service Unavailable
```

### Kubernetes Configuration

```yaml
readinessProbe:
  httpGet:
    path: /ready
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 5
  timeoutSeconds: 3
  failureThreshold: 3
  successThreshold: 1
```

## Health Check Flow

```
    KUBERNETES                          YOUR SERVICE
    ─────────────────────────────────────────────────────

    kubelet                             axum-conf
       │                                    │
       │──── GET /live ────────────────────▶│
       │◀─── 200 OK ────────────────────────│  Process alive
       │                                    │
       │──── GET /ready ───────────────────▶│
       │                                    │──── SELECT 1 ────▶ DB
       │                                    │◀─── OK ───────────
       │◀─── 200 OK ────────────────────────│  Ready for traffic
       │                                    │
       │                      [DB goes down]│
       │                                    │
       │──── GET /ready ───────────────────▶│
       │                                    │──── SELECT 1 ────▶ DB
       │                                    │◀─── TIMEOUT ──────
       │◀─── 503 Service Unavailable ───────│  Not ready
       │                                    │
    [K8s removes pod from Service endpoints]
```

## Custom Route Paths

```toml
[http]
liveness_route = "/health/live"
readiness_route = "/health/ready"
```

```yaml
livenessProbe:
  httpGet:
    path: /health/live
    port: 8080

readinessProbe:
  httpGet:
    path: /health/ready
    port: 8080
```

## Startup Probe

For slow-starting applications, use a startup probe:

```yaml
startupProbe:
  httpGet:
    path: /ready
    port: 8080
  initialDelaySeconds: 10
  periodSeconds: 10
  failureThreshold: 30  # 30 * 10s = 5 minutes to start
```

Once the startup probe succeeds, liveness and readiness probes take over.

## Best Practices

### What to Check

**Liveness** - Keep it simple:
- Process is running
- Main event loop is not blocked
- No external dependencies

**Readiness** - Check dependencies:
- Database connectivity
- Required external services
- Initialization complete

### What NOT to Check

**Liveness** - Avoid:
- Database connectivity (use readiness)
- Downstream services (can cause cascading failures)
- Expensive operations

**Readiness** - Avoid:
- Full query execution (simple ping is enough)
- Non-critical dependencies
- Operations that might timeout

### Probe Timing

| Setting | Liveness | Readiness |
|---------|----------|-----------|
| `initialDelaySeconds` | 5-10s | 5s |
| `periodSeconds` | 10-30s | 5-10s |
| `timeoutSeconds` | 5s | 3-5s |
| `failureThreshold` | 3 | 3 |
| `successThreshold` | 1 | 1 |

## Complete Example

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  replicas: 3
  selector:
    matchLabels:
      app: my-service
  template:
    metadata:
      labels:
        app: my-service
    spec:
      containers:
      - name: app
        image: my-service:latest
        ports:
        - containerPort: 8080
          name: http
        env:
        - name: RUST_ENV
          value: "prod"
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: my-service-secrets
              key: database-url
        livenessProbe:
          httpGet:
            path: /live
            port: http
          initialDelaySeconds: 5
          periodSeconds: 10
          timeoutSeconds: 5
          failureThreshold: 3
        readinessProbe:
          httpGet:
            path: /ready
            port: http
          initialDelaySeconds: 5
          periodSeconds: 5
          timeoutSeconds: 3
          failureThreshold: 3
        startupProbe:
          httpGet:
            path: /ready
            port: http
          initialDelaySeconds: 5
          periodSeconds: 5
          failureThreshold: 30
        resources:
          requests:
            memory: "128Mi"
            cpu: "100m"
          limits:
            memory: "512Mi"
            cpu: "1000m"
```

## Disabling Health Endpoints

If you need custom health logic:

```toml
[http.middleware]
exclude = ["liveness", "readiness"]
```

Then implement your own:

```rust
use axum::routing::get;

async fn custom_health() -> impl IntoResponse {
    // Custom health check logic
    if is_healthy() {
        (StatusCode::OK, "healthy")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unhealthy")
    }
}

FluentRouter::without_state(config)?
    .route("/health", get(custom_health))
    .setup_middleware()
    .await?
    .start()
    .await
```

## Troubleshooting

### Pod keeps restarting

Check liveness probe:
```bash
kubectl describe pod my-service-xxx
# Look for "Liveness probe failed"
```

Common causes:
- Probe timeout too short
- initialDelaySeconds too short for startup
- Application deadlock

### Pod not receiving traffic

Check readiness probe:
```bash
kubectl describe pod my-service-xxx
# Look for "Readiness probe failed"

kubectl get endpoints my-service
# Check if pod IP is listed
```

Common causes:
- Database connection issues
- Dependency not available
- Application not fully initialized

## Next Steps

- [Graceful Shutdown](graceful-shutdown.md) - Handle termination
- [Deployment](deployment.md) - Complete K8s manifests
