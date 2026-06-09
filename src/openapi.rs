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
        // Generate the spec once and reuse it for both the JSON route and Scalar.
        let spec = A::openapi();
        // Serialization of a derived OpenAPI spec only fails on a programming
        // error, surfaced here as a fail-fast at startup.
        #[allow(clippy::expect_used)]
        let spec_json = spec.to_json().expect("Failed to serialize OpenAPI spec");

        // No `Box::leak`: `Router::route` takes `&str`, and `Scalar::with_url`
        // accepts an owned `String`, so the paths live in the router/handler.
        self.route(
            &config.spec_path,
            get(move || async move { spec_json.clone() }),
        )
        .merge(Scalar::with_url(config.path, spec))
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use utoipa::OpenApi;

    #[derive(OpenApi)]
    #[openapi(info(title = "Test API", version = "1.2.3"))]
    struct ApiDoc;

    #[tokio::test]
    async fn serves_openapi_json_spec() {
        let app: Router = Router::new().with_openapi::<ApiDoc>("/docs");

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).expect("/openapi.json must return valid JSON");

        // A well-formed OpenAPI document carries the version and our info block.
        assert!(json.get("openapi").is_some(), "missing `openapi` field");
        assert_eq!(json["info"]["title"], "Test API");
        assert_eq!(json["info"]["version"], "1.2.3");
    }
}
