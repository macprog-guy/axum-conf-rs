//! Observability middleware: logging, metrics, and OpenTelemetry tracing.

use super::router::FluentRouter;
use crate::HttpMiddleware;

use {axum::body::Body, http::Request, tower_http::trace::TraceLayer as TowerHTTPLayer};

#[cfg(feature = "metrics")]
use axum_prometheus::PrometheusMetricLayerBuilder;

#[cfg(feature = "opentelemetry")]
use {
    crate::{Error, Result},
    opentelemetry::{global, trace::TracerProvider},
    opentelemetry_otlp::WithExportConfig,
    opentelemetry_sdk::{
        Resource,
        trace::{RandomIdGenerator, Sampler},
    },
    tracing_opentelemetry::OpenTelemetrySpanExt,
};

/// Configures a Prometheus builder from the customized metrics settings.
///
/// Replicates `axum-prometheus`'s default handle exactly for the built-in HTTP
/// duration histogram — matching on the **prefixed** metric name, because the
/// metrics layer records its own duration series under the prefixed key — then
/// layers the user's per-metric buckets, global labels, and idle timeout on top.
/// Fallible bucket calls are threaded with `?`; the only error they return is the
/// empty-bucket case, which the loop pre-guards.
#[cfg(feature = "metrics")]
fn configure_metrics_recorder(
    buckets: &[crate::MetricBucketsConfig],
    global_labels: std::collections::BTreeMap<String, String>,
    idle_timeout: Option<std::time::Duration>,
) -> std::result::Result<
    axum_prometheus::metrics_exporter_prometheus::PrometheusBuilder,
    axum_prometheus::metrics_exporter_prometheus::BuildError,
> {
    use axum_prometheus::metrics_exporter_prometheus::{Matcher, PrometheusBuilder};
    use metrics_util::MetricKindMask;

    // (1) Preserve the built-in duration histogram EXACTLY as the default handle
    //     does — matching on the PREFIXED name. `with_prefix` populates this
    //     global before this closure runs; the metrics layer records its own
    //     duration series under that prefixed key, so the matcher must use it.
    let duration_name = axum_prometheus::PREFIXED_HTTP_REQUESTS_DURATION_SECONDS
        .get()
        .map(String::as_str)
        .unwrap_or(axum_prometheus::AXUM_HTTP_REQUESTS_DURATION_SECONDS)
        .to_string();
    let mut recorder = PrometheusBuilder::new().set_buckets_for_metric(
        Matcher::Full(duration_name),
        axum_prometheus::utils::SECONDS_DURATION_BUCKETS,
    )?;

    // (2) Per-metric custom buckets. Empty lists are skipped (the only error
    //     `set_buckets_for_metric` returns is the empty-bucket case).
    for cfg in buckets {
        if cfg.buckets.is_empty() {
            tracing::warn!(
                metric = %cfg.metric,
                "metrics_buckets entry has an empty bucket list; skipping"
            );
            continue;
        }
        recorder = recorder.set_buckets_for_metric(cfg.to_matcher(), &cfg.buckets)?;
    }

    // (3) Global constant labels (consumed by value — no clone).
    for (key, value) in global_labels {
        recorder = recorder.add_global_label(key, value);
    }

    // (4) Idle eviction across all metric kinds (opt-in).
    if let Some(timeout) = idle_timeout {
        recorder = recorder.idle_timeout(MetricKindMask::ALL, Some(timeout));
    }

    Ok(recorder)
}

/// Builds and installs the customized Prometheus recorder, returning its handle.
///
/// On a configuration error (unreachable in practice — see
/// [`configure_metrics_recorder`]) it logs and falls back to a plain recorder.
/// On a double-install it logs a warning and returns a throwaway handle so
/// `/metrics` still renders (empty) instead of panicking — strictly more robust
/// than the default handle, which `.expect()`s on `set_global_recorder`.
#[cfg(feature = "metrics")]
fn build_custom_metrics_recorder(
    buckets: &[crate::MetricBucketsConfig],
    global_labels: std::collections::BTreeMap<String, String>,
    idle_timeout: Option<std::time::Duration>,
    upkeep_timeout: Option<std::time::Duration>,
) -> axum_prometheus::metrics_exporter_prometheus::PrometheusHandle {
    use axum_prometheus::metrics_exporter_prometheus::PrometheusBuilder;

    let recorder =
        configure_metrics_recorder(buckets, global_labels, idle_timeout).unwrap_or_else(|error| {
            tracing::error!(
                %error,
                "failed to configure custom Prometheus metrics; using a plain recorder"
            );
            PrometheusBuilder::new()
        });

    match recorder.install_recorder() {
        Ok(handle) => {
            let upkeep_handle = handle.clone();
            let interval = upkeep_timeout.unwrap_or(std::time::Duration::from_secs(5));
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(interval).await;
                    upkeep_handle.run_upkeep();
                }
            });
            handle
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "failed to install custom Prometheus recorder (a global recorder is \
                 likely already installed); /metrics will render an empty registry"
            );
            PrometheusBuilder::new().build_recorder().handle()
        }
    }
}

/// The recorder-affecting settings a Prometheus recorder was installed with.
///
/// Kept next to the installed handle so a later call with different settings can
/// be detected and reported (the recorder is fixed at install time).
#[cfg(feature = "metrics")]
#[derive(Debug, Clone, PartialEq)]
struct MetricsRecorderSettings {
    buckets: Vec<crate::MetricBucketsConfig>,
    global_labels: std::collections::BTreeMap<String, String>,
    idle_timeout: Option<std::time::Duration>,
    upkeep_timeout: Option<std::time::Duration>,
}

#[cfg(feature = "metrics")]
impl MetricsRecorderSettings {
    fn from_config(http: &crate::HttpConfig) -> Self {
        Self {
            buckets: http.metrics_buckets.clone(),
            global_labels: http.metrics_global_labels.clone(),
            idle_timeout: http.metrics_idle_timeout,
            upkeep_timeout: http.metrics_upkeep_timeout,
        }
    }

    /// `true` when any opt-in customization is set. When nothing is configured the
    /// byte-for-byte default recorder (identical to releases before 0.7.1) is used.
    fn is_custom(&self) -> bool {
        !self.buckets.is_empty()
            || !self.global_labels.is_empty()
            || self.idle_timeout.is_some()
            || self.upkeep_timeout.is_some()
    }
}

/// The process-wide Prometheus recorder handle and the settings it was built with.
///
/// The `metrics` facade permits a single global recorder per process, so it is
/// installed at most once and every later request for it gets a clone.
#[cfg(feature = "metrics")]
static INSTALLED_METRICS: std::sync::OnceLock<(
    axum_prometheus::metrics_exporter_prometheus::PrometheusHandle,
    MetricsRecorderSettings,
)> = std::sync::OnceLock::new();

/// Returns the `'static` metrics route, allocated at most once per process.
///
/// `with_ignore_pattern` borrows the pattern for the layer's `'static` lifetime.
/// A process-lifetime `OnceLock` avoids `Box::leak` and repeated allocation.
#[cfg(feature = "metrics")]
fn metrics_path(http: &crate::HttpConfig) -> &'static str {
    static METRICS_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    METRICS_PATH
        .get_or_init(|| http.metrics_route.clone())
        .as_str()
}

/// Builds the metrics layer and returns it with the process-wide recorder handle,
/// installing the recorder on first use and reusing it afterwards.
#[cfg(feature = "metrics")]
fn metrics_pair(
    http: &crate::HttpConfig,
) -> (
    axum_prometheus::PrometheusMetricLayer<'static>,
    axum_prometheus::metrics_exporter_prometheus::PrometheusHandle,
) {
    const PACKAGE_NAME: &str = env!("CARGO_PKG_NAME");

    // axum-prometheus panics if the metric-name prefix is set twice, so it is
    // applied by the first caller only. The lock makes that check-and-set (and the
    // install below) atomic across threads; a poisoned lock is still usable.
    static BUILD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static PREFIX_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let _guard = BUILD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let settings = MetricsRecorderSettings::from_config(http);
    let mut builder = PrometheusMetricLayerBuilder::new()
        .enable_response_body_size(true)
        .with_ignore_pattern(metrics_path(http));
    if !PREFIX_SET.swap(true, std::sync::atomic::Ordering::SeqCst) {
        builder = builder.with_prefix(PACKAGE_NAME);
    }

    // `with_metrics_from_fn` invokes the closure SYNCHRONOUSLY, after
    // `with_prefix` has populated the prefix globals. Installing inside
    // `get_or_init` makes the install-and-store step race-free.
    builder
        .with_metrics_from_fn(|| {
            let (handle, installed) = INSTALLED_METRICS.get_or_init(|| {
                let handle = if settings.is_custom() {
                    build_custom_metrics_recorder(
                        &settings.buckets,
                        settings.global_labels.clone(),
                        settings.idle_timeout,
                        settings.upkeep_timeout,
                    )
                } else {
                    axum_prometheus::Handle::default().0
                };
                (handle, settings.clone())
            });
            if *installed != settings {
                tracing::warn!(
                    "a Prometheus recorder is already installed with different metrics settings \
                     (metrics_buckets / metrics_global_labels / metrics_idle_timeout / \
                     metrics_upkeep_timeout); reusing it and ignoring the new settings"
                );
            }
            handle.clone()
        })
        .build_pair()
}

#[cfg(feature = "metrics")]
impl<T> crate::Config<T>
where
    T: Clone + Default,
{
    /// Installs the Prometheus recorder and returns the metrics layer and handle.
    ///
    /// This does what [`FluentRouter::setup_metrics`] does internally, but lets you
    /// do it up front — for example to record metrics before the router exists, or
    /// to serve `/metrics` yourself. The returned pair is:
    ///
    /// - a [`PrometheusMetricLayer`](crate::PrometheusMetricLayer) to add to a router, and
    /// - a [`PrometheusHandle`](crate::PrometheusHandle) whose `render()` produces the
    ///   scrape output.
    ///
    /// The recorder is installed **once per process** and honours the
    /// `metrics_buckets`, `metrics_global_labels`, `metrics_idle_timeout` and
    /// `metrics_upkeep_timeout` settings of the first call. Later calls — including
    /// the one made by `setup_metrics()` — reuse the installed recorder. If a later
    /// call has different recorder settings, a warning is logged and the new
    /// settings are ignored.
    ///
    /// Returns `None` when `http.with_metrics` is `false` (nothing is installed).
    ///
    /// # Panics
    ///
    /// Must be called from within a Tokio runtime: the recorder spawns a periodic
    /// upkeep task.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use axum_conf::Config;
    /// # #[tokio::main] async fn main() {
    /// let mut config = Config::new();
    /// config.http.with_metrics = true;
    /// let (layer, handle) = config.install_metrics().expect("metrics enabled");
    /// metrics::counter!("startup_total").increment(1);
    /// println!("{}", handle.render());
    /// # drop(layer);
    /// # }
    /// ```
    #[must_use]
    pub fn install_metrics(
        &self,
    ) -> Option<(
        axum_prometheus::PrometheusMetricLayer<'static>,
        axum_prometheus::metrics_exporter_prometheus::PrometheusHandle,
    )> {
        self.http.with_metrics.then(|| metrics_pair(&self.http))
    }
}

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up Prometheus metrics collection and endpoint.
    ///
    /// When `config.http.with_metrics` is true, this method:
    /// - Adds a metrics endpoint at the configured route (default: `/metrics`)
    /// - Installs Prometheus metric collection middleware
    /// - Tracks request counts, durations, and HTTP status codes
    ///
    /// Metrics are exposed in Prometheus format for scraping by monitoring systems.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// with_metrics = true
    /// metrics_route = "/metrics"
    /// ```
    ///
    /// # Customization (opt-in)
    ///
    /// By default the recorder is built exactly as in prior releases. Setting any
    /// of the following `[http]` fields switches to a customized recorder; the
    /// built-in HTTP duration histogram is always preserved:
    ///
    /// - `metrics_buckets` — per-metric histogram bucket overrides. Turns a metric
    ///   recorded via the global `metrics` facade into a true bucketed histogram
    ///   (`_bucket{le=…}`) instead of the default summary. The `metric` name is the
    ///   **raw** recorded name (facade metrics are *not* `axum_conf_`-prefixed).
    /// - `metrics_global_labels` — constant labels added to every exported series.
    /// - `metrics_idle_timeout` — evict metrics not updated within the duration.
    /// - `metrics_upkeep_timeout` — interval of the recorder upkeep loop (default 5s).
    ///
    /// ```toml
    /// [[http.metrics_buckets]]
    /// metric  = "widget_seconds"
    /// buckets = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]
    ///
    /// [http.metrics_global_labels]
    /// service = "widget"
    /// ```
    ///
    /// **Backward compatibility:** when none of these are set, `/metrics` output is
    /// byte-for-byte identical to releases before 0.7.1.
    ///
    /// # Note
    ///
    /// Disable metrics in tests to avoid conflicts with the global Prometheus registry:
    /// ```rust
    /// # use axum_conf::Config;
    /// let mut config: Config = Config::default();
    /// config.http.with_metrics = false;
    /// ```
    #[cfg(feature = "metrics")]
    #[must_use]
    pub fn setup_metrics(mut self) -> Self {
        if self.config.http.with_metrics && self.is_middleware_enabled(HttpMiddleware::Metrics) {
            tracing::trace!(
                route = %self.config.http.metrics_route,
                "Metrics middleware enabled"
            );
            let metrics_path = metrics_path(&self.config.http);
            // Reuses the process-wide recorder if one is already installed (e.g. by
            // `Config::install_metrics`), otherwise installs it now.
            let (prometheus_layer, metrics_handle) = metrics_pair(&self.config.http);

            let metrics_service = axum::routing::get(|| async move { metrics_handle.render() });

            #[cfg(feature = "basic-auth")]
            let mut router =
                if let Some(basic_auth_cfg) = self.config.http.metrics_basic_auth.clone() {
                    self.secure_with_basic_auth(&basic_auth_cfg, |r| {
                        r.route(metrics_path, metrics_service)
                    })
                } else {
                    self.inner = self.inner.route(metrics_path, metrics_service);
                    self
                };
            #[cfg(not(feature = "basic-auth"))]
            let mut router = {
                self.inner = self.inner.route(metrics_path, metrics_service);
                self
            };

            router.inner = router.inner.layer(prometheus_layer);
            return router;
        }
        self
    }

    /// No-op when `metrics` feature is disabled.
    #[cfg(not(feature = "metrics"))]
    #[must_use]
    pub fn setup_metrics(self) -> Self {
        if self.config.http.with_metrics {
            tracing::warn!(
                "Metrics are enabled in config but the 'metrics' feature is not enabled. \
                 Add `metrics` to your Cargo.toml features to enable metrics support."
            );
        }
        self
    }

    /// Sets up HTTP request/response logging middleware.
    ///
    /// Adds structured tracing for all HTTP requests, logging:
    /// - Request method and path
    /// - Response status code
    /// - Request duration
    /// - Client IP address
    /// - Request ID (when available)
    ///
    /// Log output format is controlled by the `logging.format` configuration.
    /// Request IDs are automatically included in the trace span context.
    ///
    /// When OpenTelemetry is enabled, extracts trace context from incoming W3C traceparent headers
    /// and propagates it through the application for distributed tracing.
    #[must_use]
    pub fn setup_logging(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::Logging) {
            tracing::trace!("Logging middleware skipped (disabled in config)");
            return self;
        }

        tracing::trace!("Logging middleware enabled");
        self.inner = self
            .inner
            .layer(
                TowerHTTPLayer::new_for_http().make_span_with(|request: &Request<Body>| {
                    let request_id = request
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("unknown");

                    let span = tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        request_id = %request_id,
                        user = tracing::field::Empty,
                    );

                    // Extract OpenTelemetry context from incoming headers if feature is enabled
                    #[cfg(feature = "opentelemetry")]
                    {
                        use opentelemetry::propagation::Extractor;

                        // Create an extractor that reads from HTTP headers
                        struct HeaderExtractor<'a>(&'a http::HeaderMap);

                        impl<'a> Extractor for HeaderExtractor<'a> {
                            fn get(&self, key: &str) -> Option<&str> {
                                self.0.get(key).and_then(|v| v.to_str().ok())
                            }

                            fn keys(&self) -> Vec<&str> {
                                self.0.keys().map(|k| k.as_str()).collect()
                            }
                        }

                        let extractor = HeaderExtractor(request.headers());
                        let context =
                            opentelemetry::global::get_text_map_propagator(|propagator| {
                                propagator.extract(&extractor)
                            });

                        // Set the extracted context as the parent of this span.
                        if let Err(e) = span.set_parent(context) {
                            tracing::debug!(
                                error = %e,
                                "Failed to attach extracted OpenTelemetry context to span"
                            );
                        }
                    }

                    span
                }),
            );

        self
    }

    /// Initializes OpenTelemetry distributed tracing with W3C Trace Context propagation.
    ///
    /// Sets up OTLP export to a collector (e.g., Jaeger, Tempo) for distributed tracing and
    /// configures automatic extraction and injection of W3C `traceparent` and `tracestate` headers.
    /// This enables seamless trace propagation across microservices.
    ///
    /// This must be called before other setup methods to ensure traces are properly captured.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [logging.opentelemetry]
    /// endpoint = "http://localhost:4317"
    /// service_name = "my-service"
    /// ```
    ///
    /// # Returns
    ///
    /// A `Result` containing the configured router or an error if initialization fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Cannot connect to the OTLP endpoint
    /// - Configuration is invalid
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # async fn example() -> axum_conf::Result<()> {
    /// let config: Config = Config::default();
    /// let router = FluentRouter::without_state(config)?
    ///     .setup_opentelemetry()?
    ///     .setup_logging()
    ///     .into_inner();
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "opentelemetry")]
    pub fn setup_opentelemetry(mut self) -> Result<Self> {
        if let Some(otel_config) = &self.config.logging.opentelemetry {
            use tracing_subscriber::prelude::*;

            let service_name = otel_config
                .service_name
                .clone()
                .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());

            // Create OTLP exporter using new() with endpoint
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&otel_config.endpoint)
                .build()
                .map_err(|e| Error::internal(format!("Failed to create OTLP exporter: {}", e)))?;

            // Create tracer provider
            let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_id_generator(RandomIdGenerator::default())
                .with_sampler(Sampler::AlwaysOn)
                .with_resource(
                    Resource::builder()
                        .with_service_name(service_name.clone())
                        .build(),
                )
                .build();

            // Set as global tracer provider
            global::set_tracer_provider(provider.clone());

            // Set up W3C trace context propagation (traceparent/tracestate headers)
            global::set_text_map_propagator(
                opentelemetry_sdk::propagation::TraceContextPropagator::new(),
            );

            // Create OpenTelemetry tracing layer to bridge tracing spans to OpenTelemetry
            let tracer = provider.tracer(service_name);
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

            // Compose the OTel layer with the fmt + env-filter subscriber and
            // initialize ONCE. Previously this built a separate, disconnected
            // registry whose `try_init` silently lost to any prior init (so spans
            // never exported). `setup_tracing_with` owns the single global
            // subscriber, so callers must NOT also call `Config::setup_tracing()`
            // when OpenTelemetry is enabled — this method initializes logging too.
            let endpoint = otel_config.endpoint.clone();
            self.config
                .setup_tracing_with(|subscriber| subscriber.with(otel_layer));

            // Retain the provider so its batch exporter can be flushed on shutdown.
            self.otel_provider = Some(provider);

            tracing::info!(
                endpoint = %endpoint,
                "OpenTelemetry tracing initialized with context propagation"
            );
        }
        Ok(self)
    }
}
