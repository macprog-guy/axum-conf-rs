# Shutdown Notifications

axum-conf provides a powerful shutdown notification system that allows your application components to react to graceful shutdown events. This enables coordinated cleanup, resource release, and proper termination of background tasks.

## Overview

The shutdown notification system offers two complementary mechanisms:

| Mechanism | Best For | Complexity |
|-----------|----------|------------|
| **CancellationToken** | Simple "stop work" signaling | Low |
| **ShutdownNotifier** | Phased cleanup with multiple stages | Medium |

## Quick Start

### Simple Cancellation (Most Common)

Use `cancellation_token()` when you just need to stop background tasks:

```rust
use axum_conf::{Config, FluentRouter};
use std::time::Duration;

#[tokio::main]
async fn main() -> axum_conf::Result<()> {
    let router = FluentRouter::without_state(Config::default())?;

    // Get a cancellation token before starting
    let token = router.cancellation_token();

    // Start a background task that respects shutdown
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    tracing::info!("Background task stopping");
                    break;
                }
                _ = interval.tick() => {
                    // Do periodic work
                    tracing::debug!("Running periodic task");
                }
            }
        }
    });

    // Start the server
    router.setup_middleware().await?.start().await
}
```

### Phased Shutdown (Complex Cleanup)

Use `subscribe_to_shutdown()` when you need to react to different shutdown stages:

```rust
use axum_conf::{Config, FluentRouter, ShutdownPhase};

#[tokio::main]
async fn main() -> axum_conf::Result<()> {
    let router = FluentRouter::without_state(Config::default())?;

    // Subscribe to shutdown phases
    let mut shutdown_rx = router.subscribe_to_shutdown();

    tokio::spawn(async move {
        while let Ok(phase) = shutdown_rx.recv().await {
            match phase {
                ShutdownPhase::Initiated => {
                    tracing::info!("Shutdown started - stopping new work");
                    // Stop accepting new jobs
                    // Mark health checks as unhealthy
                }
                ShutdownPhase::GracePeriodStarted { timeout } => {
                    tracing::info!(
                        "Grace period: {}s to complete work",
                        timeout.as_secs()
                    );
                    // Log remaining work
                    // Optionally cancel long-running operations
                }
                ShutdownPhase::GracePeriodEnded => {
                    tracing::warn!("Grace period ended - cleanup complete");
                    // Flush any remaining buffers
                    // Close external connections
                }
            }
        }
    });

    router.setup_middleware().await?.start().await
}
```

## Shutdown Phases

The shutdown sequence emits three phases in order:

```
┌─────────────────────────────────────────────────────────────────┐
│                        SIGTERM/SIGINT                           │
└─────────────────────────┬───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│ ShutdownPhase::Initiated                                        │
│ • Cancellation token triggered                                  │
│ • Server stops accepting new connections                        │
│ • Components should stop accepting new work                     │
└─────────────────────────┬───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│ ShutdownPhase::GracePeriodStarted { timeout }                   │
│ • In-flight requests being processed                            │
│ • Countdown begins (configured via shutdown_timeout)            │
│ • Components should prioritize completing critical work         │
└─────────────────────────┬───────────────────────────────────────┘
                          │ (after timeout)
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│ ShutdownPhase::GracePeriodEnded                                 │
│ • Timeout expired                                               │
│ • Final cleanup should be complete                              │
│ • Process termination imminent                                  │
└─────────────────────────────────────────────────────────────────┘
```

## API Reference

### FluentRouter Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `cancellation_token()` | `CancellationToken` | Token triggered on shutdown initiation |
| `shutdown_notifier()` | `&ShutdownNotifier` | Reference to the notifier for subscriptions |
| `subscribe_to_shutdown()` | `Receiver<ShutdownPhase>` | Convenience method to create a subscriber |

### ShutdownNotifier Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `subscribe()` | `Receiver<ShutdownPhase>` | Create a new phase subscriber |
| `cancellation_token()` | `CancellationToken` | Get the cancellation token |
| `is_shutdown_initiated()` | `bool` | Check if shutdown has started |

### ShutdownPhase Variants

| Variant | Fields | When Emitted |
|---------|--------|--------------|
| `Initiated` | None | Immediately when signal received |
| `GracePeriodStarted` | `timeout: Duration` | After `Initiated`, with configured timeout |
| `GracePeriodEnded` | None | After grace period timeout expires |

## Use Cases

### Database Connection Cleanup

```rust
use axum_conf::{Config, FluentRouter, ShutdownPhase};
use sqlx::PgPool;

async fn setup_db_cleanup(router: &FluentRouter, pool: PgPool) {
    let mut rx = router.subscribe_to_shutdown();

    tokio::spawn(async move {
        while let Ok(phase) = rx.recv().await {
            if let ShutdownPhase::GracePeriodEnded = phase {
                tracing::info!("Closing database connections");
                pool.close().await;
            }
        }
    });
}
```

### Background Job Queue

```rust
use axum_conf::{Config, FluentRouter, ShutdownPhase};
use std::sync::Arc;
use tokio::sync::Mutex;

struct JobQueue {
    jobs: Vec<Job>,
    accepting: bool,
}

async fn setup_job_queue(router: &FluentRouter, queue: Arc<Mutex<JobQueue>>) {
    let mut rx = router.subscribe_to_shutdown();
    let queue_clone = queue.clone();

    tokio::spawn(async move {
        while let Ok(phase) = rx.recv().await {
            match phase {
                ShutdownPhase::Initiated => {
                    // Stop accepting new jobs
                    queue_clone.lock().await.accepting = false;
                    tracing::info!("Job queue closed for new submissions");
                }
                ShutdownPhase::GracePeriodStarted { timeout } => {
                    let pending = queue_clone.lock().await.jobs.len();
                    tracing::info!(
                        "{} jobs pending, {}s to complete",
                        pending,
                        timeout.as_secs()
                    );
                }
                ShutdownPhase::GracePeriodEnded => {
                    let pending = queue_clone.lock().await.jobs.len();
                    if pending > 0 {
                        tracing::warn!("{} jobs abandoned", pending);
                    }
                }
            }
        }
    });
}

struct Job;
```

### External Service Connections

```rust
use axum_conf::{Config, FluentRouter};

async fn setup_external_services(router: &FluentRouter) {
    let token = router.cancellation_token();

    // Redis connection manager
    tokio::spawn(async move {
        let mut reconnect_interval = tokio::time::interval(
            std::time::Duration::from_secs(5)
        );

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    tracing::info!("Closing Redis connection");
                    // Close connection gracefully
                    break;
                }
                _ = reconnect_interval.tick() => {
                    // Check and maintain connection
                }
            }
        }
    });
}
```

### Multiple Subscribers

Multiple components can subscribe independently:

```rust
use axum_conf::{Config, FluentRouter, ShutdownPhase};

async fn setup_multi_subscriber(router: &FluentRouter) {
    // Subscriber 1: Logging
    let mut rx1 = router.subscribe_to_shutdown();
    tokio::spawn(async move {
        while let Ok(phase) = rx1.recv().await {
            tracing::info!("Shutdown phase: {:?}", phase);
        }
    });

    // Subscriber 2: Metrics
    let mut rx2 = router.subscribe_to_shutdown();
    tokio::spawn(async move {
        while let Ok(phase) = rx2.recv().await {
            if let ShutdownPhase::GracePeriodEnded = phase {
                // Flush metrics to external system
            }
        }
    });

    // Subscriber 3: Cache
    let mut rx3 = router.subscribe_to_shutdown();
    tokio::spawn(async move {
        while let Ok(phase) = rx3.recv().await {
            if let ShutdownPhase::Initiated = phase {
                // Stop cache refresh tasks
            }
        }
    });
}
```

## Configuration

The grace period timeout is configured in your TOML configuration:

```toml
[http]
shutdown_timeout = "30s"  # Time to wait for in-flight requests
```

For Kubernetes deployments, ensure your `terminationGracePeriodSeconds` is greater than `shutdown_timeout`:

```yaml
spec:
  terminationGracePeriodSeconds: 40  # > shutdown_timeout + buffer
  containers:
  - name: app
    lifecycle:
      preStop:
        exec:
          command: ["sleep", "5"]  # Wait for endpoint removal
```

## Best Practices

1. **Subscribe before starting**: Create subscribers before calling `start()` to ensure you don't miss early phases.

2. **Keep handlers fast**: Shutdown handlers should complete quickly. Don't block on long operations in phase handlers.

3. **Use the right tool**:
   - `CancellationToken` for simple "stop" signaling
   - `ShutdownPhase` subscribers for coordinated multi-stage cleanup

4. **Handle all phases**: Even if you only care about one phase, loop through all to avoid receiver backpressure.

5. **Log shutdown progress**: Use shutdown phases to log what's happening during shutdown for debugging.

## Thread Safety

Both `CancellationToken` and `ShutdownNotifier` are `Clone`, `Send`, and `Sync`. They can be safely shared across threads and tasks.

## See Also

- [Graceful Shutdown](../kubernetes/graceful-shutdown.md) - Kubernetes integration
- [Health Checks](../kubernetes/health-checks.md) - Liveness and readiness probes
