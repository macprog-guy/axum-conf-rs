# Benchmarking

This guide explains how to run benchmarks, interpret results, and add custom benchmarks to measure performance.

## Quick Start

Run all benchmarks:

```bash
make bench
```

Or directly with cargo:

```bash
cargo bench --all-features
```

## What Gets Benchmarked

The benchmark suite measures **middleware overhead** - the latency added by each middleware layer. Located in `benches/middleware_overhead.rs`.

### Benchmark Groups

| Benchmark | What It Measures |
|-----------|-----------------|
| `bare_axum` | Raw Axum router without axum-conf |
| `fluent_no_middleware` | FluentRouter with all middleware disabled |
| `individual_middleware/*` | Each middleware layer in isolation |
| `stack_scaling/*` | 1, 3, and 5 layer stacks |
| `cors` | CORS middleware (if feature enabled) |
| `helmet` | Security headers (if feature enabled) |
| `compression` | Compression layer (if feature enabled) |

## Running Benchmarks

### All Benchmarks

```bash
cargo bench --all-features
```

### Specific Benchmark

```bash
cargo bench --all-features -- individual_middleware/request_id
```

### Filter by Pattern

```bash
cargo bench --all-features -- stack_scaling
```

## Understanding Results

### Sample Output

```
bare_axum               time:   [1.2345 µs 1.2456 µs 1.2567 µs]
                        change: [-0.8234% +0.1234% +1.0234%] (p = 0.12 > 0.05)
                        No change in performance detected.

fluent_no_middleware    time:   [1.3456 µs 1.3567 µs 1.3678 µs]
                        change: [-0.5234% +0.2345% +0.9876%] (p = 0.34 > 0.05)
                        No change in performance detected.

individual_middleware/request_id
                        time:   [2.1234 µs 2.1456 µs 2.1678 µs]
                        change: [-1.2345% +0.0123% +1.2567%] (p = 0.98 > 0.05)
                        No change in performance detected.
```

### Reading the Output

| Field | Meaning |
|-------|---------|
| `time: [low est high]` | Lower bound, best estimate, upper bound |
| `change: [low% est% high%]` | Change from last run |
| `p = X.XX` | Statistical significance (< 0.05 = significant) |
| `thrpt` | Throughput (operations per second) |

### Performance Comparison

```
Baseline:    bare_axum           = 1.25 µs
FluentRouter (no middleware)     = 1.36 µs  (+9%)
+ request_id                     = 2.15 µs  (+72%)
+ logging                        = 3.45 µs  (+176%)
+ full stack (17 layers)         = ~15 µs   (~12x baseline)
```

## Benchmark Configuration

Benchmarks are powered by [Criterion.rs](https://github.com/bheisler/criterion.rs).

### Adjusting Sample Size

For faster iteration during development:

```rust
use criterion::Criterion;

fn custom_criterion() -> Criterion {
    Criterion::default()
        .sample_size(50)  // Default is 100
        .measurement_time(std::time::Duration::from_secs(3))
}

criterion_group! {
    name = benches;
    config = custom_criterion();
    targets = bench_my_feature
}
```

### Saving Baselines

```bash
# Save current results as baseline
cargo bench --all-features -- --save-baseline main

# Compare against baseline
cargo bench --all-features -- --baseline main
```

## Adding Custom Benchmarks

### Basic Benchmark

```rust
// benches/my_benchmark.rs
use criterion::{criterion_group, criterion_main, Criterion};
use axum::{Router, routing::get, body::Body, http::Request};
use axum_conf::{Config, FluentRouter};
use tower::ServiceExt;
use std::hint::black_box;

async fn handler() -> &'static str {
    "OK"
}

fn bench_my_feature(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut config = Config::default();
    config.http.with_metrics = false;  // Avoid global registry conflicts

    let router = FluentRouter::without_state(config)
        .unwrap()
        .route("/", get(handler))
        .into_inner();

    c.bench_function("my_feature", |b| {
        b.to_async(&rt).iter(|| async {
            let request = Request::builder()
                .method("GET")
                .uri("/")
                .body(Body::empty())
                .unwrap();

            let response = router.clone().oneshot(request).await.unwrap();
            black_box(response)
        })
    });
}

criterion_group!(benches, bench_my_feature);
criterion_main!(benches);
```

### Registering the Benchmark

Add to `Cargo.toml`:

```toml
[[bench]]
name = "my_benchmark"
harness = false
```

### Benchmarking with Parameters

```rust
use criterion::{BenchmarkId, Criterion};

fn bench_payload_sizes(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("payload_size");

    for size in [100, 1000, 10000, 100000] {
        let payload = "x".repeat(size);

        group.bench_with_input(
            BenchmarkId::new("bytes", size),
            &payload,
            |b, payload| {
                b.to_async(&rt).iter(|| async {
                    // Benchmark with this payload size
                })
            },
        );
    }

    group.finish();
}
```

## CI Integration

### GitHub Actions

```yaml
# .github/workflows/bench.yml
name: Benchmarks

on:
  pull_request:
    paths:
      - 'src/**'
      - 'benches/**'
      - 'Cargo.toml'

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable

      - name: Run benchmarks
        run: cargo bench --all-features -- --noplot

      - name: Upload results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion
```

### Performance Regression Detection

```yaml
- name: Check for regressions
  run: |
    cargo bench --all-features -- --baseline main --save-baseline pr
    # Fail if any benchmark regressed by more than 10%
```

## Profiling

### CPU Profiling with Flamegraph

```bash
# Install flamegraph
cargo install flamegraph

# Profile a benchmark
cargo flamegraph --bench middleware_overhead -- --bench
```

### Memory Profiling with DHAT

```rust
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    let _profiler = dhat::Profiler::new_heap();
    // Run benchmark code
}
```

## Interpreting Results

### What's Acceptable

| Overhead | Assessment |
|----------|------------|
| < 1 µs per layer | Excellent |
| 1-5 µs per layer | Good |
| 5-10 µs per layer | Acceptable |
| > 10 µs per layer | Investigate |

### Common Causes of Overhead

| Cause | Solution |
|-------|----------|
| Heap allocation | Use stack allocation or arenas |
| Cloning | Use `Arc` or references |
| Lock contention | Use lock-free structures |
| Regex compilation | Compile once, reuse |
| DNS/IO in hot path | Move to async task |

## Tips

1. **Disable metrics in benchmarks** - Prevents global registry conflicts:
   ```rust
   config.http.with_metrics = false;
   ```

2. **Disable rate limiting** - Prevents artificial throttling:
   ```rust
   config.http.max_requests_per_sec = 0;
   ```

3. **Use `black_box`** - Prevents compiler from optimizing away results:
   ```rust
   use std::hint::black_box;
   black_box(result);
   ```

4. **Warm up the runtime** - Criterion handles this automatically

5. **Run on quiet system** - Close other applications for consistent results

## HTML Reports

Criterion generates HTML reports in `target/criterion/`. Open `report/index.html` for interactive graphs showing:

- Distribution of timings
- Comparison with baseline
- Regression detection

## Next Steps

- [Getting Started](getting-started.md) - Project setup
- [Architecture](architecture.md) - Understand the middleware stack
- [Performance Middleware](middleware/performance.md) - Optimize your app
