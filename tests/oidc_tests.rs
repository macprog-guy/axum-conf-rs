#![cfg(feature = "keycloak")]

use axum::{
    Extension, Router,
    body::Body,
    http::{Request, StatusCode, header},
    routing::get,
};
use axum_conf::Config;
use axum_keycloak_auth::decode::{KeycloakToken, ProfileAndEmail};
use tower::ServiceExt;

#[cfg(feature = "postgres")]
use {
    testcontainers::{ImageExt, runners::AsyncRunner},
    testcontainers_modules::postgres::Postgres,
};

mod keycloak;
use keycloak::KeycloakContainer;

/// Helper function to create a test configuration with OIDC enabled
fn create_oidc_config(issuer_url: &str, realm: &str) -> Config {
    // KeycloakConfig will append /realms/{realm} automatically, so pass base URL
    let toml_str = format!(
        r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
support_compression = false
trim_trailing_slash = true
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.oidc]
issuer_url = "{}"
realm = "{}"
audiences = ["account"]
client_id = "test-client"
client_secret = "test-secret"

[logging]
format = "json"
        "#,
        issuer_url, realm
    );

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    // Disable metrics to avoid Prometheus registry conflicts in tests
    config.http.with_metrics = false;
    config
}

/// Helper function to create a test router with OIDC enabled
async fn create_oidc_test_router(mut config: Config) -> Router {
    use axum_conf::FluentRouter;
    use axum_conf::{HttpMiddleware, HttpMiddlewareConfig};

    // Disable rate limiting for tests (oneshot() doesn't provide ConnectInfo<SocketAddr>)
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(
            Router::new()
                .route("/protected", get(|| async { "Protected resource" }))
                .route("/public", get(|| async { "Public resource" })),
        )
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    // Give the OIDC discovery a moment to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    app
}

#[tokio::test]
async fn test_oidc_integration() {
    // Start Keycloak container and setup test realm/client/user
    let keycloak = KeycloakContainer::start().await;
    keycloak.create_test_user().await;

    #[cfg(feature = "postgres")]
    let pg_server = Postgres::default()
        .with_tag("16.4")
        .start()
        .await
        .expect("Could not start postgres server");

    // Create config with Keycloak issuer URL using the test-realm
    let mut config = create_oidc_config(keycloak.url.as_str(), "test-realm");

    #[cfg(feature = "postgres")]
    {
        // Build the DATABASE_URL from the image host and random port.
        let database_url = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            pg_server.get_host().await.unwrap(),
            pg_server.get_host_port_ipv4(5432).await.unwrap()
        );

        config.database.url = database_url;
    }

    // Create router with OIDC enabled
    let app = create_oidc_test_router(config).await;

    // ------------------------------------------------------------------------
    // PROTECTED
    // ------------------------------------------------------------------------

    // Try to access protected endpoint without auth token
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should be blocked (401 Unauthorized or 403 Forbidden)
    assert!(
        response.status() == StatusCode::UNAUTHORIZED || response.status() == StatusCode::FORBIDDEN,
        "Expected 401 or 403, got {}",
        response.status()
    );

    // ------------------------------------------------------------------------
    // PUBLIC / HEALTH / READINESS
    // ------------------------------------------------------------------------

    // Health endpoints should be accessible without auth
    let health_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(health_response.status(), StatusCode::OK);

    // Readiness endpoint should also be accessible
    let ready_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(ready_response.status(), StatusCode::OK);

    // ------------------------------------------------------------------------
    // INVALID AND VALID TOKENS
    // ------------------------------------------------------------------------

    // First, test with invalid token - should be rejected
    let invalid_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(header::AUTHORIZATION, "Bearer invalid-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should reject invalid token with 400, 401, or 403
    assert!(
        invalid_response.status() == StatusCode::BAD_REQUEST
            || invalid_response.status() == StatusCode::UNAUTHORIZED
            || invalid_response.status() == StatusCode::FORBIDDEN,
        "Expected 400, 401 or 403 for invalid token, got {}",
        invalid_response.status()
    );

    // ------------------------------------------------------------------------
    // VALID TOKEN
    // ------------------------------------------------------------------------

    let valid_token = keycloak
        .perform_password_login(
            "test-user-mail@foo.bar",
            "password",
            "test-realm",
            "test-client",
        )
        .await;

    // Test with valid token - should be accepted (or at least not rejected for auth reasons)
    let valid_response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header(header::AUTHORIZATION, format!("Bearer {}", valid_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // With a valid token, we should get 200 OK (or possibly 404 if route doesn't exist in real impl)
    // At minimum, it should NOT be 401/403 authorization errors
    assert!(
        valid_response.status() == StatusCode::OK
            || valid_response.status() == StatusCode::NOT_FOUND,
        "Expected 200 or 404 with valid token, got {}",
        valid_response.status()
    );

    // ------------------------------------------------------------------------
    // NO CONFIGURED OIDC
    // ------------------------------------------------------------------------

    // Create config without OIDC section
    let toml_str = r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
support_compression = false
trim_trailing_slash = true
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[logging]
format = "json"
    "#;

    let mut config: Config = toml_str.parse().expect("Failed to parse config");

    // Verify OIDC is None
    assert!(config.http.oidc.is_none());

    // Disable rate limiting for tests (oneshot() doesn't provide ConnectInfo<SocketAddr>)
    use axum_conf::{HttpMiddleware, HttpMiddlewareConfig};
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    // Should successfully setup middleware without OIDC
    use axum_conf::FluentRouter;

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(Router::new().route("/test", get(|| async { "Test endpoint" })))
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    // Should be able to access endpoints without auth
    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let status = response.status();
    if status != StatusCode::OK {
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body_bytes);
        eprintln!("ERROR: Status={}, Body={}", status, body_str);
    }

    assert_eq!(status, StatusCode::OK);
}

/// Handler that extracts and returns the authenticated username from KeycloakToken
async fn whoami_handler(
    Extension(token): Extension<KeycloakToken<String, ProfileAndEmail>>,
) -> String {
    // Prefer preferred_username, fall back to subject
    let username = &token.extra.profile.preferred_username;
    if !username.is_empty() {
        username.clone()
    } else {
        token.subject.clone()
    }
}

#[tokio::test]
async fn test_oidc_user_span_integration() {
    use axum_conf::{FluentRouter, HttpMiddleware, HttpMiddlewareConfig};

    // Start Keycloak container and setup test realm/client/user
    let keycloak = KeycloakContainer::start().await;
    keycloak.create_test_user().await;

    #[cfg(feature = "postgres")]
    let pg_server = Postgres::default()
        .with_tag("16.4")
        .start()
        .await
        .expect("Could not start postgres server");

    // Create config with Keycloak issuer URL
    let mut config = create_oidc_config(keycloak.url.as_str(), "test-realm");

    #[cfg(feature = "postgres")]
    {
        let database_url = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            pg_server.get_host().await.unwrap(),
            pg_server.get_host_port_ipv4(5432).await.unwrap()
        );
        config.database.url = database_url;
    }

    // Disable metrics and rate limiting for tests
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));

    // Create router with a /whoami endpoint that returns the authenticated username
    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(Router::new().route("/whoami", get(whoami_handler)))
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    // Give OIDC discovery time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Get a valid token for the test user
    let valid_token = keycloak
        .perform_password_login(
            "test-user-mail@foo.bar",
            "password",
            "test-realm",
            "test-client",
        )
        .await;

    // Call /whoami with valid token - should return the username
    let response = app
        .oneshot(
            Request::builder()
                .uri("/whoami")
                .header(header::AUTHORIZATION, format!("Bearer {}", valid_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK with valid token"
    );

    // Extract and verify the username from response body
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body_bytes);

    // The username should be "test-user" (as configured in KeycloakContainer::create_test_user)
    assert!(
        body_str.contains("test-user"),
        "Expected response to contain 'test-user', got: {}",
        body_str
    );
}
