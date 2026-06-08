//! Tests for the application-supplied readiness hook (`with_readiness_check`).
//!
//! These tests exercise the composition of the application readiness closure with
//! the built-in database / circuit-breaker checks performed by `setup_readiness`.
//!
//! Feature notes:
//! - The application check is evaluated **before** the database check, so a
//!   `NotReady` result yields a `503` regardless of whether the `postgres`
//!   feature is enabled (no database round-trip occurs).
//! - The `200 OK` cases require the built-in database check to pass, which under
//!   `--all-features` (postgres on) would need a live database. Those assertions
//!   are therefore gated to `not(feature = "postgres")`; the postgres-on
//!   composition is verified by forcing the database circuit breaker open
//!   (deterministic and network-free) instead.

use crate::{Config, FluentRouter, Readiness};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt; // for `oneshot`

/// Builds a `GET /ready` request (the default readiness route).
fn ready_request() -> Request<Body> {
    Request::builder()
        .uri("/ready")
        .body(Body::empty())
        .unwrap()
}

/// Reads a response body into a `String`.
async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Application state used to prove the hook receives a clone of the app state.
#[derive(Clone)]
struct SaturationState {
    /// Number of available worker permits (0 == saturated).
    available_permits: usize,
    /// Flipped by the hook so the test can confirm it ran with the state.
    hook_invoked: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Criterion 1: with no hook registered, `/ready` returns `200 OK`.
///
/// Gated to non-postgres builds because, with `postgres` enabled, the built-in
/// database check runs against a database that is not available in unit tests.
#[cfg(not(feature = "postgres"))]
#[tokio::test]
async fn readiness_without_hook_returns_ok() {
    let app = FluentRouter::without_state(Config::new())
        .unwrap()
        .setup_readiness()
        .into_inner();

    let response = app.oneshot(ready_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response).await, "OK\n");
}

/// Criterion 3: a hook returning `Readiness::ready()` returns `200 OK`.
#[cfg(not(feature = "postgres"))]
#[tokio::test]
async fn readiness_hook_ready_returns_ok() {
    let app = FluentRouter::without_state(Config::new())
        .unwrap()
        .with_readiness_check(|_state: ()| async move { Readiness::ready() })
        .setup_readiness()
        .into_inner();

    let response = app.oneshot(ready_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_string(response).await, "OK\n");
}

/// Criterion 2: a hook returning `Readiness::not_ready("saturated")` returns
/// `503` with the message in the body.
///
/// Feature-agnostic: because the application check runs first, the `503` is
/// produced before any database access.
#[tokio::test]
async fn readiness_hook_not_ready_returns_503_with_message() {
    let app = FluentRouter::without_state(Config::new())
        .unwrap()
        .with_readiness_check(|_state: ()| async move { Readiness::not_ready("saturated") })
        .setup_readiness()
        .into_inner();

    let response = app.oneshot(ready_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        body_string(response).await.contains("saturated"),
        "503 body should surface the NotReady message"
    );
}

/// Criterion 5: the hook receives a clone of the application state and can read
/// it. The closure reads `available_permits` (driving the outcome) and flips
/// `hook_invoked` so the test can confirm it ran with the state.
///
/// Feature-agnostic: the `NotReady` result short-circuits before the database
/// check.
#[tokio::test]
async fn readiness_hook_receives_app_state() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let hook_invoked = std::sync::Arc::new(AtomicBool::new(false));
    let state = SaturationState {
        available_permits: 0,
        hook_invoked: hook_invoked.clone(),
    };

    let app = FluentRouter::<SaturationState>::with_state(Config::new(), state.clone())
        .unwrap()
        .with_readiness_check(|s: SaturationState| async move {
            s.hook_invoked.store(true, Ordering::SeqCst);
            if s.available_permits == 0 {
                Readiness::not_ready(format!("no permits (available={})", s.available_permits))
            } else {
                Readiness::ready()
            }
        })
        .setup_readiness()
        .into_inner()
        .with_state(state);

    let response = app.oneshot(ready_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        body_string(response).await.contains("available=0"),
        "503 body should reflect data read from the app state"
    );
    assert!(
        hook_invoked.load(Ordering::SeqCst),
        "the hook must have been invoked with the application state"
    );
}

/// Criterion 4 (postgres + circuit-breaker): the application hook composes with —
/// rather than replaces — the built-in check. When the built-in check fails
/// (here, the database circuit breaker is open) `/ready` returns `503` even
/// though the application reports `Ready`.
///
/// The breaker is forced open directly (default `failure_threshold` is 5), which
/// keeps the test deterministic and free of any network access — the handler
/// short-circuits at the circuit check before attempting `SELECT 1`.
#[cfg(all(feature = "postgres", feature = "circuit-breaker"))]
#[tokio::test]
async fn readiness_built_in_check_failure_returns_503_even_when_app_ready() {
    use std::sync::atomic::{AtomicBool, Ordering};

    // Records that the application hook actually ran (and returned Ready), so we
    // can prove it was *composed* with the built-in check rather than bypassed.
    let hook_ran = std::sync::Arc::new(AtomicBool::new(false));
    let hook_ran_for_closure = hook_ran.clone();

    let router = FluentRouter::without_state(Config::new())
        .unwrap()
        .with_readiness_check(move |_state: ()| {
            let hook_ran = hook_ran_for_closure.clone();
            async move {
                hook_ran.store(true, Ordering::SeqCst);
                Readiness::ready()
            }
        });

    // Trip the "database" circuit breaker so the built-in readiness check fails.
    let breaker = router.circuit_breakers().get_or_default("database");
    for _ in 0..10 {
        breaker.record_failure();
    }
    assert!(
        !breaker.should_allow(),
        "precondition: the database circuit breaker should be open"
    );

    let app = router.setup_readiness().into_inner();
    let response = app.oneshot(ready_request()).await.unwrap();

    // App reports ready, but the built-in check fails -> composed result is 503.
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    // The 503 must originate at the built-in circuit-breaker check...
    assert!(
        body_string(response).await.contains("Database circuit"),
        "the 503 should come from the built-in circuit-breaker check"
    );
    // ...and the application hook must have actually been consulted (returned
    // Ready) — proving composition, not replacement.
    assert!(
        hook_ran.load(Ordering::SeqCst),
        "the application readiness hook should have been consulted and returned Ready"
    );
}

/// Pins the app-before-built-in evaluation order: when the application hook
/// reports `NotReady` *and* the built-in circuit breaker is open, the application
/// message wins because the application check is evaluated first and
/// short-circuits — the built-in check is never reached.
#[cfg(all(feature = "postgres", feature = "circuit-breaker"))]
#[tokio::test]
async fn readiness_app_check_precedes_built_in_check() {
    let router = FluentRouter::without_state(Config::new())
        .unwrap()
        .with_readiness_check(|_state: ()| async move { Readiness::not_ready("saturated") });

    let breaker = router.circuit_breakers().get_or_default("database");
    for _ in 0..10 {
        breaker.record_failure();
    }

    let app = router.setup_readiness().into_inner();
    let response = app.oneshot(ready_request()).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_string(response).await;
    assert!(
        body.contains("saturated"),
        "the application message should win (evaluated first)"
    );
    assert!(
        !body.contains("Database circuit"),
        "the built-in check should not run once the application reports NotReady"
    );
}
