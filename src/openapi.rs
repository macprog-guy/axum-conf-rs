//! OpenAPI documentation support via utoipa.
//!
//! This module provides helpers for generating OpenAPI documentation
//! and serving a Scalar UI for API exploration.
//!
//! # Example
//!
//! ```rust,ignore
//! use axum_conf::{Config, FluentRouter};
//! use axum_conf::openapi::{OpenApiBuilder, ScalarConfig};
//! use utoipa::OpenApi;
//!
//! #[derive(OpenApi)]
//! #[openapi(
//!     paths(list_users, get_user),
//!     components(schemas(User))
//! )]
//! struct ApiDoc;
//!
//! let config = Config::default();
//! let router = FluentRouter::without_state(config)?
//!     .merge(ApiDoc::router())
//!     .with_openapi::<ApiDoc>("/docs")
//!     .setup_middleware()
//!     .await?
//!     .start()
//!     .await;
//! ```

use axum::{Router, routing::get};
use utoipa_scalar::{Scalar, Servable};

/// Configuration for the Scalar API documentation UI.
pub struct ScalarConfig {
    /// The route path for the Scalar UI (default: "/docs")
    pub path: String,
    /// The route path for the OpenAPI JSON spec (default: "/openapi.json")
    pub spec_path: String,
    /// Custom title for the documentation page
    pub title: Option<String>,
}

impl Default for ScalarConfig {
    fn default() -> Self {
        Self {
            path: "/docs".to_string(),
            spec_path: "/openapi.json".to_string(),
            title: None,
        }
    }
}

impl ScalarConfig {
    /// Creates a new ScalarConfig with the given UI path.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Default::default()
        }
    }

    /// Sets the OpenAPI spec path.
    pub fn with_spec_path(mut self, path: impl Into<String>) -> Self {
        self.spec_path = path.into();
        self
    }

    /// Sets a custom title for the documentation page.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Extension trait for adding OpenAPI documentation to a router.
pub trait OpenApiExt<S>
where
    S: Clone + Send + Sync + 'static,
{
    /// Adds OpenAPI documentation routes to the router.
    ///
    /// This method adds:
    /// - A Scalar UI at the configured path (default: `/docs`)
    /// - An OpenAPI JSON spec at the configured spec path (default: `/openapi.json`)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use axum_conf::openapi::OpenApiExt;
    /// use utoipa::OpenApi;
    ///
    /// #[derive(OpenApi)]
    /// #[openapi(paths(), components())]
    /// struct ApiDoc;
    ///
    /// let router = Router::new()
    ///     .with_openapi::<ApiDoc>("/docs");
    /// ```
    fn with_openapi<A: utoipa::OpenApi>(self, path: &str) -> Self;

    /// Adds OpenAPI documentation with custom configuration.
    fn with_openapi_config<A: utoipa::OpenApi>(self, config: ScalarConfig) -> Self;
}

impl<S> OpenApiExt<S> for Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    fn with_openapi<A: utoipa::OpenApi>(self, path: &str) -> Self {
        let config = ScalarConfig::new(path);
        self.with_openapi_config::<A>(config)
    }

    fn with_openapi_config<A: utoipa::OpenApi>(self, config: ScalarConfig) -> Self {
        let spec = A::openapi();
        let spec_json = spec.to_json().expect("Failed to serialize OpenAPI spec");

        // Leak the path string to get a static lifetime for Scalar
        let docs_path: &'static str = Box::leak(config.path.into_boxed_str());
        let spec_path: &'static str = Box::leak(config.spec_path.into_boxed_str());

        self.route(spec_path, get(move || async move { spec_json.clone() }))
            .merge(Scalar::with_url(docs_path, A::openapi()))
    }
}

/// Re-export common utoipa types for convenience.
pub use utoipa::{
    OpenApi, ToResponse, ToSchema,
    openapi::{Contact, Info, License},
};

/// Helper to create OpenAPI info with common defaults.
pub fn info(title: impl Into<String>, version: impl Into<String>) -> Info {
    Info::builder().title(title).version(version).build()
}

/// Helper to create OpenAPI info with full metadata.
pub fn info_full(
    title: impl Into<String>,
    version: impl Into<String>,
    description: impl Into<String>,
) -> Info {
    Info::builder()
        .title(title)
        .version(version)
        .description(Some(description.into()))
        .build()
}
