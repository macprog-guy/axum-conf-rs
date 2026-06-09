//! Orchestration and router delegation: setup_middleware(), start(), layer(), route(), etc.

use super::router::FluentRouter;
use super::shutdown::{ShutdownNotifier, ShutdownPhase};
use crate::Result;

use {
    axum::{Router, body::Body, routing::Route},
    http::Request,
    std::{convert::Infallible, env, net::SocketAddr, time::Duration},
    tokio::signal,
    tower::{Layer, Service},
};

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up all standard middleware layers in the correct order.
    ///
    /// This is the **recommended way** to configure middleware. It handles the complex
    /// ordering requirements automatically, ensuring all layers work correctly together.
    ///
    /// # When to Use This Method
    ///
    /// **Use `setup_middleware()`** for most applications. It provides production-ready
    /// defaults and handles middleware dependencies automatically.
    ///
    /// **Use individual `setup_*` methods** only when you need:
    /// - Custom middleware ordering
    /// - Middleware between specific layers
    /// - Partial middleware stack (though `exclude` config is preferred)
    ///
    /// # What It Configures
    ///
    /// - Liveness/readiness probes
    /// - OIDC authentication (if `keycloak` feature enabled)
    /// - Request deduplication
    /// - Concurrency limits
    /// - Payload size limits
    /// - Compression/decompression
    /// - Path normalization
    /// - Sensitive header protection
    /// - Request ID generation
    /// - API versioning
    /// - CORS headers
    /// - Security headers (Helmet)
    /// - Logging and tracing
    /// - Metrics collection (Prometheus)
    /// - Request timeouts
    /// - Rate limiting
    /// - Panic recovery
    ///
    /// # Middleware Order
    ///
    /// **CRITICAL**: Middleware is processed outside-in for requests and inside-out for responses.
    /// The **last layer added is the outermost layer** and executes **first** on incoming requests.
    ///
    /// The current order (innermost → outermost) is grouped as:
    ///
    /// 1. **Authentication & routing** — protected static files, OIDC / Basic-Auth / proxy-header
    ///    authentication (applied as `route_layer`s), public static files, the OIDC login routes,
    ///    and session handling.
    /// 2. **Request shaping** — deduplication, concurrency limit, payload limit, (de)compression,
    ///    path normalization, sensitive-header redaction, and API versioning.
    /// 3. **Cross-cutting** — CORS, security headers (Helmet), logging, and metrics.
    /// 4. **Operational (outermost)** — readiness, timeout, rate limiting, request id, liveness,
    ///    and panic recovery (the outermost layer — it executes first and catches all inner panics).
    ///
    /// The exact, position-by-position order is the `MIDDLEWARE_ORDER` list in
    /// `src/fluent/tests/middleware/ordering.rs`, rendered as a table in `CLAUDE.md`; a doc-sync
    /// test fails if the two drift. The literal call sequence lives in the body of this method.
    ///
    /// # Manual Setup (Advanced)
    ///
    /// If you need custom ordering, call individual `setup_*` methods. **Important rules**:
    ///
    /// - **Call order matters**: Methods must be called in reverse execution order
    ///   (first method called = innermost layer = executes last on request)
    /// - **Dependencies**: Some middleware depends on others:
    ///   - `setup_request_id()` must be called **after** `setup_deduplication()` so the
    ///     request ID is available when deduplication checks for duplicates
    ///   - `setup_oidc()` requires `setup_session_handling()` (when using sessions)
    /// - **Don't call twice**: Each `setup_*` method should only be called once
    /// - **Configuration controls**: Use `[http.middleware] exclude/include` instead of
    ///   skipping methods, as this ensures proper dependency handling
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter, Result};
    /// # async fn example() -> Result<()> {
    /// // Manual setup example (not recommended unless you need custom ordering)
    /// let router = FluentRouter::without_state(Config::<()>::default())?
    ///     // Innermost layers first (execute last on request)
    ///     .setup_deduplication()
    ///     .setup_logging()
    ///     .setup_readiness()   // /ready - after timeout/rate limiting (benefits from protection)
    ///     .setup_timeout()
    ///     .setup_rate_limiting()
    ///     .setup_request_id()  // Outer to deduplication, generates ID early
    ///     .setup_liveness()    // /live - always accessible, very early
    ///     .setup_catch_panic();  // Outermost (executes first on request)
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Returns
    ///
    /// A `Result` containing the configured router or an error if setup fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - OIDC configuration is invalid (when `keycloak` feature enabled)
    /// - Configuration validation fails
    ///
    /// # Note
    ///
    /// Disable Prometheus in tests to avoid global registry conflicts:
    /// ```rust
    /// # use axum_conf::Config;
    /// let mut config: Config = Config::default();
    /// config.http.with_metrics = false;
    /// ```
    pub async fn setup_middleware(self) -> Result<Self> {
        // Output the current version of the service
        const PACKAGE_NAME: &str = env!("CARGO_PKG_NAME");
        const VERSION: &str = env!("CARGO_PKG_VERSION");
        tracing::info!("Starting {PACKAGE_NAME} version {VERSION}...");

        // Capture config values before moving self
        let default_api_version = self.config.http.default_api_version;

        // Middleware is added from innermost to outermost; the last layer added
        // executes FIRST on incoming requests. The trailing `position N` comments
        // refer to `MIDDLEWARE_ORDER` in `src/fluent/tests/middleware/ordering.rs`,
        // the single source of truth for this order (a doc-sync test keeps the
        // CLAUDE.md table aligned with it). Note: `route_layer` applies only to
        // routes added BEFORE it, so auth is applied first and the health
        // endpoints are added AFTER (so they're not protected by auth).

        // Protected static files must be added BEFORE auth so route_layer applies to them.
        let router = self.setup_protected_files()?; // position 1

        // Browser login redirect is the innermost route_layer so it runs AFTER all
        // auth middleware has resolved identity.
        #[cfg(feature = "keycloak")]
        let router = router.setup_browser_login_redirect(); // position 2

        #[cfg(feature = "keycloak")]
        let router = router.setup_oidc().await?; // position 3 (route_layer)

        #[cfg(feature = "basic-auth")]
        let router = router.setup_basic_auth()?; // position 4 (route_layer)

        let router = router.setup_proxy_oidc(); // position 5 (route_layer)

        // Public static files added AFTER auth so they're accessible without authentication.
        let router = router.setup_public_files()?; // position 6

        // OIDC auth code flow routes (login/callback/logout) - public, after auth middleware.
        #[cfg(feature = "keycloak")]
        let router = router.setup_oidc_routes().await?; // position 7

        let router = router.setup_user_span(); // position 8 (record username on the span)

        // Session handling must wrap auth middleware so sessions are established
        // before session_to_identity middleware reads them.
        #[cfg(feature = "session")]
        let router = router.setup_session_handling().await?; // position 9

        let router = router
            .setup_deduplication() // position 10
            .setup_concurrency_limit() // position 11
            .setup_max_payload_size() // position 12
            .setup_compression() // position 13
            .setup_path_normalization() // position 14
            .setup_sensitive_headers() // position 15
            .setup_api_versioning(default_api_version) // position 16
            .setup_cors() // position 17
            .setup_helmet() // position 18
            .setup_logging() // position 19
            .setup_metrics() // position 20
            .setup_readiness() // position 21 (benefits from timeout/rate limiting)
            .setup_timeout() // position 22
            .setup_rate_limiting() // position 23
            .setup_request_id() // position 24 (early so all requests get IDs)
            .setup_liveness() // position 25 (always accessible, very early)
            .setup_catch_panic() // position 26 (outermost - panic recovery)
            .setup_fallback_files()?; // position 27 (must be last)

        Ok(router)
    }

    /// Adds the remaining standard middleware layers in the correct order.
    /// These layers should be added last as they handle security, errors and panics.
    /// Since they are added last, they are the outermost layers and thus executed first.
    ///
    /// # Deprecated
    ///
    /// This method is deprecated. Use `setup_middleware()` instead, which now includes
    /// all middleware layers in the optimal order. This method is kept for backward
    /// compatibility but does nothing.
    #[must_use]
    #[deprecated(
        since = "0.2.2",
        note = "Use setup_middleware() instead, which now includes all layers"
    )]
    pub fn build(self) -> Self {
        // All middleware is now configured in setup_middleware()
        // This method is a no-op for backward compatibility
        self
    }

    /// Starts the HTTP server based on the current configuration.
    ///
    /// The server supports both HTTP/1.1 and HTTP/2 protocols automatically.
    /// HTTP/2 will be used when clients request it via ALPN negotiation.
    ///
    /// # Graceful Shutdown
    ///
    /// When a shutdown signal is received (SIGTERM or SIGINT), the server:
    ///
    /// 1. Emits [`ShutdownPhase::Initiated`] to all subscribers
    /// 2. Triggers the cancellation token (stopping background tasks)
    /// 3. Stops accepting new connections
    /// 4. Emits [`ShutdownPhase::GracePeriodStarted`] with the configured timeout
    /// 5. Waits for in-flight requests to complete (up to `shutdown_timeout`)
    /// 6. Emits [`ShutdownPhase::GracePeriodEnded`] if timeout expires
    /// 7. Exits
    ///
    /// If all connections drain before the timeout, shutdown completes early
    /// without waiting for the full timeout duration.
    ///
    /// Components can subscribe to these phases before calling `start()`:
    ///
    /// ```rust,no_run
    /// use axum_conf::{Config, FluentRouter, ShutdownPhase};
    ///
    /// # async fn example() -> axum_conf::Result<()> {
    /// let router = FluentRouter::without_state(Config::<()>::default())?;
    ///
    /// // Set up shutdown handlers BEFORE starting
    /// let mut shutdown_rx = router.subscribe_to_shutdown();
    ///
    /// tokio::spawn(async move {
    ///     while let Ok(phase) = shutdown_rx.recv().await {
    ///         tracing::info!("Shutdown phase: {:?}", phase);
    ///     }
    /// });
    ///
    /// // Now start the server
    /// router.setup_middleware().await?.start().await
    /// # }
    /// ```
    pub async fn start(self) -> Result<()>
    where
        State: Clone + Send + Sync + 'static,
    {
        let bind_addr = self.config.http.full_bind_addr();
        let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

        tracing::info!("Bound to {}", &bind_addr);
        tracing::info!("Waiting for connections");
        tracing::info!("Max req/s: {}", self.config.http.max_requests_per_sec);

        // Move the tracer provider out of `self` before `self.state`/`self.inner`
        // are consumed, so we can flush exported spans after shutdown completes.
        #[cfg(feature = "opentelemetry")]
        let otel_provider = self.otel_provider;

        let service = self
            .inner
            .with_state(self.state)
            .into_make_service_with_connect_info::<SocketAddr>();

        let shutdown_timeout = self.config.http.shutdown_timeout;
        let shutdown_notifier = self.shutdown_notifier.clone();

        // Subscribe to shutdown notifications to know when signal is received
        let mut shutdown_rx = shutdown_notifier.subscribe();

        let serve_future = axum::serve(listener, service).with_graceful_shutdown(
            shutdown_signal_with_notifications(shutdown_timeout, shutdown_notifier.clone()),
        );

        // Wait for graceful shutdown with timeout enforcement.
        // The timeout only starts AFTER a shutdown signal is received, not immediately.
        // If connections drain before the timeout, we complete early.
        // If the timeout expires first, we emit GracePeriodEnded and stop waiting:
        // dropping `serve_future` stops accepting connections, but in-flight handler
        // tasks already spawned by hyper are abandoned, not actively cancelled.
        tokio::select! {
            result = serve_future => {
                // Server shut down gracefully (connections drained)
                tracing::info!("Graceful shutdown completed");
                result?;
            }
            _ = async {
                // Wait for shutdown to be initiated before starting the timeout
                loop {
                    match shutdown_rx.recv().await {
                        Ok(ShutdownPhase::Initiated) => break,
                        Ok(_) => continue,
                        Err(_) => return, // Channel closed
                    }
                }
                // Now start the timeout (only after signal received)
                tokio::time::sleep(shutdown_timeout).await;
            } => {
                // Timeout expired after shutdown was initiated: stop waiting and
                // let the server stop accepting. In-flight requests still running
                // past the grace period are abandoned rather than force-cancelled.
                tracing::warn!(
                    "Graceful shutdown timeout expired; stopping the server \
                     (in-flight requests may be abandoned)"
                );
                shutdown_notifier.emit(ShutdownPhase::GracePeriodEnded);
            }
        }

        // Flush any buffered OpenTelemetry spans before exiting. The batch
        // exporter would otherwise drop un-flushed spans at process exit.
        #[cfg(feature = "opentelemetry")]
        if let Some(provider) = otel_provider
            && let Err(e) = provider.shutdown()
        {
            tracing::warn!(error = %e, "Failed to flush OpenTelemetry tracer provider on shutdown");
        }

        Ok(())
    }

    /// Adds a custom Tower middleware layer to the router.
    ///
    /// This is a low-level method that forwards to `axum::Router::layer()`,
    /// allowing you to add custom middleware that isn't provided by the library.
    ///
    /// # Type Parameters
    ///
    /// * `L` - A Tower Layer that produces services compatible with Axum
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use tower::limit::ConcurrencyLimitLayer;
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    ///
    /// let router = FluentRouter::without_state(Config::<()>::default())?
    ///     .layer(ConcurrencyLimitLayer::new(100));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request<Body>> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request<Body>>>::Response: axum::response::IntoResponse + 'static,
        <L::Service as Service<Request<Body>>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        self.inner = self.inner.layer(layer);
        self
    }

    /// Adds a new route to the router at the specified path.
    ///
    /// Routes define how HTTP requests to specific paths are handled.
    /// Use the routing helpers from `axum::routing` to create method routers:
    /// - `get()` - Handle GET requests
    /// - `post()` - Handle POST requests
    /// - `put()` - Handle PUT requests
    /// - `delete()` - Handle DELETE requests
    /// - And more...
    ///
    /// # Arguments
    ///
    /// * `path` - The URL path pattern for this route (e.g., "/users/:id")
    /// * `route` - A `MethodRouter` created with `axum::routing` helpers
    ///
    /// # Examples
    ///
    /// ```
    /// use axum_conf::{Config, FluentRouter};
    /// use axum::routing::get;
    ///
    /// async fn handler() -> &'static str {
    ///     "Hello, World!"
    /// }
    ///
    /// # async fn example() {
    /// let config: Config = Config::default();
    /// let router = FluentRouter::without_state(config)
    ///     .unwrap()
    ///     .route("/hello", get(handler))
    ///     .into_inner();
    /// # }
    /// ```
    #[must_use]
    pub fn route(mut self, path: &str, route: axum::routing::MethodRouter<State>) -> Self {
        self.inner = self.inner.route(path, route);
        self
    }

    /// Adds a middleware layer that only applies to routes, not services.
    ///
    /// This is a low-level method that forwards to `axum::Router::route_layer()`.
    /// Unlike `layer()`, this only affects route handlers and doesn't wrap
    /// nested services.
    ///
    /// # Type Parameters
    ///
    /// * `L` - A Tower Layer that produces services compatible with Axum
    ///
    /// # Use Cases
    ///
    /// Use this when you want middleware to only affect your route handlers
    /// but not services like `ServeDir` or nested routers.
    #[must_use]
    pub fn route_layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request<Body>> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request<Body>>>::Response: axum::response::IntoResponse + 'static,
        <L::Service as Service<Request<Body>>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        self.inner = self.inner.route_layer(layer);
        self
    }

    /// Nests another router at a specific path prefix.
    ///
    /// All routes in the nested router will be prefixed with the given path.
    /// Middleware added to the nested router only affects its own routes.
    ///
    /// # Arguments
    ///
    /// * `path` - The path prefix (must start with `/`)
    /// * `router` - The router to nest
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use axum::{Router, routing::get};
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    ///
    /// let api_v1 = Router::new()
    ///     .route("/users", get(|| async { "users" }));
    ///
    /// let app = FluentRouter::without_state(Config::<()>::default())?
    ///     .nest("/api/v1", api_v1);  // Routes at /api/v1/users
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn nest(mut self, path: &str, router: Router<State>) -> Self {
        self.inner = self.inner.nest(path, router);
        self
    }

    /// Nests a Tower service at a specific path prefix.
    ///
    /// Similar to `nest()` but for raw Tower services instead of Axum routers.
    /// Commonly used for serving static files with `ServeDir`.
    ///
    /// # Arguments
    ///
    /// * `path` - The path prefix (must start with `/`)
    /// * `service` - The Tower service to nest
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use axum::{Router, routing::get};
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    ///
    /// let service = Router::new().route("/health", get(|| async { "OK" }));
    /// let app = FluentRouter::without_state(Config::<()>::default())?
    ///     .nest_service("/api", service);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn nest_service<T>(mut self, path: &str, service: T) -> Self
    where
        T: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        T::Future: Send + 'static,
    {
        self.inner = self.inner.nest_service(path, service);
        self
    }

    /// Merges another router into this one.
    ///
    /// Routes and services from the other router are added to this router.
    /// Unlike `nest()`, routes are not prefixed - they're added at the same level.
    ///
    /// # Arguments
    ///
    /// * `other` - The router to merge
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use axum::{Router, routing::get};
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    ///
    /// let user_routes = Router::new()
    ///     .route("/users", get(|| async { "users" }));
    ///
    /// let app = FluentRouter::without_state(Config::<()>::default())?
    ///     .merge(user_routes);  // Routes directly at /users
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Common Pattern
    ///
    /// Use `merge()` to combine route modules:
    /// ```rust,no_run
    /// # use axum::Router;
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    /// # fn api_routes() -> Router { Router::new() }
    /// # fn admin_routes() -> Router { Router::new() }
    /// FluentRouter::without_state(Config::<()>::default())?
    ///     .merge(api_routes())
    ///     .merge(admin_routes());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn merge(mut self, other: Router<State>) -> Self {
        self.inner = self.inner.merge(other);
        self
    }

    /// Adds a Tower service at a specific route.
    ///
    /// Unlike `nest_service()`, this adds the service at an exact path rather
    /// than a path prefix.
    ///
    /// # Arguments
    ///
    /// * `path` - The exact route path
    /// * `service` - The Tower service to add
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use tower::service_fn;
    /// use http::Response;
    /// # use axum_conf::{Config, FluentRouter};
    /// # async fn example() -> axum_conf::Result<()> {
    ///
    /// let service = service_fn(|_req| async {
    ///     Ok::<_, std::convert::Infallible>(Response::new("Hello".into()))
    /// });
    ///
    /// let app = FluentRouter::without_state(Config::<()>::default())?
    ///     .route_service("/custom", service);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn route_service<T>(mut self, path: &str, service: T) -> Self
    where
        T: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        T::Future: Send + 'static,
    {
        self.inner = self.inner.route_service(path, service);
        self
    }

    /// Consumes the `FluentRouter` and returns the underlying `axum::Router`.
    ///
    /// Use this when you need direct access to the Axum router, typically for
    /// testing or when you want to add additional middleware that requires
    /// the concrete `Router` type.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    /// let fluent = FluentRouter::without_state(Config::<()>::default())?;
    /// let axum_router: axum::Router = fluent.into_inner();
    /// # Ok(())
    /// # }
    /// ```
    pub fn into_inner(self) -> Router<State> {
        self.inner
    }
}

/// Returns a signal handler that emits shutdown phase notifications.
///
/// This function:
/// 1. Waits for SIGTERM or SIGINT (Ctrl+C)
/// 2. Emits [`ShutdownPhase::Initiated`] (and triggers the cancellation token)
/// 3. Emits [`ShutdownPhase::GracePeriodStarted`] with the configured timeout
/// 4. Returns immediately to let axum start graceful shutdown
///
/// The grace period timeout is enforced by the caller (see [`FluentRouter::start`]),
/// which wraps the serve call with a timeout. When connections drain before the
/// timeout, shutdown completes early. If the timeout expires first,
/// [`ShutdownPhase::GracePeriodEnded`] is emitted and shutdown is forced.
///
/// Components can subscribe to these phases to perform coordinated cleanup.
///
/// If signal registration fails, the function logs a warning and falls back to
/// waiting indefinitely. This ensures the server continues running even if signal
/// handlers cannot be installed (e.g., in restricted environments).
pub(crate) async fn shutdown_signal_with_notifications(
    timeout: Duration,
    notifier: ShutdownNotifier,
) {
    let ctrl_c = async {
        match signal::ctrl_c().await {
            Ok(()) => {
                tracing::debug!("Ctrl+C signal received");
            }
            Err(err) => {
                tracing::warn!("Failed to install Ctrl+C handler: {}", err);
                // Wait indefinitely if we can't install the handler
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal_handler) => {
                signal_handler.recv().await;
                tracing::debug!("SIGTERM signal received");
            }
            Err(err) => {
                tracing::warn!("Failed to install SIGTERM handler: {}", err);
                // Wait indefinitely if we can't install the handler
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    // Phase 1: Initiated - signal received, cancellation token triggered
    tracing::info!(
        "Shutdown signal received, starting graceful shutdown (timeout: {}s)",
        timeout.as_secs()
    );
    let subscriber_count = notifier.emit(ShutdownPhase::Initiated);
    tracing::debug!(
        "Shutdown initiated notification sent to {} subscriber(s)",
        subscriber_count
    );

    // Phase 2: Grace period started - in-flight requests draining
    // Return immediately to let axum start graceful shutdown.
    // The timeout is enforced by the caller wrapping the serve call.
    notifier.emit(ShutdownPhase::GracePeriodStarted { timeout });
}

/// Returns a signal handler that allows us to stop the server using Ctrl+C
/// or the terminate signal, which in turn allows us to perform a graceful
/// shutdown with a configurable timeout.
///
/// If signal registration fails, the function logs a warning and falls back to
/// waiting indefinitely. This ensures the server continues running even if signal
/// handlers cannot be installed (e.g., in restricted environments).
///
/// # Deprecated
///
/// This function is deprecated. Use [`shutdown_signal_with_notifications`] instead,
/// which emits [`ShutdownPhase`] events for coordinated shutdown handling.
/// Note: The timeout is now enforced by the caller, not within this function.
#[allow(dead_code)]
#[deprecated(
    since = "0.4.0",
    note = "Use shutdown_signal_with_notifications instead for shutdown phase notifications"
)]
pub(crate) async fn shutdown_signal_with_timeout(timeout: Duration) {
    shutdown_signal_with_notifications(timeout, ShutdownNotifier::default()).await;
}
