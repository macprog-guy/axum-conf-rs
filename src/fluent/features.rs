//! Feature middleware: routing, compression, CORS, security headers, sessions, health checks.

use super::router::FluentRouter;
use crate::HttpMiddleware;

use {
    axum::{response::IntoResponse, routing::get},
    http::StatusCode,
    tower_http::timeout::TimeoutLayer,
};

#[cfg(feature = "path-normalization")]
use tower_http::normalize_path::NormalizePathLayer;

#[cfg(feature = "compression")]
use tower_http::{compression::CompressionLayer, decompression::RequestDecompressionLayer};

#[cfg(feature = "cors")]
use {http::HeaderName, tower_http::cors::CorsLayer};

#[cfg(feature = "security-headers")]
use axum_helmet::Helmet;

#[cfg(feature = "session")]
use tower_sessions::{
    Expiry, MemoryStore, SessionManagerLayer,
    cookie::{SameSite, time::Duration as CookieDuration},
};

#[cfg(feature = "session")]
impl From<crate::config::SameSiteConfig> for SameSite {
    fn from(value: crate::config::SameSiteConfig) -> Self {
        use crate::config::SameSiteConfig;
        match value {
            SameSiteConfig::Strict => SameSite::Strict,
            SameSiteConfig::Lax => SameSite::Lax,
            SameSiteConfig::None => SameSite::None,
        }
    }
}

#[cfg(feature = "api-versioning")]
use {
    crate::utils::ApiVersion,
    axum::{body::Body, middleware::Next},
    http::Request,
};

impl<State> FluentRouter<State>
where
    State: Clone + Send + Sync + 'static,
{
    /// Sets up cookie-based session handling using the configured store.
    ///
    /// The backend is selected by `config.http.session_store`
    /// ([`SessionStoreConfig`](crate::SessionStoreConfig)): in-memory (default),
    /// PostgreSQL (`session-postgres` feature, reusing the configured pool), or
    /// Redis (`session-redis` feature). For an arbitrary `tower_sessions`
    /// backend, use [`Self::with_session_store`] instead.
    ///
    /// The `Secure` and `SameSite` cookie attributes are taken from
    /// `config.http.session_secure_cookie` (default `true`) and
    /// `config.http.session_same_site` (default `Strict`).
    ///
    /// # Errors
    ///
    /// Returns an error if the chosen backend cannot be initialized (e.g. the
    /// Postgres table migration fails, or the Redis connection cannot be
    /// established).
    ///
    /// # Security note
    ///
    /// Sessions store the OIDC PKCE verifier, CSRF state, nonce, and tokens.
    /// Identity is rebuilt from the stored ID token's claims on each request
    /// *without* re-verifying its signature (it was verified at callback time),
    /// so an external store (Postgres/Redis) must be trusted for integrity since
    /// stored claims are taken at face value.
    #[cfg(feature = "session")]
    pub async fn setup_session_handling(mut self) -> crate::Result<Self> {
        let secure = self.config.http.session_secure_cookie;
        let same_site: SameSite = self.config.http.session_same_site.into();

        if !secure && !self.config.http.bind_addr_is_loopback() {
            tracing::warn!(
                bind_addr = %self.config.http.bind_addr,
                "session_secure_cookie = false while not bound to loopback: session \
                 cookies may be sent over unencrypted connections. Set \
                 session_secure_cookie = true for any non-local deployment."
            );
        }

        match &self.config.http.session_store {
            crate::SessionStoreConfig::Memory => {
                tracing::trace!(
                    secure,
                    ?same_site,
                    "Session middleware enabled (memory store)"
                );
                self = self.apply_session_layer(MemoryStore::default(), secure, same_site);
            }
            #[cfg(feature = "session-postgres")]
            crate::SessionStoreConfig::Postgres => {
                use super::session_store::PostgresSessionStore;
                let store = PostgresSessionStore::new(self.db_pool.clone());
                store.migrate().await.map_err(|e| {
                    crate::Error::database(format!("session store migration failed: {e}"))
                })?;
                // Periodically purge expired rows; aborts on drop with the router.
                let cleanup = store.clone();
                self.session_cleanup_handle = Some(tokio_util::task::AbortOnDropHandle::new(
                    tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_secs(60));
                        interval.tick().await; // first tick is immediate; skip
                        loop {
                            interval.tick().await;
                            if let Err(e) = cleanup.delete_expired().await {
                                tracing::warn!(error = %e, "session expiry cleanup failed");
                            }
                        }
                    }),
                ));
                tracing::trace!(
                    secure,
                    ?same_site,
                    "Session middleware enabled (postgres store)"
                );
                self = self.apply_session_layer(store, secure, same_site);
            }
            #[cfg(feature = "session-redis")]
            crate::SessionStoreConfig::Redis { url } => {
                use super::session_store::RedisSessionStore;
                use fred::prelude::*;
                let config = Config::from_url(url)
                    .map_err(|e| crate::Error::config(format!("invalid redis url: {e}")))?;
                let pool = Pool::new(config, None, None, None, 6)
                    .map_err(|e| crate::Error::config(format!("redis pool init failed: {e}")))?;
                pool.connect();
                pool.wait_for_connect()
                    .await
                    .map_err(|e| crate::Error::config(format!("redis connect failed: {e}")))?;
                tracing::trace!(
                    secure,
                    ?same_site,
                    "Session middleware enabled (redis store)"
                );
                self = self.apply_session_layer(RedisSessionStore::new(pool), secure, same_site);
            }
        }
        Ok(self)
    }

    /// Applies the session-manager layer for the given store, honoring the
    /// configured cookie attributes.
    #[cfg(feature = "session")]
    fn apply_session_layer<St>(mut self, store: St, secure: bool, same_site: SameSite) -> Self
    where
        St: tower_sessions::SessionStore + Clone,
    {
        let layer = SessionManagerLayer::new(store)
            .with_secure(secure)
            .with_same_site(same_site)
            .with_expiry(Expiry::OnInactivity(CookieDuration::seconds(3600)));
        self.inner = self.inner.layer(layer);
        self
    }

    /// Installs session handling backed by a caller-supplied `tower_sessions`
    /// store, bypassing [`SessionStoreConfig`](crate::SessionStoreConfig).
    ///
    /// Use this for backends the library does not provide built-in (e.g. a
    /// Moka, DynamoDB, or custom store). Cookie attributes still come from the
    /// `session_*` config fields.
    ///
    /// ```rust,ignore
    /// let router = FluentRouter::without_state(config)?
    ///     .with_session_store(my_store);
    /// ```
    #[cfg(feature = "session")]
    #[must_use]
    pub fn with_session_store<St>(self, store: St) -> Self
    where
        St: tower_sessions::SessionStore + Clone,
    {
        let secure = self.config.http.session_secure_cookie;
        let same_site: SameSite = self.config.http.session_same_site.into();
        self.apply_session_layer(store, secure, same_site)
    }

    /// Sets up path normalization middleware.
    ///
    /// When `config.http.trim_trailing_slash` is true, automatically removes
    /// trailing slashes from request paths:
    /// - `/api/users/` → `/api/users`
    /// - `/health/` → `/health`
    ///
    /// This ensures consistent routing behavior regardless of whether clients
    /// include trailing slashes.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// trim_trailing_slash = true  # Default
    /// ```
    #[cfg(feature = "path-normalization")]
    #[must_use]
    pub fn setup_path_normalization(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::PathNormalization) {
            tracing::trace!("PathNormalization middleware skipped (disabled in config)");
            return self;
        }

        self.inner = self.inner.layer(NormalizePathLayer::trim_trailing_slash());
        tracing::trace!("PathNormalization middleware enabled");
        self
    }

    /// No-op when `path-normalization` feature is disabled.
    #[cfg(not(feature = "path-normalization"))]
    #[must_use]
    pub fn setup_path_normalization(self) -> Self {
        self
    }

    /// Sets up request timeout middleware.
    ///
    /// Aborts requests that take longer than the configured duration with a
    /// `408 Request Timeout` response.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// request_timeout = "30s"  # Optional, uses humantime format
    /// ```
    ///
    /// # Use Cases
    ///
    /// - Prevent slow requests from tying up resources
    /// - Ensure predictable response times
    /// - Protect against slowloris attacks
    #[must_use]
    pub fn setup_timeout(mut self) -> Self {
        // Skip if timeout middleware is disabled
        if !self.is_middleware_enabled(HttpMiddleware::Timeout) {
            tracing::trace!("Timeout middleware skipped (disabled in config)");
            return self;
        }

        if let Some(timeout) = self.config.http.request_timeout {
            self.inner = self.inner.layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                timeout,
            ));
            tracing::trace!(timeout = ?timeout, "Timeout middleware enabled");
        }
        self
    }

    /// Sets up API versioning middleware.
    ///
    /// Automatically extracts API version from requests and adds it to request extensions.
    /// Supports multiple version detection methods:
    /// - **Path-based**: `/v1/users`, `/api/v2/users`
    /// - **Header-based**: `X-API-Version: 2`, `Accept: application/vnd.api+json;version=2`
    /// - **Query parameter**: `/users?version=1`
    ///
    /// The version is checked in order: path → header → query → default.
    ///
    /// **Note**: This middleware is automatically included in `setup_middleware()` using
    /// the `config.http.default_api_version` setting. You only need to call this method
    /// directly if you want to override the configured default or set up versioning manually.
    ///
    /// # Arguments
    ///
    /// * `default_version` - The version to use when none is specified in the request
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use axum_conf::{Config, FluentRouter, ApiVersion};
    /// use axum::{routing::get, extract::Extension};
    ///
    /// async fn handler(Extension(version): Extension<ApiVersion>) -> String {
    ///     format!("API version: {}", version)
    /// }
    ///
    /// # async fn example() -> axum_conf::Result<()> {
    /// FluentRouter::without_state(Config::<()>::default())?
    ///     .setup_api_versioning(1)  // Default to v1
    ///     .route("/users", get(handler));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Handler Usage
    ///
    /// Extract the version in your handlers using `Extension<ApiVersion>`:
    ///
    /// ```rust,ignore
    /// use axum::extract::Extension;
    /// use axum_conf::ApiVersion;
    ///
    /// async fn my_handler(Extension(version): Extension<ApiVersion>) -> String {
    ///     match version.as_u32() {
    ///         1 => handle_v1(),
    ///         2 => handle_v2(),
    ///         _ => "Unsupported version".to_string(),
    ///     }
    /// }
    /// ```
    #[cfg(feature = "api-versioning")]
    #[must_use]
    pub fn setup_api_versioning(mut self, default_version: u32) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::ApiVersioning) {
            tracing::trace!("ApiVersioning middleware skipped (disabled in config)");
            return self;
        }

        tracing::trace!(
            default_version = default_version,
            "ApiVersioning middleware enabled"
        );
        use axum::middleware;

        let default_version = ApiVersion::new(default_version);

        self.inner = self.inner.layer(middleware::from_fn(
            move |mut req: Request<Body>, next: Next| async move {
                // Try to extract version from path first
                let version = ApiVersion::from_path(req.uri().path())
                    // Then try X-API-Version header
                    .or_else(|| {
                        req.headers()
                            .get("x-api-version")
                            .and_then(|h| h.to_str().ok())
                            .and_then(ApiVersion::from_header)
                    })
                    // Then try Accept header
                    .or_else(|| {
                        req.headers()
                            .get(http::header::ACCEPT)
                            .and_then(|h| h.to_str().ok())
                            .and_then(ApiVersion::from_header)
                    })
                    // Then try query parameter
                    .or_else(|| req.uri().query().and_then(ApiVersion::from_query))
                    // Fall back to default
                    .unwrap_or(default_version);

                // Add version to request extensions
                req.extensions_mut().insert(version);

                // Log the version being used
                tracing::debug!(
                    version = %version,
                    path = %req.uri().path(),
                    "API version detected"
                );

                next.run(req).await
            },
        ));
        self
    }

    /// No-op when `api-versioning` feature is disabled.
    #[cfg(not(feature = "api-versioning"))]
    #[must_use]
    pub fn setup_api_versioning(self, _default_version: u32) -> Self {
        self
    }

    /// Sets up security headers using Helmet.
    ///
    /// Adds HTTP security headers based on configuration:
    /// - `X-Content-Type-Options: nosniff` (prevents MIME sniffing)
    /// - `X-Frame-Options` (clickjacking protection)
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// x_content_type_nosniff = true  # Default
    /// x_frame_options = "DENY"       # Default: DENY, SAMEORIGIN, or URL
    /// ```
    ///
    /// # Security Benefits
    ///
    /// - Prevents browsers from MIME-sniffing responses
    /// - Protects against clickjacking attacks
    /// - Improves security score in penetration tests
    #[cfg(feature = "security-headers")]
    #[must_use]
    pub fn setup_helmet(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::SecurityHeaders) {
            tracing::trace!("SecurityHeaders middleware skipped (disabled in config)");
            return self;
        }

        tracing::trace!(
            x_frame_options = ?self.config.http.x_frame_options,
            x_content_type_nosniff = self.config.http.x_content_type_nosniff,
            "SecurityHeaders middleware enabled"
        );
        let mut helmet = Helmet::new();
        if self.config.http.x_content_type_nosniff {
            helmet = helmet.add(helmet_core::XContentTypeOptions::nosniff());
        }
        // Convert our local XFrameOptions to axum_helmet's version
        let x_frame = match &self.config.http.x_frame_options.0 {
            crate::XFrameOptions::Deny => axum_helmet::XFrameOptions::Deny,
            crate::XFrameOptions::SameOrigin => axum_helmet::XFrameOptions::SameOrigin,
            #[allow(deprecated)]
            crate::XFrameOptions::AllowFrom(url) => {
                axum_helmet::XFrameOptions::AllowFrom(url.clone())
            }
        };
        helmet = helmet.add(x_frame);
        match helmet.into_layer() {
            Ok(layer) => self.inner = self.inner.layer(layer),
            Err(e) => tracing::warn!(error = %e, "Failed to build HelmetLayer"),
        }

        // Optional HSTS — only meaningful over HTTPS, hence opt-in via config.
        if let Some(max_age) = self.config.http.hsts_max_age {
            let value = if self.config.http.hsts_include_subdomains {
                format!("max-age={max_age}; includeSubDomains")
            } else {
                format!("max-age={max_age}")
            };
            if let Ok(header_value) = http::HeaderValue::from_str(&value) {
                self.inner =
                    self.inner
                        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                            http::header::STRICT_TRANSPORT_SECURITY,
                            header_value,
                        ));
            }
        }

        // Optional Content-Security-Policy, emitted verbatim when configured.
        if let Some(csp) = &self.config.http.content_security_policy
            && let Ok(header_value) = http::HeaderValue::from_str(csp)
        {
            self.inner =
                self.inner
                    .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
                        http::header::CONTENT_SECURITY_POLICY,
                        header_value,
                    ));
        }
        self
    }

    /// No-op when `security-headers` feature is disabled.
    #[cfg(not(feature = "security-headers"))]
    #[must_use]
    pub fn setup_helmet(self) -> Self {
        self
    }

    /// Sets up request decompression and response compression.
    ///
    /// When `config.http.support_compression` is true, enables:
    /// - Request body decompression (gzip, brotli, deflate, zstd)
    /// - Response body compression (based on Accept-Encoding header)
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// support_compression = true  # Default: false
    /// ```
    ///
    /// # Performance
    ///
    /// - Reduces bandwidth usage
    /// - May increase CPU usage
    /// - Most beneficial for text-based responses (JSON, HTML, etc.)
    #[cfg(feature = "compression")]
    #[must_use]
    pub fn setup_compression(mut self) -> Self {
        if self.config.http.support_compression
            && self.is_middleware_enabled(HttpMiddleware::Compression)
        {
            self.inner = self
                .inner
                .layer(RequestDecompressionLayer::new())
                .layer(CompressionLayer::new());
            tracing::trace!("Compression middleware enabled");
        } else {
            tracing::trace!("Compression middleware skipped (disabled in config)");
        }
        self
    }

    /// No-op when `compression` feature is disabled.
    #[cfg(not(feature = "compression"))]
    #[must_use]
    pub fn setup_compression(self) -> Self {
        if self.config.http.support_compression {
            tracing::warn!(
                "Compression is enabled in config but the 'compression' feature is not enabled. \
                 Add `compression` to your Cargo.toml features to enable compression support."
            );
        }
        self
    }

    /// Sets up Cross-Origin Resource Sharing (CORS) middleware.
    ///
    /// Configures which web domains can make requests to your API from a browser.
    /// If no CORS configuration is provided, defaults to very permissive settings
    /// (allows all origins, methods, and headers).
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http.cors]
    /// allow_credentials = true
    /// allowed_origins = ["https://app.example.com", "https://admin.example.com"]
    /// allowed_methods = ["GET", "POST", "PUT", "DELETE"]
    /// allowed_headers = ["content-type", "authorization"]
    /// exposed_headers = ["x-request-id"]
    /// max_age = "1h"
    /// ```
    ///
    /// # Security Considerations
    ///
    /// - When `allow_credentials` is `true`, wildcard origins are not allowed
    /// - Without explicit configuration, uses `CorsLayer::very_permissive()` which is suitable
    ///   for development but may be too permissive for production
    /// - Always configure explicit `allowed_origins` in production environments
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use axum_conf::{Config, HttpCorsConfig};
    /// # fn example() -> axum_conf::Result<()> {
    /// let mut config: Config = Config::default();
    /// config.http.cors = Some(HttpCorsConfig {
    ///     allow_credentials: Some(true),
    ///     allowed_origins: Some(vec!["https://app.example.com".to_string()]),
    ///     allowed_methods: None,
    ///     allowed_headers: None,
    ///     exposed_headers: None,
    ///     max_age: None,
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "cors")]
    #[must_use]
    pub fn setup_cors(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::Cors) {
            tracing::trace!("CORS middleware skipped (disabled in config)");
            return self;
        }

        use http::HeaderValue;

        if let Some(cors_config) = &self.config.http.cors {
            tracing::trace!("CORS middleware enabled with custom configuration");
            let mut cors = CorsLayer::new();

            // By default we do NOT allow credentials
            let has_credentials = cors_config.allow_credentials.unwrap_or(false);

            // Configure allowed origins
            if let Some(origins) = &cors_config.allowed_origins {
                for origin in origins {
                    if let Ok(header_value) = HeaderValue::from_str(origin) {
                        cors = cors.allow_origin(header_value);
                    }
                }
            } else if !has_credentials {
                // Only use wildcard if credentials is not enabled
                cors = cors.allow_origin(tower_http::cors::Any);
            }

            // Configure allowed methods
            if let Some(methods) = &cors_config.allowed_methods {
                let method_list: Vec<http::Method> = methods.iter().map(|m| m.0.clone()).collect();
                cors = cors.allow_methods(method_list);
            } else if !has_credentials {
                // Only use wildcard if credentials is not enabled
                cors = cors.allow_methods(tower_http::cors::Any);
            }

            // Configure allowed headers
            if let Some(headers) = &cors_config.allowed_headers {
                let header_list: Vec<HeaderName> = headers.iter().map(|h| h.0.clone()).collect();
                cors = cors.allow_headers(header_list);
            } else if !has_credentials {
                // Only use wildcard if credentials is not enabled
                cors = cors.allow_headers(tower_http::cors::Any);
            }

            // Configure exposed headers
            if let Some(headers) = &cors_config.exposed_headers {
                let header_list: Vec<HeaderName> = headers.iter().map(|h| h.0.clone()).collect();
                cors = cors.expose_headers(header_list);
            }

            // Configure max age
            if let Some(max_age) = cors_config.max_age {
                cors = cors.max_age(max_age);
            }

            // Configure credentials (must be set last after origins/headers)
            if has_credentials {
                cors = cors.allow_credentials(true);
            }

            self.inner = self.inner.layer(cors);
        } else if self.config.is_production {
            // Production: fail-safe to restrictive CORS (same-origin only).
            // The deployment environment is resolved once at config load
            // (`Config::is_production`); this code never reads `RUST_ENV` itself.
            tracing::warn!(
                "No CORS configuration found in production environment. \
                 Using restrictive same-origin policy. Configure [http.cors] \
                 in your config file to allow cross-origin requests."
            );
            // Default CorsLayer denies all cross-origin requests
            self.inner = self.inner.layer(CorsLayer::new());
        } else {
            // Development/Test: use permissive defaults with warning
            tracing::warn!(
                "No CORS configuration found (non-production environment). Using permissive \
                 defaults. This is NOT safe for production - configure explicit CORS rules."
            );
            self.inner = self.inner.layer(CorsLayer::very_permissive());
        }
        self
    }

    /// No-op when `cors` feature is disabled.
    #[cfg(not(feature = "cors"))]
    #[must_use]
    pub fn setup_cors(self) -> Self {
        if self.config.http.cors.is_some() {
            tracing::warn!(
                "CORS is configured but the 'cors' feature is not enabled. \
                 Add `cors` to your Cargo.toml features to enable CORS support."
            );
        }
        self
    }

    /// Sets up the Kubernetes liveness probe endpoint.
    ///
    /// Adds a simple endpoint that always returns 200 OK to indicate the process is running.
    /// This endpoint is placed very early in the middleware stack (after panic catching) so
    /// it remains accessible even when other middleware fails.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// liveness_route = "/live"   # Default
    /// ```
    ///
    /// # Kubernetes Integration
    ///
    /// ```yaml
    /// livenessProbe:
    ///   httpGet:
    ///     path: /live
    ///     port: 3000
    /// ```
    #[must_use]
    pub fn setup_liveness(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::Liveness) {
            tracing::trace!("Liveness middleware skipped (disabled in config)");
            return self;
        }

        let liveness_route = self.config.http.liveness_route.clone();
        tracing::trace!(route = %liveness_route, "Liveness endpoint enabled");
        self.inner = self.inner.route(&liveness_route, get(|| async { "OK\n" }));
        self
    }

    /// Sets up the Kubernetes readiness probe endpoint.
    ///
    /// Adds an endpoint that returns 200 OK if the service can handle traffic.
    /// When the `postgres` feature is enabled and a database is configured,
    /// this endpoint verifies database connectivity by executing a simple query.
    /// If the database is unreachable, returns 503 Service Unavailable.
    ///
    /// When the `circuit-breaker` feature is also enabled, the endpoint first checks
    /// if the database circuit breaker is open. If the circuit is open, it returns
    /// 503 immediately without attempting a database query, preventing additional
    /// load on a failing database.
    ///
    /// # Application Readiness Hook
    ///
    /// An application can make `/ready` reflect its own state — not just database
    /// connectivity — by registering a closure with
    /// [`with_readiness_check`](Self::with_readiness_check). The hook is **composed
    /// with** the built-in checks: the endpoint reports ready iff the application
    /// check returns [`Readiness::Ready`](crate::Readiness::Ready) *and* the
    /// database / circuit-breaker check passes. A
    /// [`Readiness::NotReady`](crate::Readiness::NotReady) result short-circuits to
    /// `503 Service Unavailable` with the supplied message in the body.
    ///
    /// The application check is evaluated **before** the database check so a
    /// saturated service can shed load without incurring a database round-trip.
    /// When no hook is registered, behavior is unchanged (database-or-`OK`). The
    /// hook is available regardless of the `postgres` feature.
    ///
    /// This endpoint is placed after rate limiting and timeout middleware so that:
    /// - Excessive health check requests don't overwhelm the service
    /// - Database queries have a timeout to prevent hanging probes
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// readiness_route = "/ready" # Default
    /// ```
    ///
    /// # Kubernetes Integration
    ///
    /// ```yaml
    /// readinessProbe:
    ///   httpGet:
    ///     path: /ready
    ///     port: 3000
    /// ```
    #[must_use]
    pub fn setup_readiness(mut self) -> Self {
        if !self.is_middleware_enabled(HttpMiddleware::Readiness) {
            tracing::trace!("Readiness middleware skipped (disabled in config)");
            return self;
        }

        let readiness_route = self.config.http.readiness_route.clone();
        tracing::trace!(route = %readiness_route, "Readiness endpoint enabled");

        // Application-supplied readiness hook, composed with the built-in checks.
        let readiness_check = self.readiness_check.clone();

        #[cfg(feature = "postgres")]
        let db_pool = self.db_pool.clone();

        #[cfg(all(feature = "circuit-breaker", feature = "postgres"))]
        let circuit_breaker_registry = self.circuit_breaker_registry.clone();

        self.inner = self.inner.route(
            &readiness_route,
            get(
                move |axum::extract::State(app_state): axum::extract::State<State>| async move {
                    // 1. Application-supplied readiness check (evaluated first; it is
                    //    cheap and app-specific, so a saturated service sheds load
                    //    without a database round-trip). Composes with the built-in
                    //    checks: ready iff the app check passes AND the DB check passes.
                    if let Some(check) = readiness_check.as_ref()
                        && let crate::Readiness::NotReady(msg) = check(app_state).await
                    {
                        tracing::warn!(reason = %msg, "Application readiness check reported not ready");
                        return (StatusCode::SERVICE_UNAVAILABLE, format!("{msg}\n")).into_response();
                    }

                    // 2. When circuit-breaker and postgres are both enabled,
                    //    check circuit state before querying.
                    #[cfg(all(feature = "circuit-breaker", feature = "postgres"))]
                    {
                        let breaker = circuit_breaker_registry.get_or_default("database");
                        if !breaker.should_allow() {
                            tracing::warn!(
                                "Database circuit breaker is open, skipping health check query"
                            );
                            return (StatusCode::SERVICE_UNAVAILABLE, "Database circuit open\n")
                                .into_response();
                        }
                    }

                    // 3. Database connectivity check.
                    #[cfg(feature = "postgres")]
                    if let Err(e) = sqlx::query("SELECT 1").execute(&db_pool).await {
                        tracing::error!("Database health check failed: {}", e);
                        return (StatusCode::SERVICE_UNAVAILABLE, "Database unavailable\n")
                            .into_response();
                    }

                    (StatusCode::OK, "OK\n").into_response()
                },
            ),
        );
        self
    }

    /// Sets up both Kubernetes health check endpoints.
    ///
    /// This is a convenience method that calls both [`setup_liveness`](Self::setup_liveness)
    /// and [`setup_readiness`](Self::setup_readiness). However, when using `setup_middleware()`,
    /// these endpoints are placed at different positions in the middleware stack for optimal
    /// behavior.
    ///
    /// # Deprecated
    ///
    /// Prefer using `setup_middleware()` which places liveness and readiness endpoints at
    /// their optimal positions in the middleware stack. If you need manual control, use
    /// `setup_liveness()` and `setup_readiness()` separately.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// liveness_route = "/live"   # Default
    /// readiness_route = "/ready" # Default
    /// ```
    #[must_use]
    #[deprecated(
        since = "0.4.0",
        note = "Use setup_middleware() or call setup_liveness() and setup_readiness() separately for optimal middleware ordering"
    )]
    pub fn setup_liveness_readiness(self) -> Self {
        self.setup_liveness().setup_readiness()
    }
}
