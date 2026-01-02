//! Core FluentRouter struct and initialization methods.

#[cfg(any(feature = "rate-limiting", feature = "deduplication"))]
use tokio_util::task::AbortOnDropHandle;
use tokio_util::sync::CancellationToken;

use {
    super::shutdown::ShutdownNotifier,
    crate::{Config, HttpMiddleware, Result, StaticDirRoute},
    axum::Router,
    tokio::sync::broadcast,
    tower_http::{services::fs::ServeDir, set_header::SetResponseHeaderLayer},
};

/// Fluent builder for axum::Router with configuration-based middleware setup.
///
/// This wrapper around `axum::Router` provides a fluent API for configuring middleware
/// and routes based on the application configuration. Create instances using
/// [`FluentRouter::without_state`] or [`FluentRouter::with_state`].
///
/// The router forwards layering and nesting calls to the underlying `axum::Router`,
/// allowing middleware to be set up at any stage through dedicated `setup_*` methods.
///
/// If the configuration has a static files directory configured as a fallback,
/// it will be automatically set up. For all other directories, call
/// [`FluentRouter::setup_directories`] to install the necessary middleware.
///
/// # Graceful Shutdown
///
/// `FluentRouter` provides built-in support for graceful shutdown notifications.
/// Components can subscribe to shutdown events or use a cancellation token:
///
/// ```rust,no_run
/// use axum_conf::{Config, FluentRouter, ShutdownPhase};
///
/// # async fn example() -> axum_conf::Result<()> {
/// let router = FluentRouter::without_state(Config::default())?;
///
/// // Option 1: Simple cancellation token for background tasks
/// let token = router.cancellation_token();
/// tokio::spawn(async move {
///     loop {
///         tokio::select! {
///             _ = token.cancelled() => break,
///             _ = do_work() => {}
///         }
///     }
/// });
///
/// // Option 2: Subscribe to shutdown phases for complex cleanup
/// let mut rx = router.shutdown_notifier().subscribe();
/// tokio::spawn(async move {
///     while let Ok(phase) = rx.recv().await {
///         match phase {
///             ShutdownPhase::Initiated => println!("Shutting down..."),
///             _ => {}
///         }
///     }
/// });
/// # async fn do_work() {}
/// # Ok(())
/// # }
/// ```
pub struct FluentRouter<State = ()> {
    pub(crate) config: Config,
    pub(crate) state: State,
    pub(crate) inner: Router<State>,
    #[cfg(feature = "rate-limiting")]
    pub(crate) governor_handle: Option<AbortOnDropHandle<()>>,
    #[cfg(feature = "deduplication")]
    pub(crate) dedup_cleanup_handle: Option<AbortOnDropHandle<()>>,
    pub(crate) panic_channel: Option<tokio::sync::mpsc::Sender<String>>,
    pub(crate) shutdown_notifier: ShutdownNotifier,
    #[cfg(feature = "postgres")]
    pub(crate) db_pool: sqlx_postgres::PgPool,
    #[cfg(feature = "circuit-breaker")]
    pub(crate) circuit_breaker_registry: crate::circuit_breaker::CircuitBreakerRegistry,
}

impl FluentRouter {
    /// Creates a new `FluentRouter` without application state.
    pub fn without_state(config: Config) -> Result<FluentRouter<()>> {
        FluentRouter::<()>::with_state(config, ())
    }
}

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Creates a new `FluentRouter` with the provided configuration.
    ///
    /// Validates the configuration and sets up any fallback static file directories.
    /// Other static directories must be set up explicitly using `setup_directories()`.
    /// If a configuration for a database pool is provided, the pool will be created
    /// and made available for health checks. It can also be accessed via a call to
    /// `db_pool()`.
    ///
    /// # Arguments
    ///
    /// * `config` - The service configuration
    ///
    /// # Returns
    ///
    /// A `Result` containing the new `FluentRouter` or an error if configuration is invalid.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Configuration validation fails
    /// - Fallback directories are marked as protected
    /// - Required configuration values are missing
    pub fn with_state<S: Clone + Send + Sync + 'static>(
        config: Config,
        state: S,
    ) -> Result<FluentRouter<S>> {
        // Validate the configuration
        config.validate()?;

        #[cfg(feature = "postgres")]
        let db_pool = config.create_pgpool()?;

        #[cfg(feature = "circuit-breaker")]
        let circuit_breaker_registry =
            crate::circuit_breaker::CircuitBreakerRegistry::new(&config.circuit_breaker);

        // Create the base router and add public fallback files if configured
        let me = FluentRouter {
            config,
            state,
            inner: Router::new(),
            #[cfg(feature = "rate-limiting")]
            governor_handle: None,
            #[cfg(feature = "deduplication")]
            dedup_cleanup_handle: None,
            panic_channel: None,
            shutdown_notifier: ShutdownNotifier::default(),
            #[cfg(feature = "postgres")]
            db_pool,
            #[cfg(feature = "circuit-breaker")]
            circuit_breaker_registry,
        };

        me.setup_fallback_files()
    }

    /// Returns a reference to the shutdown notifier.
    ///
    /// Use this to subscribe to shutdown phase notifications for coordinated
    /// cleanup across multiple components.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use axum_conf::{Config, FluentRouter, ShutdownPhase};
    ///
    /// # async fn example() -> axum_conf::Result<()> {
    /// let router = FluentRouter::without_state(Config::default())?;
    /// let notifier = router.shutdown_notifier();
    ///
    /// // Subscribe from multiple places
    /// let mut rx1 = notifier.subscribe();
    /// let mut rx2 = notifier.subscribe();
    ///
    /// // Each subscriber receives all phases
    /// tokio::spawn(async move {
    ///     while let Ok(phase) = rx1.recv().await {
    ///         match phase {
    ///             ShutdownPhase::Initiated => {
    ///                 // Close external connections
    ///             }
    ///             ShutdownPhase::GracePeriodStarted { timeout } => {
    ///                 // Log remaining time
    ///             }
    ///             ShutdownPhase::GracePeriodEnded => {
    ///                 // Flush buffers
    ///             }
    ///         }
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn shutdown_notifier(&self) -> &ShutdownNotifier {
        &self.shutdown_notifier
    }

    /// Returns a cancellation token that is triggered when shutdown begins.
    ///
    /// This is a convenience method equivalent to calling
    /// `router.shutdown_notifier().cancellation_token()`.
    ///
    /// The token is triggered when the server receives a shutdown signal (SIGTERM/SIGINT).
    /// Use it in background tasks to gracefully stop work.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use axum_conf::{Config, FluentRouter};
    /// use std::time::Duration;
    ///
    /// # async fn example() -> axum_conf::Result<()> {
    /// let router = FluentRouter::without_state(Config::default())?;
    /// let token = router.cancellation_token();
    ///
    /// // Background task that respects shutdown
    /// tokio::spawn(async move {
    ///     let mut interval = tokio::time::interval(Duration::from_secs(60));
    ///     loop {
    ///         tokio::select! {
    ///             _ = token.cancelled() => {
    ///                 tracing::info!("Periodic task stopping");
    ///                 break;
    ///             }
    ///             _ = interval.tick() => {
    ///                 // Do periodic work
    ///                 tracing::debug!("Running periodic task");
    ///             }
    ///         }
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Multiple Tokens
    ///
    /// Each call returns a new clone of the token. All tokens share the same
    /// cancellation state - when one is cancelled, all are cancelled:
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example() -> axum_conf::Result<()> {
    /// let router = FluentRouter::without_state(Config::default())?;
    ///
    /// let token1 = router.cancellation_token();
    /// let token2 = router.cancellation_token();
    ///
    /// // Both tokens will be cancelled simultaneously on shutdown
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.shutdown_notifier.cancellation_token()
    }

    /// Returns a receiver for shutdown phase notifications.
    ///
    /// This is a convenience method equivalent to calling
    /// `router.shutdown_notifier().subscribe()`.
    ///
    /// Each call creates a new independent subscriber. Subscribers created
    /// after a phase is emitted will not receive that phase.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use axum_conf::{Config, FluentRouter, ShutdownPhase};
    ///
    /// # async fn example() -> axum_conf::Result<()> {
    /// let router = FluentRouter::without_state(Config::default())?;
    /// let mut rx = router.subscribe_to_shutdown();
    ///
    /// tokio::spawn(async move {
    ///     while let Ok(phase) = rx.recv().await {
    ///         tracing::info!("Shutdown phase: {:?}", phase);
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn subscribe_to_shutdown(&self) -> broadcast::Receiver<super::shutdown::ShutdownPhase> {
        self.shutdown_notifier.subscribe()
    }

    /// Returns the configured PostgreSQL database pool.
    #[cfg(feature = "postgres")]
    pub fn db_pool(&self) -> sqlx_postgres::PgPool {
        self.db_pool.clone()
    }

    /// Returns the circuit breaker registry.
    ///
    /// Use this to access circuit breakers for external service calls.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # fn example(router: &FluentRouter) {
    /// let breaker = router.circuit_breakers().get_or_default("payment-api");
    /// if breaker.should_allow() {
    ///     // make external call
    /// }
    /// # }
    /// ```
    #[cfg(feature = "circuit-breaker")]
    pub fn circuit_breakers(&self) -> &crate::circuit_breaker::CircuitBreakerRegistry {
        &self.circuit_breaker_registry
    }

    /// Returns a guarded database pool with circuit breaker protection.
    ///
    /// The returned [`crate::circuit_breaker::GuardedPool`] wraps the underlying database pool and
    /// tracks failures/successes for the specified target circuit breaker.
    ///
    /// # Arguments
    ///
    /// * `target` - The circuit breaker target name (e.g., "database")
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # async fn example(router: &FluentRouter) {
    /// let pool = router.guarded_db_pool("database");
    /// // Use pool.fetch_one(), pool.execute(), etc.
    /// # }
    /// ```
    #[cfg(all(feature = "circuit-breaker", feature = "postgres"))]
    pub fn guarded_db_pool(&self, target: &str) -> crate::circuit_breaker::GuardedPool {
        crate::circuit_breaker::GuardedPool::new(
            self.db_pool.clone(),
            self.circuit_breaker_registry.clone(),
            target,
        )
    }

    /// Helper method to check if a middleware is enabled in the configuration.
    /// Returns true if no middleware config is specified (all enabled by default),
    /// or if the middleware is explicitly enabled/not excluded.
    pub(crate) fn is_middleware_enabled(&self, middleware: HttpMiddleware) -> bool {
        self.config
            .http
            .middleware
            .as_ref()
            .map(|config| config.is_enabled(middleware))
            .unwrap_or(true) // If no middleware config, all are enabled
    }

    /// Sets a notification channel for panic messages.
    ///
    /// When configured, any panics caught by the panic handler middleware will
    /// send a message to this channel. Useful for monitoring, alerting, or logging
    /// panic events in production.
    ///
    /// # Arguments
    ///
    /// * `ch` - A tokio mpsc sender for panic notification messages
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # async fn example() -> axum_conf::Result<()> {
    /// let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    /// let config = Config::default();
    ///
    /// let router = FluentRouter::without_state(config)?
    ///     .with_panic_notification_channel(tx);
    ///
    /// // In another task, receive panic notifications
    /// tokio::spawn(async move {
    ///     while let Some(panic_msg) = rx.recv().await {
    ///         eprintln!("Panic caught: {}", panic_msg);
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_panic_notification_channel(self, ch: tokio::sync::mpsc::Sender<String>) -> Self {
        Self {
            panic_channel: Some(ch),
            ..self
        }
    }

    /// Sets up all static directories configured in the HTTP section except the fallback one.
    /// If public is true, only unprotected directories will be added.
    /// Otherwise only protected directories are added.
    pub fn setup_directories(mut self, protected: bool) -> Result<Self> {
        // Add all other static directories
        for dir in &self.config.http.directories {
            if let StaticDirRoute::Route(route) = &dir.route
                && dir.protected == protected
            {
                let serve_dir = ServeDir::new(&dir.directory)
                    .append_index_html_on_directories(true)
                    .precompressed_br()
                    .precompressed_gzip();

                // Add cache headers if configured
                if let Some(max_age) = dir.cache_max_age {
                    let cache_value = format!("public, max-age={}", max_age);
                    self.inner = self.inner.nest_service(
                        route,
                        tower::ServiceBuilder::new()
                            .layer(SetResponseHeaderLayer::if_not_present(
                                http::header::CACHE_CONTROL,
                                http::HeaderValue::from_str(&cache_value)?,
                            ))
                            .service(serve_dir),
                    );
                } else {
                    self.inner = self.inner.nest_service(route, serve_dir);
                }
            }
        }
        Ok(self)
    }

    /// Sets up a fallback static file directory if configured.
    pub fn setup_fallback_files(mut self) -> Result<Self> {
        if let Some(dir) = self
            .config
            .http
            .directories
            .iter()
            .find(|dir| dir.is_fallback())
        {
            let serve_dir = ServeDir::new(&dir.directory)
                .append_index_html_on_directories(true)
                .precompressed_br()
                .precompressed_gzip();

            // Add cache headers if configured
            if let Some(max_age) = dir.cache_max_age {
                let cache_value = format!("public, max-age={}", max_age);
                self.inner = self.inner.fallback_service(
                    tower::ServiceBuilder::new()
                        .layer(SetResponseHeaderLayer::if_not_present(
                            http::header::CACHE_CONTROL,
                            http::HeaderValue::from_str(&cache_value)?,
                        ))
                        .service(serve_dir),
                );
            } else {
                self.inner = self.inner.fallback_service(serve_dir);
            }
        }
        Ok(self)
    }

    /// Sets up public (unprotected) static file directories.
    ///
    /// Convenience method that calls `setup_directories(false)` to serve
    /// static files that don't require authentication.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [[http.directories]]
    /// directory = "./public"
    /// route = "/static"
    /// protected = false
    /// ```
    pub fn setup_public_files(self) -> Result<Self> {
        self.setup_directories(false)
    }

    /// Sets up protected static file directories that require authentication.
    ///
    /// Convenience method that calls `setup_directories(true)` to serve
    /// static files only to authenticated users. Must be called after
    /// authentication middleware is set up.
    ///
    /// # Examples
    ///
    /// ```toml
    /// [[http.directories]]
    /// directory = "./private"
    /// route = "/downloads"
    /// protected = true
    /// ```
    pub fn setup_protected_files(self) -> Result<Self> {
        self.setup_directories(true)
    }
}
