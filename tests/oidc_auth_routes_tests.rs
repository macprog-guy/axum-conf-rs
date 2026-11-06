//! `AuthRoutes` extractor and GET/POST logout, against an in-process stub IdP
//! (no Docker).
#![cfg(feature = "keycloak")]

use axum::{Json, Router, body::Body, http::Request, routing::get};
use axum_conf::{AuthRoutes, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig};
use tower::ServiceExt;

/// Serves a minimal discovery document and empty JWKS. Returns the base URL.
async fn spawn_stub_idp() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let doc = serde_json::json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "jwks_uri": format!("{base}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "end_session_endpoint": format!("{base}/signoff")
    });
    let app = Router::new()
        .route(
            "/jwks",
            get(|| async { Json(serde_json::json!({"keys": []})) }),
        )
        .route(
            "/.well-known/openid-configuration",
            get(move || {
                let doc = doc.clone();
                async move { Json(doc) }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn config(issuer: &str, extra_oidc: &str) -> Config {
    let toml_str = format!(
        r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_payload_size_bytes = "1KiB"

[http.oidc]
issuer_url = "{issuer}"
audiences = ["account"]
client_id = "test-client"
client_secret = "test-secret"
redirect_uri = "http://127.0.0.1:3000/auth/callback"
{extra_oidc}
"#
    );
    let mut config: Config = toml_str.parse().expect("test config TOML");
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

async fn app(config: Config) -> Router {
    FluentRouter::without_state(config)
        .unwrap()
        .route(
            "/routes",
            get(|routes: AuthRoutes| async move { format!("{} {}", routes.login, routes.logout) }),
        )
        .route(
            "/maybe-routes",
            get(|routes: Option<AuthRoutes>| async move {
                routes.map_or("none".to_string(), |r| r.logout)
            }),
        )
        .setup_middleware()
        .await
        .expect("middleware sets up")
        .into_inner()
}

async fn send(app: &Router, method: &str, uri: &str) -> (u16, String, Option<String>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let location = response
        .headers()
        .get("location")
        .map(|v| v.to_str().unwrap().to_string());
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        String::from_utf8_lossy(&body).into_owned(),
        location,
    )
}

#[tokio::test]
async fn auth_routes_extractor_returns_defaults() {
    let issuer = spawn_stub_idp().await;
    let app = app(config(&issuer, "")).await;

    let (status, body, _) = send(&app, "GET", "/routes").await;
    assert_eq!(status, 200);
    assert_eq!(body, "/auth/login /auth/logout");

    let (_, body, _) = send(&app, "GET", "/maybe-routes").await;
    assert_eq!(body, "/auth/logout");
}

#[tokio::test]
async fn auth_routes_extractor_reflects_custom_routes() {
    let issuer = spawn_stub_idp().await;
    let app = app(config(
        &issuer,
        "login_route = \"/sso/in\"\nlogout_route = \"/sso/out\"",
    ))
    .await;

    let (_, body, _) = send(&app, "GET", "/routes").await;
    assert_eq!(body, "/sso/in /sso/out");
}

#[tokio::test]
async fn logout_accepts_get_and_post_by_default() {
    let issuer = spawn_stub_idp().await;
    let app = app(config(&issuer, "")).await;

    for method in ["GET", "POST"] {
        let (status, _, location) = send(&app, method, "/auth/logout").await;
        assert_eq!(status, 307, "{method} logout should redirect");
        assert!(
            location.as_deref().is_some_and(|l| l.contains("/signoff")),
            "{method} logout should go to the end-session endpoint: {location:?}"
        );
    }
}

#[tokio::test]
async fn logout_disable_get_leaves_only_post() {
    let issuer = spawn_stub_idp().await;
    let app = app(config(&issuer, "logout_disable_get = true")).await;

    let (status, _, _) = send(&app, "GET", "/auth/logout").await;
    assert_eq!(status, 405, "GET logout must be rejected");

    let (status, _, _) = send(&app, "POST", "/auth/logout").await;
    assert_eq!(status, 307, "POST logout must still work");
}

#[tokio::test]
async fn auth_routes_extractor_without_auth_code_flow() {
    use axum::extract::{FromRequestParts, OptionalFromRequestParts};

    // No flow routes were registered, so nothing put AuthRoutes in the request.
    let (mut parts, _) = Request::builder().body(()).unwrap().into_parts();

    let rejection = <AuthRoutes as FromRequestParts<()>>::from_request_parts(&mut parts, &())
        .await
        .unwrap_err();
    assert_eq!(rejection.0, 404);

    let optional =
        <AuthRoutes as OptionalFromRequestParts<()>>::from_request_parts(&mut parts, &())
            .await
            .unwrap();
    assert_eq!(optional, None);
}
