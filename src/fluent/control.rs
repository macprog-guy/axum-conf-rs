//! Traffic control middleware: rate limiting and panic catching.

use super::router::FluentRouter;
use crate::HttpMiddleware;

use {
    http::{Response, StatusCode},
    tower_http::catch_panic::CatchPanicLayer,
};

#[cfg(feature = "rate-limiting")]
use {
    std::time::Duration,
    tokio_util::task::AbortOnDropHandle,
    tower_governor::{GovernorLayer, governor::GovernorConfigBuilder},
};

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up per-IP rate limiting middleware.
    ///
    /// Limits the number of requests per second from each IP address. When the
    /// limit is exceeded, requests receive a `429 Too Many Requests` response.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// max_requests_per_sec = 100  # Default
    /// ```
    ///
    /// # Implementation
    ///
    /// Uses the token bucket algorithm:
    /// - Each IP gets a bucket with tokens
    /// - Each request consumes one token
    /// - Tokens refill at the configured rate
    ///
    /// # Notes
    ///
    /// Rate limiting is per IP address. Behind a reverse proxy, ensure the
    /// client's real IP is forwarded correctly.
    ///
    /// This middleware is automatically included in `setup_middleware()` as one of the
    /// outermost layers to reject excessive traffic early.
    #[cfg(feature = "rate-limiting")]
    #[must_use]
    pub fn setup_rate_limiting(mut self) -> Self {
        // Skip rate limiting if max_requests_per_sec is 0
        // This is useful for tests using oneshot() which don't have ConnectInfo<SocketAddr>
        if self.config.http.max_requests_per_sec > 0
            && self.is_middleware_enabled(HttpMiddleware::RateLimiting)
        {
            // Used for rate limiting below
            let governor_conf = Box::new(
                GovernorConfigBuilder::default()
                    .per_nanosecond((1_000_000_000 / self.config.http.max_requests_per_sec) as u64)
                    .burst_size(self.config.http.max_requests_per_sec)
                    .finish()
                    .expect("Failed to build governor config for rate limiting"),
            );

            // Spawn a background thread to periodically clean up old entries
            let governor_limiter = governor_conf.limiter().clone();
            let interval = Duration::from_secs(60);

            // Spawn a background task to clean up old entries
            let handle = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(interval).await;
                    governor_limiter.retain_recent();
                    tracing::debug!("remaining rate limiting quotas: {}", governor_limiter.len());
                }
            });

            // Wrap the handle so that it gets cancelled when the router is dropped
            self.governor_handle = Some(AbortOnDropHandle::new(handle));

            // Add the GovernorLayer for rate limiting
            self.inner = self.inner.layer(GovernorLayer::new(governor_conf));
        }
        self
    }

    /// No-op when `rate-limiting` feature is disabled.
    #[cfg(not(feature = "rate-limiting"))]
    #[must_use]
    pub fn setup_rate_limiting(self) -> Self {
        if self.config.http.max_requests_per_sec > 0 {
            tracing::warn!(
                "Rate limiting is configured but the 'rate-limiting' feature is not enabled. \
                 Add `rate-limiting` to your Cargo.toml features to enable rate limiting support."
            );
        }
        self
    }

    /// Sets up panic catching middleware.
    ///
    /// Catches panics in request handlers and returns a `500 Internal Server Error`
    /// response instead of crashing the server. Optionally sends panic details to
    /// a notification channel if configured with `with_panic_notification_channel()`.
    ///
    /// # Panic Handling
    ///
    /// When a handler panics:
    /// 1. Panic is caught before it crashes the server
    /// 2. Client receives 500 response
    /// 3. Panic message is sent to notification channel (if configured)
    /// 4. Server continues running
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use axum_conf::{Config, FluentRouter};
    /// # async fn example() -> axum_conf::Result<()> {
    /// let (tx, rx) = tokio::sync::mpsc::channel(100);
    ///
    /// FluentRouter::without_state(Config::default())?
    ///     .with_panic_notification_channel(tx)
    ///     .setup_catch_panic();
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Production Use
    ///
    /// Essential for production to prevent panics from taking down the server.
    /// That's why this middleware cannot be disabled.
    ///
    /// This middleware is automatically included in `setup_middleware()` as the
    /// outermost layer to ensure ALL panics are caught.
    #[must_use]
    pub fn setup_catch_panic(mut self) -> Self {
        // Note: Panic catching is critical and should generally not be disabled
        // But we still respect the configuration for testing purposes
        if !self.is_middleware_enabled(HttpMiddleware::CatchPanic) {
            return self;
        }

        let panic_channel = self.panic_channel.clone();
        self.inner = self.inner.layer(CatchPanicLayer::custom(
            move |err: Box<dyn std::any::Any + Send + 'static>| {
                // NOTE: taken verbatime from the source of DefaultResponseForPanic
                let msg = if let Some(s) = err.downcast_ref::<String>() {
                    format!("Service panicked: {}", s)
                } else if let Some(s) = err.downcast_ref::<&str>() {
                    format!("Service panicked: {}", s)
                } else {
                    "`CatchPanic` was unable to downcast the panic info".to_string()
                };

                tracing::error!("Service panicked: {}", msg);
                if let Some(ch) = &panic_channel {
                    ch.try_send(msg).ok();
                }

                // Build the final response - use unwrap_or_else to avoid panicking
                // inside the panic handler
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
                    .body("Internal Server Error".to_string())
                    .unwrap_or_else(|_| Response::new("Internal Server Error".to_string()))
            },
        ));
        self
    }
}
