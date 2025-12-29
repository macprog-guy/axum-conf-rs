//! Observability middleware: logging, metrics, and OpenTelemetry tracing.

use super::router::FluentRouter;
use crate::HttpMiddleware;

use {
    axum::body::Body,
    http::Request,
    tower_http::trace::TraceLayer as TowerHTTPLayer,
};

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
    /// # Note
    ///
    /// Disable metrics in tests to avoid conflicts with the global Prometheus registry:
    /// ```rust
    /// # use axum_conf::Config;
    /// let mut config = Config::default();
    /// config.http.with_metrics = false;
    /// ```
    #[cfg(feature = "metrics")]
    #[must_use]
    pub fn setup_metrics(mut self) -> Self {
        if self.config.http.with_metrics && self.is_middleware_enabled(HttpMiddleware::Metrics) {
            const PACKAGE_NAME: &str = env!("CARGO_PKG_NAME");
            let metrics_path: &str =
                Box::leak(self.config.http.metrics_route.clone().into_boxed_str());
            let (prometheus_layer, metrics_handle) = PrometheusMetricLayerBuilder::new()
                .enable_response_body_size(true)
                .with_prefix(PACKAGE_NAME)
                .with_ignore_pattern(metrics_path)
                .with_default_metrics()
                .build_pair();

            self.inner = self
                .inner
                .route(metrics_path, axum::routing::get(|| async move { metrics_handle.render() }))
                .layer(prometheus_layer);
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
            return self;
        }

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

                        // Set the extracted context as the parent of this span
                        span.set_parent(context).ok();
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
    /// let config = Config::default();
    /// let router = FluentRouter::without_state(config)?
    ///     .setup_opentelemetry()?
    ///     .setup_logging()
    ///     .into_inner();
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "opentelemetry")]
    pub fn setup_opentelemetry(self) -> Result<Self> {
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

            // Add the OpenTelemetry layer to the existing tracing subscriber
            // This allows tracing spans to be exported as OpenTelemetry spans
            let _ = tracing_subscriber::registry().with(otel_layer).try_init();

            tracing::info!(
                endpoint = %otel_config.endpoint,
                "OpenTelemetry tracing initialized with context propagation"
            );
        }
        Ok(self)
    }
}
