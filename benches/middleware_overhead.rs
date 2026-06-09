//! Benchmarks for measuring middleware overhead.
//!
//! These benchmarks measure the latency added by each middleware layer
//! to help identify performance bottlenecks and track regressions.

use axum::extract::FromRequestParts;
use axum::{Router, body::Body, http::Request, routing::get};
use axum_conf::{
    AuthMethod, AuthenticatedIdentity, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig,
    SharedIdentity,
};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use tower::ServiceExt;

/// Simple handler that returns immediately
async fn handler() -> &'static str {
    "OK"
}

/// Creates a test config with metrics disabled (to avoid global registry conflicts)
fn test_config() -> Config {
    let mut config = Config::default();
    config.http.with_metrics = false;
    config.http.max_requests_per_sec = 0; // Disable rate limiting for benchmarks
    config
}

/// Creates a minimal request for benchmarking
fn test_request(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

/// Benchmark: Bare axum router (no axum-conf middleware)
fn bench_bare_axum(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();

    let router = Router::new().route("/", get(handler));

    c.bench_function("bare_axum", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router.clone().oneshot(test_request("/")).await.unwrap();
            black_box(response)
        })
    });
}

/// Benchmark: FluentRouter with no middleware (baseline)
fn bench_no_middleware(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();

    let mut config = test_config();
    // Disable all middleware
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![]));

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .into_inner();

    c.bench_function("fluent_no_middleware", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router.clone().oneshot(test_request("/")).await.unwrap();
            black_box(response)
        })
    });
}

/// Benchmark: Individual middleware layers
fn bench_individual_middleware(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();
    let mut group = c.benchmark_group("individual_middleware");

    // Request ID only
    {
        let mut config = test_config();
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::RequestId,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_request_id()
            .into_inner();

        group.bench_function("request_id", |b| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    // Logging only
    {
        let mut config = test_config();
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![HttpMiddleware::Logging]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_logging()
            .into_inner();

        group.bench_function("logging", |b| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    // Timeout only
    {
        let mut config = test_config();
        config.http.request_timeout = Some(std::time::Duration::from_secs(30));
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![HttpMiddleware::Timeout]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_timeout()
            .into_inner();

        group.bench_function("timeout", |b| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    // Catch panic only
    {
        let mut config = test_config();
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::CatchPanic,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_catch_panic()
            .into_inner();

        group.bench_function("catch_panic", |b| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    group.finish();
}

/// Benchmark: Feature-gated middleware (only runs if features are enabled)
#[cfg(feature = "cors")]
fn bench_cors(c: &mut Criterion) {
    use axum_conf::HttpCorsConfig;

    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();

    let mut config = test_config();
    config.http.cors = Some(HttpCorsConfig::default());
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![HttpMiddleware::Cors]));

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .setup_cors()
        .into_inner();

    c.bench_function("cors", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router.clone().oneshot(test_request("/")).await.unwrap();
            black_box(response)
        })
    });
}

#[cfg(feature = "security-headers")]
fn bench_helmet(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();

    let mut config = test_config();
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
        HttpMiddleware::SecurityHeaders,
    ]));

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .setup_helmet()
        .into_inner();

    c.bench_function("helmet", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router.clone().oneshot(test_request("/")).await.unwrap();
            black_box(response)
        })
    });
}

#[cfg(feature = "compression")]
fn bench_compression(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();

    let mut config = test_config();
    config.http.support_compression = true;
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
        HttpMiddleware::Compression,
    ]));

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .setup_compression()
        .into_inner();

    c.bench_function("compression", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router.clone().oneshot(test_request("/")).await.unwrap();
            black_box(response)
        })
    });
}

/// Benchmark: identity extraction — deep-cloning `AuthenticatedIdentity` vs the
/// refcount-only `SharedIdentity`. Demonstrates the per-request allocation saved
/// on the hottest authenticated path for read-only handlers.
fn bench_identity_extraction(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();
    let identity = Arc::new(AuthenticatedIdentity {
        method: AuthMethod::Oidc,
        user: "alice@example.com".to_string(),
        email: Some("alice@example.com".to_string()),
        groups: vec!["staff".to_string(), "eng".to_string(), "oncall".to_string()],
        roles: vec!["admin".to_string(), "editor".to_string()],
        preferred_username: Some("alice".to_string()),
        access_token: None,
    });

    // Build request `Parts` carrying the shared identity (mirrors what the auth
    // middleware inserts). Extraction reads it without removing it.
    let make_parts = |id: Arc<AuthenticatedIdentity>| {
        let mut req = Request::builder().body(()).unwrap();
        req.extensions_mut().insert(id);
        req.into_parts().0
    };

    let mut group = c.benchmark_group("identity_extraction");

    group.bench_function("authenticated_identity_clone", |b| {
        b.to_async(&rt).iter(|| {
            let id = Arc::clone(&identity);
            async move {
                let mut parts = make_parts(id);
                let extracted =
                    <AuthenticatedIdentity as FromRequestParts<()>>::from_request_parts(
                        &mut parts,
                        &(),
                    )
                    .await
                    .unwrap();
                black_box(extracted)
            }
        })
    });

    group.bench_function("shared_identity_arc", |b| {
        b.to_async(&rt).iter(|| {
            let id = Arc::clone(&identity);
            async move {
                let mut parts = make_parts(id);
                let extracted =
                    <SharedIdentity as FromRequestParts<()>>::from_request_parts(&mut parts, &())
                        .await
                        .unwrap();
                black_box(extracted)
            }
        })
    });

    group.finish();
}

/// Benchmark: request deduplication under both the duplicate (replay) path and
/// the new-request path, guarding the borrowed fast path and O(1) eviction.
#[cfg(feature = "deduplication")]
fn bench_deduplication(c: &mut Criterion) {
    use std::sync::atomic::{AtomicU64, Ordering};

    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();

    let mut config = test_config();
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
        HttpMiddleware::RequestId,
        HttpMiddleware::RequestDeduplication,
    ]));
    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .setup_request_id()
        .setup_deduplication()
        .into_inner();

    let request_with_id = |id: &str| {
        Request::builder()
            .method("GET")
            .uri("/")
            .header("x-request-id", id)
            .body(Body::empty())
            .unwrap()
    };

    let mut group = c.benchmark_group("deduplication");

    // Duplicate path: the same id repeats, so every call after the first is a
    // borrowed-lookup duplicate (409).
    group.bench_function("duplicate", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router
                .clone()
                .oneshot(request_with_id("fixed-id"))
                .await
                .unwrap();
            black_box(response)
        })
    });

    // New-request path: a fresh id each call exercises insert + eviction.
    let counter = AtomicU64::new(0);
    group.bench_function("unique", |b| {
        b.to_async(&rt).iter(|| async {
            let id = format!("id-{}", counter.fetch_add(1, Ordering::Relaxed));
            let response = router.clone().oneshot(request_with_id(&id)).await.unwrap();
            black_box(response)
        })
    });

    group.finish();
}

/// Benchmark: Middleware stack scaling
fn bench_middleware_stack_scaling(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    // Enter the runtime so synchronous setup (e.g. building a FluentRouter whose
    // lazy Postgres pool spawns a reaper) has a Tokio context.
    let _guard = rt.enter();
    let mut group = c.benchmark_group("stack_scaling");

    // 1 layer
    {
        let mut config = test_config();
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::RequestId,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_request_id()
            .into_inner();

        group.bench_with_input(BenchmarkId::new("layers", 1), &router, |b, router| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    // 3 layers
    {
        let mut config = test_config();
        config.http.request_timeout = Some(std::time::Duration::from_secs(30));
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::Logging,
            HttpMiddleware::Timeout,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_request_id()
            .setup_logging()
            .setup_timeout()
            .into_inner();

        group.bench_with_input(BenchmarkId::new("layers", 3), &router, |b, router| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    // 5 layers
    {
        let mut config = test_config();
        config.http.request_timeout = Some(std::time::Duration::from_secs(30));
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::RequestId,
            HttpMiddleware::Logging,
            HttpMiddleware::Timeout,
            HttpMiddleware::CatchPanic,
            HttpMiddleware::Liveness,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_request_id()
            .setup_logging()
            .setup_timeout()
            .setup_catch_panic()
            .setup_readiness()
            .setup_liveness()
            .into_inner();

        group.bench_with_input(BenchmarkId::new("layers", 5), &router, |b, router| {
            b.to_async(&rt).iter(|| async {
                let response = router.clone().oneshot(test_request("/")).await.unwrap();
                black_box(response)
            })
        });
    }

    group.finish();
}

// Define criterion groups - all benchmarks in a single group for simplicity
criterion_group!(
    benches,
    bench_bare_axum,
    bench_no_middleware,
    bench_individual_middleware,
    bench_identity_extraction,
    bench_middleware_stack_scaling,
);

// Deduplication bench is gated on its own feature.
#[cfg(feature = "deduplication")]
criterion_group!(dedup_benches, bench_deduplication);

// Feature-gated benchmarks need separate groups and conditional main.
#[cfg(all(
    feature = "cors",
    feature = "security-headers",
    feature = "compression"
))]
criterion_group!(feature_benches, bench_cors, bench_helmet, bench_compression,);

// One `criterion_main` per feature combination (cors+helmet+compression × dedup).
#[cfg(all(
    feature = "cors",
    feature = "security-headers",
    feature = "compression",
    feature = "deduplication"
))]
criterion_main!(benches, feature_benches, dedup_benches);

#[cfg(all(
    feature = "cors",
    feature = "security-headers",
    feature = "compression",
    not(feature = "deduplication")
))]
criterion_main!(benches, feature_benches);

#[cfg(all(
    not(all(
        feature = "cors",
        feature = "security-headers",
        feature = "compression"
    )),
    feature = "deduplication"
))]
criterion_main!(benches, dedup_benches);

#[cfg(all(
    not(all(
        feature = "cors",
        feature = "security-headers",
        feature = "compression"
    )),
    not(feature = "deduplication")
))]
criterion_main!(benches);
