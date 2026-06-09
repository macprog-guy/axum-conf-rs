//! Integration round-trip tests for the built-in session store backends.
//!
//! Each test stands up a real backend (Postgres / Redis) via testcontainers and
//! proves a session persists **across two independent router instances sharing
//! the same store** — i.e. the multi-replica scenario the feature exists for.
//!
//! Requires Docker. Run with the relevant feature, e.g.:
//!   cargo test --features session-postgres --test session_store_tests
//!   cargo test --features session-redis    --test session_store_tests
#![cfg(any(feature = "session-postgres", feature = "session-redis"))]

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    routing::get,
};
use axum_conf::{Config, FluentRouter, HttpMiddleware};
use tower::ServiceExt;
use tower_sessions::Session;

/// Handler that increments a counter held in the session, exercising the
/// store's load → save round-trip on every call.
async fn session_handler(session: Session) -> Result<String, (StatusCode, String)> {
    let counter: i32 = session.get("counter").await.unwrap().unwrap_or(0);
    let next = counter + 1;
    session
        .insert("counter", next)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(format!("Counter: {next}"))
}

async fn build_app(config: Config) -> Router {
    FluentRouter::without_state(config)
        .expect("Failed to create FluentRouter")
        .route("/counter", get(session_handler))
        .setup_middleware()
        .await
        .expect("Failed to set up middleware")
        .into_inner()
}

/// Sends `GET /counter`, optionally with a `Cookie`, returning
/// (status, set-cookie value, body).
async fn counter_request(
    app: &Router,
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, String) {
    let mut builder = Request::builder().uri("/counter");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();

    let status = response.status();
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap().to_string());
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        set_cookie,
        String::from_utf8_lossy(&body).to_string(),
    )
}

#[cfg(feature = "session-postgres")]
#[tokio::test]
async fn postgres_session_store_round_trip() {
    use axum_conf::SessionStoreConfig;
    use testcontainers::ImageExt;
    use testcontainers_modules::{postgres::Postgres, testcontainers::runners::AsyncRunner};

    let pg = Postgres::default()
        .with_tag("16.4")
        .start()
        .await
        .expect("Could not start postgres container");
    let url = format!(
        "postgres://postgres:postgres@{}:{}/postgres",
        pg.get_host().await.unwrap(),
        pg.get_host_port_ipv4(5432).await.unwrap()
    );

    let make_config = || {
        let mut config = Config::new();
        config.http.with_metrics = false; // avoid the global Prometheus registry across instances
        config.http.session_store = SessionStoreConfig::Postgres;
        config.database.url = url.clone();
        config.with_excluded_middlewares(vec![HttpMiddleware::RateLimiting])
    };

    // Instance A: establish a session and confirm the in-store round-trip.
    let app_a = build_app(make_config()).await;
    let (status, cookie, body) = counter_request(&app_a, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "Counter: 1");
    let cookie = cookie.expect("session cookie should be set");

    let (_, _, body) = counter_request(&app_a, Some(&cookie)).await;
    assert_eq!(
        body, "Counter: 2",
        "session must round-trip through Postgres"
    );

    // Instance B: a fresh router against the same database must see the session.
    let app_b = build_app(make_config()).await;
    let (_, _, body) = counter_request(&app_b, Some(&cookie)).await;
    assert_eq!(
        body, "Counter: 3",
        "session must persist across router instances sharing the store"
    );
}

#[cfg(feature = "session-redis")]
#[tokio::test]
async fn redis_session_store_round_trip() {
    use axum_conf::SessionStoreConfig;
    use testcontainers::{
        GenericImage,
        core::{ContainerPort, WaitFor},
        runners::AsyncRunner,
    };

    let redis = GenericImage::new("redis", "7-alpine")
        .with_exposed_port(ContainerPort::Tcp(6379))
        .with_wait_for(WaitFor::message_on_stdout("Ready to accept connections"))
        .start()
        .await
        .expect("Could not start redis container");
    let url = format!(
        "redis://{}:{}",
        redis.get_host().await.unwrap(),
        redis.get_host_port_ipv4(6379).await.unwrap()
    );

    let make_config = || {
        let mut config = Config::new();
        config.http.with_metrics = false;
        config.http.session_store = SessionStoreConfig::Redis { url: url.clone() };
        config.with_excluded_middlewares(vec![HttpMiddleware::RateLimiting])
    };

    let app_a = build_app(make_config()).await;
    let (status, cookie, body) = counter_request(&app_a, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "Counter: 1");
    let cookie = cookie.expect("session cookie should be set");

    let (_, _, body) = counter_request(&app_a, Some(&cookie)).await;
    assert_eq!(body, "Counter: 2", "session must round-trip through Redis");

    let app_b = build_app(make_config()).await;
    let (_, _, body) = counter_request(&app_b, Some(&cookie)).await;
    assert_eq!(
        body, "Counter: 3",
        "session must persist across router instances sharing the store"
    );
}
