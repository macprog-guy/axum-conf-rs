//! Bearer-path OIDC discovery tests against an in-process stub IdP (no Docker).
#![cfg(feature = "keycloak")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Json, Router, routing::get};
use axum_conf::{Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig};

#[derive(Default)]
struct Hits {
    discovery: AtomicUsize,
    jwks: AtomicUsize,
}

/// Serves a discovery document (under the realm path when `realm` is Some) and
/// a JWKS at the deliberately non-Keycloak path `/pf/JWKS`. Returns (base_url, hits).
///
/// Note: tests assert `hits.jwks >= 1` (not `== 1`) because openidconnect's
/// discovery itself fetches `jwks_uri` into the metadata, then `JwksProvider`
/// fetches it again — two startup GETs are expected.
async fn spawn_stub_idp(realm: Option<&str>, serve_discovery: bool) -> (String, Arc<Hits>) {
    let hits = Arc::new(Hits::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());

    let issuer = match realm {
        Some(r) => format!("{base}/realms/{r}"),
        None => base.clone(),
    };
    let discovery_path = match realm {
        Some(r) => format!("/realms/{r}/.well-known/openid-configuration"),
        None => "/.well-known/openid-configuration".to_string(),
    };
    let doc = serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{base}/pf/JWKS"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "end_session_endpoint": format!("{issuer}/signoff")
    });

    let mut app = Router::new().route("/pf/JWKS", {
        let hits = hits.clone();
        get(move || {
            hits.jwks.fetch_add(1, Ordering::SeqCst);
            async { Json(serde_json::json!({"keys": []})) }
        })
    });
    if serve_discovery {
        let hits = hits.clone();
        app = app.route(
            &discovery_path,
            get(move || {
                hits.discovery.fetch_add(1, Ordering::SeqCst);
                async move { Json(doc) }
            }),
        );
    }

    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, hits)
}

fn oidc_config(issuer_url: &str, realm: &str, jwks_url: Option<&str>) -> Config {
    let jwks_line = jwks_url
        .map(|u| format!("jwks_url = \"{u}\"\n"))
        .unwrap_or_default();
    let toml_str = format!(
        r#"
[http]
bind_addr = "127.0.0.1"
bind_port = 3000
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"

[http.oidc]
issuer_url = "{issuer_url}"
realm = "{realm}"
audiences = ["account"]
client_id = "test-client"
client_secret = "test-secret"
{jwks_line}
[logging]
format = "json"
"#
    );
    let mut config: Config = toml_str.parse().expect("test config TOML");
    config.http.with_metrics = false; // avoid Prometheus registry conflicts
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

async fn build_router(config: Config) -> axum_conf::Result<()> {
    FluentRouter::without_state(config)?
        .merge(Router::new().route("/protected", get(|| async { "ok" })))
        .setup_middleware()
        .await
        .map(|_| ())
}

#[tokio::test]
async fn discovery_resolves_non_keycloak_jwks_uri() {
    let (base, hits) = spawn_stub_idp(None, true).await;
    build_router(oidc_config(&base, "", None))
        .await
        .expect("startup via discovery");
    assert!(
        hits.discovery.load(Ordering::SeqCst) >= 1,
        "discovery must be consulted"
    );
    assert!(
        hits.jwks.load(Ordering::SeqCst) >= 1,
        "jwks_uri from discovery must be fetched"
    );
}

#[tokio::test]
async fn keycloak_layout_still_works_without_config_changes() {
    let (base, hits) = spawn_stub_idp(Some("test-realm"), true).await;
    build_router(oidc_config(&base, "test-realm", None))
        .await
        .expect("startup");
    // The Keycloak layout now goes through the realm-prefixed .well-known path.
    assert!(
        hits.discovery.load(Ordering::SeqCst) >= 1,
        "discovery must be consulted"
    );
    // JWKS came from the doc's jwks_uri (/pf/JWKS), not the Keycloak template.
    assert!(hits.jwks.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn explicit_jwks_url_skips_discovery() {
    let (base, hits) = spawn_stub_idp(None, true).await;
    let jwks = format!("{base}/pf/JWKS");
    build_router(oidc_config(&base, "", Some(&jwks)))
        .await
        .expect("startup via override");
    assert_eq!(
        hits.discovery.load(Ordering::SeqCst),
        0,
        "override must skip discovery"
    );
    assert!(hits.jwks.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn discovery_404_fails_startup() {
    let (base, _hits) = spawn_stub_idp(None, false).await; // no discovery route -> 404
    let err = build_router(oidc_config(&base, "", None))
        .await
        .unwrap_err();
    // Pin the fail-fast classification: the failure must come from the
    // discovery step, not a later JWKS fetch.
    assert!(
        err.to_string().contains("discovery"),
        "unexpected error: {err}"
    );
}
