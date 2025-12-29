//! Benchmarks for measuring middleware overhead.
//!
//! These benchmarks measure the latency added by each middleware layer
//! to help identify performance bottlenecks and track regressions.

use axum::{body::Body, http::Request, routing::get, Router};
use axum_conf::{Config, FluentRouter, HttpMiddlewareConfig, HttpMiddleware};
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::hint::black_box;
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

    let router = Router::new().route("/", get(handler));

    c.bench_function("bare_axum", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router
                .clone()
                .oneshot(test_request("/"))
                .await
                .unwrap();
            black_box(response)
        })
    });
}

/// Benchmark: FluentRouter with no middleware (baseline)
fn bench_no_middleware(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut config = test_config();
    // Disable all middleware
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![]));

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .into_inner();

    c.bench_function("fluent_no_middleware", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router
                .clone()
                .oneshot(test_request("/"))
                .await
                .unwrap();
            black_box(response)
        })
    });
}

/// Benchmark: Individual middleware layers
fn bench_individual_middleware(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
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
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
                black_box(response)
            })
        });
    }

    // Logging only
    {
        let mut config = test_config();
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::Logging,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_logging()
            .into_inner();

        group.bench_function("logging", |b| {
            b.to_async(&rt).iter(|| async {
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
                black_box(response)
            })
        });
    }

    // Timeout only
    {
        let mut config = test_config();
        config.http.request_timeout = Some(std::time::Duration::from_secs(30));
        config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
            HttpMiddleware::Timeout,
        ]));

        let router = FluentRouter::without_state(config)
            .unwrap()
            .route("/", get(handler))
            .setup_timeout()
            .into_inner();

        group.bench_function("timeout", |b| {
            b.to_async(&rt).iter(|| async {
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
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
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
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

    let mut config = test_config();
    config.http.cors = Some(HttpCorsConfig::default());
    config.http.middleware = Some(HttpMiddlewareConfig::Include(vec![
        HttpMiddleware::Cors,
    ]));

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .setup_cors()
        .into_inner();

    c.bench_function("cors", |b| {
        b.to_async(&rt).iter(|| async {
            let response = router
                .clone()
                .oneshot(test_request("/"))
                .await
                .unwrap();
            black_box(response)
        })
    });
}

#[cfg(feature = "security-headers")]
fn bench_helmet(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

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
            let response = router
                .clone()
                .oneshot(test_request("/"))
                .await
                .unwrap();
            black_box(response)
        })
    });
}

#[cfg(feature = "compression")]
fn bench_compression(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

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
            let response = router
                .clone()
                .oneshot(test_request("/"))
                .await
                .unwrap();
            black_box(response)
        })
    });
}

/// Benchmark: Middleware stack scaling
fn bench_middleware_stack_scaling(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
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
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
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
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
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
            .setup_liveness_readiness()
            .into_inner();

        group.bench_with_input(BenchmarkId::new("layers", 5), &router, |b, router| {
            b.to_async(&rt).iter(|| async {
                let response = router
                    .clone()
                    .oneshot(test_request("/"))
                    .await
                    .unwrap();
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
    bench_middleware_stack_scaling,
);

// Feature-gated benchmarks need separate groups and conditional main
#[cfg(all(feature = "cors", feature = "security-headers", feature = "compression"))]
criterion_group!(
    feature_benches,
    bench_cors,
    bench_helmet,
    bench_compression,
);

#[cfg(all(feature = "cors", feature = "security-headers", feature = "compression"))]
criterion_main!(benches, feature_benches);

#[cfg(not(all(feature = "cors", feature = "security-headers", feature = "compression")))]
criterion_main!(benches);
