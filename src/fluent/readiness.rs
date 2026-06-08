//! Application-supplied readiness checks for the `/ready` endpoint.
//!
//! By default the readiness probe set up by [`FluentRouter::setup_readiness`](crate::FluentRouter::setup_readiness)
//! reflects only infrastructure health (database connectivity and, when enabled,
//! the database circuit breaker). Many services also need `/ready` to reflect
//! *application* state — for example shedding load when a bounded worker pool is
//! saturated — so that a load balancer stops routing to an instance that cannot
//! make progress.
//!
//! [`FluentRouter::with_readiness_check`](crate::FluentRouter::with_readiness_check)
//! registers an application closure that receives the app state and returns a
//! [`Readiness`]. The closure is composed with (not a replacement for) the
//! built-in checks: the endpoint reports ready **iff** the application check
//! returns [`Readiness::Ready`] *and* the built-in database/circuit-breaker
//! check passes.

use std::{future::Future, pin::Pin, sync::Arc};

/// Outcome of an application readiness check.
///
/// Returned by the closure registered with
/// [`FluentRouter::with_readiness_check`](crate::FluentRouter::with_readiness_check).
/// [`Readiness::NotReady`] carries a human-readable message that is surfaced in
/// the `503` response body and emitted via `tracing::warn!`.
///
/// # Examples
///
/// ```rust
/// use axum_conf::Readiness;
///
/// fn check(available_permits: usize) -> Readiness {
///     if available_permits == 0 {
///         Readiness::not_ready("all conversion permits held")
///     } else {
///         Readiness::ready()
///     }
/// }
///
/// assert_eq!(check(4), Readiness::Ready);
/// assert!(matches!(check(0), Readiness::NotReady(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// The application is ready to serve traffic.
    Ready,
    /// The application is not ready; the message is surfaced in the `503` body and logs.
    NotReady(String),
}

impl Readiness {
    /// Returns [`Readiness::Ready`].
    #[must_use]
    pub fn ready() -> Self {
        Readiness::Ready
    }

    /// Returns [`Readiness::NotReady`] carrying the supplied message.
    ///
    /// The message is surfaced in the `503 Service Unavailable` response body
    /// and logged via `tracing::warn!`.
    #[must_use]
    pub fn not_ready(msg: impl Into<String>) -> Self {
        Readiness::NotReady(msg.into())
    }

    /// Returns `true` when the value is [`Readiness::Ready`].
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Readiness::Ready)
    }
}

/// Type-erased, shareable application readiness check stored on the builder.
///
/// `with_readiness_check` boxes the caller's `Fn(State) -> impl Future<Output = Readiness>`
/// into this shape so it can be cloned into the readiness handler (which is itself
/// cloned per request by axum).
pub(crate) type ReadinessCheck<State> =
    Arc<dyn Fn(State) -> Pin<Box<dyn Future<Output = Readiness> + Send + 'static>> + Send + Sync>;
