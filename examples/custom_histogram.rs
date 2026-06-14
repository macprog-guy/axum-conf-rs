//! Custom Prometheus Histogram Buckets Example
//!
//! Shows how to turn an application metric recorded through the global `metrics`
//! facade into a true bucketed Prometheus **histogram** (instead of the default
//! summary) by configuring `[[http.metrics_buckets]]`, plus a global constant
//! label applied to every series.
//!
//! Run with:
//! ```bash
//! cargo run --example custom_histogram --features metrics
//! ```
//!
//! Then scrape the endpoint and look for the bucketed series:
//! ```bash
//! curl -s http://127.0.0.1:3000/work
//! curl -s http://127.0.0.1:3000/metrics | grep widget_seconds
//! # widget_seconds_bucket{service="widget-demo",le="0.005"} ...
//! # widget_seconds_bucket{service="widget-demo",le="0.01"}  ...
//! # widget_seconds_sum{service="widget-demo"} ...
//! # widget_seconds_count{service="widget-demo"} ...
//! ```

use axum::routing::get;
use axum_conf::{Config, FluentRouter, Result};

/// Records a sample into the `widget_seconds` histogram on each request. The
/// metric name here is the RAW recorded name — it is not `axum_conf_`-prefixed,
/// so the `[[http.metrics_buckets]]` entry below uses the same raw name.
async fn work() -> &'static str {
    metrics::histogram!("widget_seconds").record(0.03);
    "recorded a widget_seconds sample"
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::new()
        .with_bind_addr("127.0.0.1")
        .with_bind_port(3000)
        // Render `widget_seconds` as a bucketed histogram rather than a summary.
        .with_metric_buckets(
            "widget_seconds",
            [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0],
        )
        // Tag every exported series with `service="widget-demo"`.
        .with_metrics_global_label("service", "widget-demo");

    config.setup_tracing();

    println!("Starting server on http://127.0.0.1:3000");
    println!("  GET /work     -> records a widget_seconds sample");
    println!("  GET /metrics  -> Prometheus exposition (look for widget_seconds_bucket)");

    FluentRouter::without_state(config)?
        .route("/work", get(work))
        .setup_middleware()
        .await?
        .start()
        .await
}
