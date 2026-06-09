//! Integration tests for Proxy OIDC authentication middleware.

use axum::{Router, routing::get};
use axum_conf::{
    AuthenticatedIdentity, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig,
};
use reqwest::Client;
use std::time::Duration;
use tokio::net::TcpListener;

fn create_proxy_oidc_config() -> Config {
    // The test client connects over loopback, so trust the loopback ranges: this
    // exercises the real peer-IP trust mechanism rather than relying on dev mode.
    proxy_oidc_config_with_trusted(r#"["127.0.0.1/32", "::1/128"]"#)
}

fn proxy_oidc_config_with_trusted(trusted_proxies_toml: &str) -> Config {
    let toml_str = format!(
        r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 0
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.proxy_oidc]
trusted_proxies = {trusted_proxies_toml}

[logging]
format = "json"
    "#
    );

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

async fn whoami_handler(identity: AuthenticatedIdentity) -> String {
    format!(
        "user={} email={} groups={} preferred={}",
        identity.user,
        identity.email.unwrap_or_default(),
        identity.groups.join(","),
        identity.preferred_username.unwrap_or_default(),
    )
}

async fn optional_handler(identity: Option<AuthenticatedIdentity>) -> String {
    match identity {
        Some(id) => format!("Hello, {}!", id.user),
        None => "Hello, anonymous!".to_string(),
    }
}

async fn start_test_server(config: Config) -> (u16, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");

    let port = listener.local_addr().unwrap().port();

    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(
            Router::new()
                .route("/test", get(|| async { "OK" }))
                .route("/whoami", get(whoami_handler))
                .route("/optional", get(optional_handler)),
        )
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();

    let handle = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .expect("Server failed to run");
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    (port, handle)
}

#[tokio::test]
async fn test_proxy_oidc_all_headers() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client
        .get(&url)
        .header("X-Auth-Request-User", "jdoe")
        .header("X-Auth-Request-Email", "jdoe@example.com")
        .header("X-Auth-Request-Groups", "admin,operators")
        .header("X-Auth-Request-Preferred-Username", "johndoe")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(
        body,
        "user=jdoe email=jdoe@example.com groups=admin,operators preferred=johndoe"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_no_headers_passes_through() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/optional", port);

    let response = client.get(&url).send().await.expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "Hello, anonymous!");

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_required_identity_missing_returns_401() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client.get(&url).send().await.expect("Request failed");

    assert_eq!(
        response.status(),
        401,
        "Required identity should return 401 when proxy headers are absent"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_health_endpoints_accessible() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("Failed to create client");

    let health_url = format!("http://127.0.0.1:{}/health", port);
    let response = client
        .get(&health_url)
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        response.status(),
        200,
        "Health endpoint should not require proxy headers"
    );

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_untrusted_source_headers_ignored() {
    // The proxy trusts only 10.0.0.0/8, but the test client connects from
    // loopback — so spoofed identity headers must be ignored.
    let config = proxy_oidc_config_with_trusted(r#"["10.0.0.0/8"]"#);
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();

    // A required-identity route must reject the forged headers with 401.
    let resp = client
        .get(format!("http://127.0.0.1:{port}/whoami"))
        .header("X-Auth-Request-User", "attacker")
        .header("X-Auth-Request-Groups", "admin")
        .send()
        .await
        .expect("Request failed");
    assert_eq!(
        resp.status(),
        401,
        "forged proxy headers from an untrusted peer must not authenticate"
    );

    // An optional-identity route must see the request as anonymous.
    let resp = client
        .get(format!("http://127.0.0.1:{port}/optional"))
        .header("X-Auth-Request-User", "attacker")
        .send()
        .await
        .expect("Request failed");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "Hello, anonymous!");

    server_handle.abort();
}

#[tokio::test]
async fn test_proxy_oidc_user_only() {
    let config = create_proxy_oidc_config();
    let (port, server_handle) = start_test_server(config).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/whoami", port);

    let response = client
        .get(&url)
        .header("X-Auth-Request-User", "jdoe")
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert_eq!(body, "user=jdoe email= groups= preferred=");

    server_handle.abort();
}
