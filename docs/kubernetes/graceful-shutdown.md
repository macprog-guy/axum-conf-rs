# Graceful Shutdown

axum-conf handles shutdown signals properly, allowing in-flight requests to complete before the process exits.

## How It Works

```
    SIGTERM received
          │
          ▼
    ┌─────────────────────────────┐
    │  Stop accepting new         │
    │  connections                │
    └─────────────────────────────┘
          │
          ▼
    ┌─────────────────────────────┐
    │  Wait for in-flight         │
    │  requests to complete       │
    │  (up to shutdown_timeout)   │
    └─────────────────────────────┘
          │
          ▼
    ┌─────────────────────────────┐
    │  Force close remaining      │
    │  connections                │
    └─────────────────────────────┘
          │
          ▼
    ┌─────────────────────────────┐
    │  Process exits              │
    └─────────────────────────────┘
```

## Configuration

```toml
[http]
shutdown_timeout = "30s"  # Time to wait for in-flight requests
```

## Kubernetes Integration

### Pod Termination Sequence

When Kubernetes terminates a pod:

```
1. Pod receives SIGTERM
2. Pod removed from Service endpoints (no new traffic)
3. preStop hook runs (if configured)
4. terminationGracePeriodSeconds countdown starts
5. SIGKILL sent if still running
```

### Alignment with axum-conf

```yaml
spec:
  terminationGracePeriodSeconds: 35  # Must be > shutdown_timeout
  containers:
  - name: app
    lifecycle:
      preStop:
        exec:
          command: ["sleep", "5"]  # Wait for endpoint removal
```

Timeline:
```
0s   SIGTERM received
0-5s preStop hook runs (wait for endpoint propagation)
5-35s shutdown_timeout allows requests to complete
35s  SIGKILL if still running
```

### Why preStop Hook?

Kubernetes endpoint removal is asynchronous. A brief delay ensures:
- Load balancer updates
- kube-proxy updates
- Other pods stop sending traffic

## Complete Configuration

```toml
# config/prod.toml
[http]
shutdown_timeout = "30s"
```

```yaml
# kubernetes/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  template:
    spec:
      terminationGracePeriodSeconds: 40  # shutdown_timeout + buffer
      containers:
      - name: app
        image: my-service:latest
        lifecycle:
          preStop:
            exec:
              command: ["sleep", "5"]
```

## Signals Handled

| Signal | Action |
|--------|--------|
| SIGTERM | Graceful shutdown |
| SIGINT | Graceful shutdown (Ctrl+C) |
| SIGQUIT | Graceful shutdown |

## Database Connections

When shutting down with the `postgres` feature:

1. New database requests rejected
2. Active queries allowed to complete
3. Connection pool drained
4. Connections closed

Ensure your `shutdown_timeout` is longer than your longest query:

```toml
[http]
shutdown_timeout = "30s"

[database]
# Queries should complete within shutdown_timeout
```

## Logging During Shutdown

```
2024-01-15T10:30:00Z INFO  axum_conf: Received shutdown signal
2024-01-15T10:30:00Z INFO  axum_conf: Stopping new connections
2024-01-15T10:30:00Z INFO  axum_conf: Waiting for 5 in-flight requests
2024-01-15T10:30:02Z INFO  axum_conf: 3 requests remaining
2024-01-15T10:30:05Z INFO  axum_conf: All requests completed
2024-01-15T10:30:05Z INFO  axum_conf: Server shutdown complete
```

## Testing Graceful Shutdown

### Locally

```bash
# Start the server
RUST_ENV=dev cargo run &
SERVER_PID=$!

# Make a slow request
curl http://localhost:3000/slow-endpoint &
REQUEST_PID=$!

# Send SIGTERM
kill -TERM $SERVER_PID

# Wait for request to complete
wait $REQUEST_PID
echo "Request completed during shutdown"

# Server should exit cleanly
wait $SERVER_PID
```

### In Kubernetes

```bash
# Watch pod during termination
kubectl delete pod my-service-xxx --wait=false
kubectl logs -f my-service-xxx

# Should see graceful shutdown logs
```

## Long-Running Requests

For requests that may exceed `shutdown_timeout`:

### Option 1: Increase Timeout

```toml
[http]
shutdown_timeout = "5m"
```

```yaml
terminationGracePeriodSeconds: 310
```

### Option 2: Use Background Jobs

Move long operations to async jobs:

```rust
async fn start_long_job(State(queue): State<JobQueue>) -> impl IntoResponse {
    // Queue the job
    queue.enqueue(LongRunningJob::new()).await;

    // Return immediately
    StatusCode::ACCEPTED
}
```

### Option 3: Client Retries

Design for client retries:

```rust
async fn idempotent_operation(
    Json(request): Json<IdempotentRequest>,
) -> impl IntoResponse {
    // Use request ID to ensure idempotency
    // Client can retry safely if connection drops
}
```

## Unhealthy During Shutdown

During shutdown, health endpoints continue working:

```
/live  → 200 (process still alive)
/ready → 503 (not accepting new work)
```

This helps Kubernetes:
- Not restart the pod (liveness OK)
- Remove from load balancer (readiness fail)

## Best Practices

1. **Set appropriate timeouts**
   - `shutdown_timeout` > longest expected request
   - `terminationGracePeriodSeconds` > `shutdown_timeout` + 10s

2. **Use preStop hooks**
   - 5-10 second delay for endpoint propagation
   - Ensures clean traffic drain

3. **Design for interruption**
   - Idempotent operations
   - Client retry logic
   - Background jobs for long work

4. **Monitor shutdown metrics**
   - Track forced terminations
   - Alert on slow shutdowns

## Next Steps

- [Health Checks](health-checks.md) - Configure probes
- [Deployment](deployment.md) - Complete manifests
