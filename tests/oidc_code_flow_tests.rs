//! Integration tests for the OIDC Authorization Code flow.
//!
//! Exercises the full browser-like flow against a real Keycloak testcontainer:
//! login redirect -> Keycloak form submission -> callback code exchange ->
//! session-based identity -> Bearer coexistence -> logout.

#![cfg(feature = "keycloak")]

use axum::{Router, routing::get};
use axum_conf::{
    AuthenticatedIdentity, Config, FluentRouter, HttpMiddleware, HttpMiddlewareConfig,
};
use reqwest::redirect::Policy;
use std::time::Duration;
use tokio::net::TcpListener;

#[cfg(feature = "postgres")]
use {
    testcontainers::{ImageExt, runners::AsyncRunner},
    testcontainers_modules::postgres::Postgres,
};

mod keycloak;
use keycloak::KeycloakContainer;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn protected_handler(identity: AuthenticatedIdentity) -> String {
    format!(
        "user={} email={} method={:?}",
        identity.user,
        identity.email.unwrap_or_default(),
        identity.method,
    )
}

async fn optional_handler(identity: Option<AuthenticatedIdentity>) -> String {
    match identity {
        Some(id) => format!("authenticated={}", id.user),
        None => "anonymous".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Config helper
// ---------------------------------------------------------------------------

fn create_code_flow_config(keycloak_url: &str, port: u16) -> Config {
    let redirect_uri = format!("http://127.0.0.1:{port}/auth/callback");
    let toml_str = format!(
        r#"
[http]
bind_addr = "127.0.0.1"
bind_port = {port}
max_concurrent_requests = 100
max_payload_size_bytes = "1KiB"
liveness_route = "/health"
readiness_route = "/ready"
metrics_route = "/metrics"

[http.oidc]
issuer_url = "{keycloak_url}"
realm = "test-realm"
audiences = ["account"]
client_id = "test-confidential"
client_secret = "test-secret"
redirect_uri = "{redirect_uri}"
scopes = ["openid", "profile", "email"]
post_login_redirect = "/dashboard"
post_logout_redirect = "/"

[logging]
format = "json"
"#,
    );

    let mut config: Config = toml_str.parse().expect("Failed to parse test config TOML");
    config.http.with_metrics = false;
    config.http.middleware = Some(HttpMiddlewareConfig::Exclude(vec![
        HttpMiddleware::RateLimiting,
    ]));
    config
}

// ---------------------------------------------------------------------------
// HTML parsing helper
// ---------------------------------------------------------------------------

/// Extracts the form action URL from Keycloak's login page HTML.
fn extract_form_action(html: &str) -> String {
    // Look for: <form id="kc-form-login" ... action="...">
    let action_marker = "id=\"kc-form-login\"";
    let form_pos = html
        .find(action_marker)
        .unwrap_or_else(|| panic!("Could not find kc-form-login in HTML:\n{html}"));

    let after_form = &html[form_pos..];
    let action_start = after_form
        .find("action=\"")
        .expect("Could not find action attribute")
        + "action=\"".len();
    let action_end = after_form[action_start..]
        .find('"')
        .expect("Could not find closing quote for action");

    after_form[action_start..action_start + action_end].replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_oidc_auth_code_flow_full() {
    // ── Setup ────────────────────────────────────────────────────────────

    // Start Keycloak
    let keycloak = KeycloakContainer::start().await;

    // Bind a TCP listener to get a random port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind");
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    // Create realm with confidential client whose redirect_uri matches our listener
    let redirect_uri = format!("{base_url}/auth/callback");
    keycloak.create_code_flow_realm(&redirect_uri).await;

    #[cfg(feature = "postgres")]
    let pg_server = Postgres::default()
        .with_tag("16.4")
        .start()
        .await
        .expect("Could not start postgres server");

    let mut config = create_code_flow_config(keycloak.url.as_str(), port);

    #[cfg(feature = "postgres")]
    {
        let database_url = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            pg_server.get_host().await.unwrap(),
            pg_server.get_host_port_ipv4(5432).await.unwrap()
        );
        config.database.url = database_url;
    }

    // Build the router
    let app = FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .merge(
            Router::new()
                .route("/protected", get(protected_handler))
                .route("/optional", get(optional_handler))
                .route("/dashboard", get(|| async { "dashboard" })),
        )
        .setup_middleware()
        .await
        .expect("Failed to setup middleware")
        .into_inner();

    let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .expect("Server failed");
    });

    // Let middleware (OIDC discovery) settle
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Cookie-enabled client that does NOT auto-follow redirects
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build reqwest client");

    // ── 1. Unauthenticated access ────────────────────────────────────────

    // Optional endpoint → anonymous
    let resp = client
        .get(format!("{base_url}/optional"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "anonymous");

    // Protected endpoint → 401
    let resp = client
        .get(format!("{base_url}/protected"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "Protected endpoint should return 401 when unauthenticated"
    );

    // ── 2. Login redirect ────────────────────────────────────────────────

    let resp = client
        .get(format!("{base_url}/auth/login"))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_redirection(),
        "Login should redirect, got {}",
        resp.status()
    );

    let keycloak_auth_url = resp
        .headers()
        .get("location")
        .expect("Login redirect missing Location header")
        .to_str()
        .unwrap()
        .to_string();

    // Verify the redirect URL contains expected OIDC parameters
    assert!(
        keycloak_auth_url.contains("client_id=test-confidential"),
        "Auth URL should contain client_id: {keycloak_auth_url}"
    );
    assert!(
        keycloak_auth_url.contains("redirect_uri="),
        "Auth URL should contain redirect_uri: {keycloak_auth_url}"
    );
    assert!(
        keycloak_auth_url.contains("code_challenge="),
        "Auth URL should contain PKCE code_challenge: {keycloak_auth_url}"
    );
    assert!(
        keycloak_auth_url.contains("scope="),
        "Auth URL should contain scope: {keycloak_auth_url}"
    );

    // ── 3. Keycloak authentication ───────────────────────────────────────

    // Follow Keycloak redirects to get the login form
    // Use a separate client for Keycloak that follows redirects internally
    let kc_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(Policy::limited(10))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let login_page = kc_client
        .get(&keycloak_auth_url)
        .send()
        .await
        .expect("Failed to fetch Keycloak login page");
    assert_eq!(
        login_page.status(),
        200,
        "Keycloak login page should return 200"
    );

    let _login_html = login_page.text().await.unwrap();

    // POST credentials to Keycloak
    let kc_post_client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    // We need to first GET the login page with the non-redirect client to collect cookies
    let login_page2 = kc_post_client
        .get(&keycloak_auth_url)
        .send()
        .await
        .unwrap();

    // Keycloak may return the page directly or redirect — follow to the form
    let (form_action_url, kc_session_client) = if login_page2.status() == 200 {
        let html = login_page2.text().await.unwrap();
        (extract_form_action(&html), kc_post_client)
    } else if login_page2.status().is_redirection() {
        // Follow redirects manually while preserving cookies
        let redirect_url = login_page2
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let page = kc_post_client.get(&redirect_url).send().await.unwrap();
        let html = page.text().await.unwrap();
        (extract_form_action(&html), kc_post_client)
    } else {
        panic!(
            "Unexpected status from Keycloak: {}",
            login_page2.status()
        );
    };

    let auth_response = kc_session_client
        .post(&form_action_url)
        .form(&[
            ("username", "test-user-mail@foo.bar"),
            ("password", "password"),
        ])
        .send()
        .await
        .expect("Failed to POST credentials to Keycloak");

    assert!(
        auth_response.status().is_redirection(),
        "Keycloak should redirect after login, got {} body: {}",
        auth_response.status(),
        "(redirect expected)"
    );

    let callback_url = auth_response
        .headers()
        .get("location")
        .expect("Keycloak login redirect missing Location header")
        .to_str()
        .unwrap()
        .to_string();

    // The callback URL should point back to our app
    assert!(
        callback_url.contains("/auth/callback"),
        "Keycloak should redirect to our callback: {callback_url}"
    );
    assert!(
        callback_url.contains("code="),
        "Callback URL should contain authorization code: {callback_url}"
    );
    assert!(
        callback_url.contains("state="),
        "Callback URL should contain state: {callback_url}"
    );

    // ── 4. Callback (code exchange) ──────────────────────────────────────

    // Hit our callback endpoint with the authorization code.
    // The session cookie from step 2 (/auth/login) must be present.
    let callback_resp = client.get(&callback_url).send().await.unwrap();

    assert!(
        callback_resp.status().is_redirection(),
        "Callback should redirect to post_login_redirect, got {}",
        callback_resp.status()
    );

    let post_login_location = callback_resp
        .headers()
        .get("location")
        .expect("Callback redirect missing Location header")
        .to_str()
        .unwrap();

    assert_eq!(
        post_login_location, "/dashboard",
        "Should redirect to post_login_redirect"
    );

    // ── 5. Session-based identity ────────────────────────────────────────

    // With session cookies, protected endpoint should now work
    let resp = client
        .get(format!("{base_url}/protected"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "Protected endpoint should be accessible with session"
    );
    let body = resp.text().await.unwrap();
    assert!(
        body.contains("method=Oidc"),
        "Auth method should be Oidc: {body}"
    );
    assert!(
        body.contains("email=test-user-mail@foo.bar"),
        "Email should match test user: {body}"
    );

    // Optional endpoint should show authenticated user
    let resp = client
        .get(format!("{base_url}/optional"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.starts_with("authenticated="),
        "Should be authenticated: {body}"
    );

    // ── 6. Bearer token coexistence ──────────────────────────────────────

    // Fresh client (no cookies) with a Bearer token from direct-access-grants.
    // Verifies that Bearer tokens produce AuthenticatedIdentity alongside session auth.
    let bearer_token = keycloak
        .perform_password_login(
            "test-user-mail@foo.bar",
            "password",
            "test-realm",
            "test-client",
        )
        .await;

    let fresh_client = reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let resp = fresh_client
        .get(format!("{base_url}/optional"))
        .header("Authorization", format!("Bearer {bearer_token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.starts_with("authenticated="),
        "Bearer token should produce AuthenticatedIdentity: {body}"
    );

    // ── 7. Logout ────────────────────────────────────────────────────────

    let resp = client
        .get(format!("{base_url}/auth/logout"))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_redirection(),
        "Logout should redirect, got {}",
        resp.status()
    );

    // ── 8. Post-logout verification ──────────────────────────────────────

    // After logout, session should be cleared
    let resp = client
        .get(format!("{base_url}/optional"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.text().await.unwrap(),
        "anonymous",
        "Should be anonymous after logout"
    );

    server_handle.abort();
}
