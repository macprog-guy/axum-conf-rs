//! Feature middleware: routing, compression, CORS, security headers, sessions, health checks.

use super::router::FluentRouter;
use crate::HttpMiddleware;

use {
    axum::routing::get,
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
use axum_helmet::{Helmet, HelmetLayer};

#[cfg(feature = "session")]
use tower_sessions::{
    Expiry, MemoryStore, SessionManagerLayer,
    cookie::{SameSite, time::Duration as CookieDuration},
};

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
    /// Sets up cookie-based session handling using an in-memory store.
    #[cfg(feature = "session")]
    #[must_use]
    pub fn setup_session_handling(mut self) -> Self {
        let session_store = MemoryStore::default();
        let session_layer = SessionManagerLayer::new(session_store)
            .with_secure(false)
            .with_same_site(SameSite::Lax)
            .with_expiry(Expiry::OnInactivity(CookieDuration::seconds(3600)));
        self.inner = self.inner.layer(session_layer);
        self
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
            return self;
        }

        self.inner = self.inner.layer(NormalizePathLayer::trim_trailing_slash());
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
            return self;
        }

        if let Some(timeout) = self.config.http.request_timeout {
            self.inner = self.inner.layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                timeout,
            ));
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
    /// FluentRouter::without_state(Config::default())?
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
            return self;
        }

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
            return self;
        }

        let mut helmet = Helmet::new();
        if self.config.http.x_content_type_nosniff {
            helmet = helmet.add(helmet_core::XContentTypeOptions::nosniff());
        }
        // Convert our local XFrameOptions to axum_helmet's version
        let x_frame = match &self.config.http.x_frame_options.0 {
            crate::XFrameOptions::Deny => axum_helmet::XFrameOptions::Deny,
            crate::XFrameOptions::SameOrigin => axum_helmet::XFrameOptions::SameOrigin,
            crate::XFrameOptions::AllowFrom(url) => {
                axum_helmet::XFrameOptions::AllowFrom(url.clone())
            }
        };
        helmet = helmet.add(x_frame);
        self.inner = self.inner.layer(HelmetLayer::new(helmet));
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
    /// let mut config = Config::default();
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
            return self;
        }

        use http::HeaderValue;

        if let Some(cors_config) = &self.config.http.cors {
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
        } else {
            // No CORS config specified - behavior depends on environment
            let rust_env = std::env::var("RUST_ENV").unwrap_or_default().to_lowercase();
            let is_production = rust_env.is_empty()
                || rust_env == "prod"
                || rust_env == "production"
                || rust_env == "release";

            if is_production {
                // Production: fail-safe to restrictive CORS (same-origin only)
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
                    "No CORS configuration found (RUST_ENV={}). Using permissive defaults. \
                     This is NOT safe for production - configure explicit CORS rules.",
                    rust_env
                );
                self.inner = self.inner.layer(CorsLayer::very_permissive());
            }
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

    /// Sets up Kubernetes health check endpoints.
    ///
    /// Adds two endpoints for Kubernetes probes:
    /// - **Liveness probe** - Always returns 200 OK (indicates process is running)
    /// - **Readiness probe** - Returns 200 OK if service can handle traffic, including database connectivity check
    ///
    /// When the `postgres` feature is enabled and a database is configured in the config file,
    /// the readiness endpoint will verify database connectivity by executing a simple query.
    /// If the database is unreachable, returns 503 Service Unavailable.
    ///
    /// # Configuration
    ///
    /// ```toml
    /// [http]
    /// liveness_route = "/live"   # Default
    /// readiness_route = "/ready" # Default
    /// ```
    ///
    /// # Kubernetes Integration
    ///
    /// ```yaml
    /// livenessProbe:
    ///   httpGet:
    ///     path: /live
    ///     port: 3000
    /// readinessProbe:
    ///   httpGet:
    ///     path: /ready
    ///     port: 3000
    /// ```
    #[must_use]
    pub fn setup_liveness_readiness(mut self) -> Self {
        let liveness_enabled = self.is_middleware_enabled(HttpMiddleware::Liveness);
        let readiness_enabled = self.is_middleware_enabled(HttpMiddleware::Readiness);

        if !liveness_enabled && !readiness_enabled {
            return self;
        }

        let liveness_route = self.config.http.liveness_route.clone();
        let readiness_route = self.config.http.readiness_route.clone();

        #[cfg(feature = "postgres")]
        let db_pool = self.db_pool.clone();

        // Add liveness endpoint if enabled
        if liveness_enabled {
            self.inner = self.inner.route(&liveness_route, get(|| async { "OK\n" }));
        }

        // Add readiness endpoint if enabled
        if readiness_enabled {
            self.inner = self.inner.route(
                &readiness_route,
                get(|| async move {
                    #[cfg(feature = "postgres")]
                    match sqlx::query("SELECT 1").execute(&db_pool).await {
                        Ok(_) => (StatusCode::OK, "OK\n"),
                        Err(e) => {
                            tracing::error!("Database health check failed: {}", e);
                            (StatusCode::SERVICE_UNAVAILABLE, "Database unavailable\n")
                        }
                    }

                    #[cfg(not(feature = "postgres"))]
                    (StatusCode::OK, "OK\n")
                }),
            );
        }
        self
    }
}
