//! Shutdown notification system for graceful application termination.
//!
//! This module provides mechanisms for components to be notified of and react to
//! application shutdown. It offers two complementary approaches:
//!
//! - **[`CancellationToken`]**: A simple mechanism for signaling cancellation to background
//!   tasks. Ideal for stopping work without needing to know the shutdown phase.
//!
//! - **[`ShutdownNotifier`]**: A broadcast-based system that emits [`ShutdownPhase`] events,
//!   allowing components to react differently based on the shutdown stage.
//!
//! # Architecture
//!
//! ```text
//!                    ┌─────────────────────────────┐
//!                    │     SIGTERM / SIGINT        │
//!                    └─────────────┬───────────────┘
//!                                  │
//!                                  ▼
//!                    ┌─────────────────────────────┐
//!                    │    ShutdownNotifier         │
//!                    │  (emits ShutdownPhase)      │
//!                    └─────────────┬───────────────┘
//!                                  │
//!          ┌───────────────────────┼───────────────────────┐
//!          │                       │                       │
//!          ▼                       ▼                       ▼
//!    ┌───────────┐          ┌───────────┐          ┌───────────┐
//!    │ Receiver 1│          │ Receiver 2│          │ Receiver N│
//!    │ (cleanup) │          │ (logging) │          │ (custom)  │
//!    └───────────┘          └───────────┘          └───────────┘
//!
//!                    ┌─────────────────────────────┐
//!                    │    CancellationToken        │
//!                    │   (triggered on Initiated)  │
//!                    └─────────────┬───────────────┘
//!                                  │
//!          ┌───────────────────────┼───────────────────────┐
//!          │                       │                       │
//!          ▼                       ▼                       ▼
//!    ┌───────────┐          ┌───────────┐          ┌───────────┐
//!    │  Task 1   │          │  Task 2   │          │  Task N   │
//!    │  (stops)  │          │  (stops)  │          │  (stops)  │
//!    └───────────┘          └───────────┘          └───────────┘
//! ```
//!
//! # Usage
//!
//! ## Simple Cancellation (Recommended for Most Cases)
//!
//! Use [`CancellationToken`] when you just need to stop background work:
//!
//! ```rust,no_run
//! use axum_conf::{Config, FluentRouter};
//!
//! # async fn example() -> axum_conf::Result<()> {
//! let router = FluentRouter::without_state(Config::default())?;
//! let token = router.cancellation_token();
//!
//! // Spawn a background task that respects shutdown
//! tokio::spawn(async move {
//!     loop {
//!         tokio::select! {
//!             _ = token.cancelled() => {
//!                 tracing::info!("Background task stopping");
//!                 break;
//!             }
//!             _ = do_periodic_work() => {}
//!         }
//!     }
//! });
//! # async fn do_periodic_work() {}
//! # Ok(())
//! # }
//! ```
//!
//! ## Phased Shutdown (For Complex Cleanup)
//!
//! Use [`ShutdownNotifier::subscribe`] when you need to react to different phases:
//!
//! ```rust,no_run
//! use axum_conf::{Config, FluentRouter, ShutdownPhase};
//!
//! # async fn example() -> axum_conf::Result<()> {
//! let router = FluentRouter::without_state(Config::default())?;
//! let mut shutdown_rx = router.shutdown_notifier().subscribe();
//!
//! tokio::spawn(async move {
//!     while let Ok(phase) = shutdown_rx.recv().await {
//!         match phase {
//!             ShutdownPhase::Initiated => {
//!                 tracing::info!("Shutdown starting - stop accepting new work");
//!             }
//!             ShutdownPhase::GracePeriodStarted { timeout } => {
//!                 tracing::info!("Grace period: {}s to complete work", timeout.as_secs());
//!             }
//!             ShutdownPhase::GracePeriodEnded => {
//!                 tracing::warn!("Grace period ended - forcing shutdown");
//!             }
//!         }
//!     }
//! });
//! # Ok(())
//! # }
//! ```

use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// The phases of a graceful shutdown sequence.
///
/// These phases are emitted in order during shutdown, allowing components
/// to react appropriately at each stage.
///
/// # Phase Sequence
///
/// ```text
/// 1. Initiated          → Signal received, stop accepting new work
/// 2. GracePeriodStarted → In-flight requests draining, countdown begins
/// 3. GracePeriodEnded   → Timeout expired, forcing shutdown
/// ```
///
/// # Example
///
/// ```rust,no_run
/// use axum_conf::ShutdownPhase;
///
/// fn handle_phase(phase: ShutdownPhase) {
///     match phase {
///         ShutdownPhase::Initiated => {
///             // Stop accepting new connections/jobs
///             // Mark service as unhealthy for load balancers
///         }
///         ShutdownPhase::GracePeriodStarted { timeout } => {
///             // Log remaining time
///             // Optionally cancel long-running operations
///         }
///         ShutdownPhase::GracePeriodEnded => {
///             // Final cleanup before forced termination
///             // Flush any buffered data
///         }
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// Shutdown signal received (SIGTERM or SIGINT).
    ///
    /// At this point:
    /// - The server stops accepting new connections
    /// - The [`CancellationToken`] is triggered
    /// - Components should stop accepting new work
    /// - Existing work should continue until completion or timeout
    Initiated,

    /// Grace period has started for in-flight requests.
    ///
    /// The `timeout` indicates how long the server will wait for requests
    /// to complete before forcing shutdown.
    ///
    /// At this point:
    /// - In-flight requests are being processed
    /// - No new connections are accepted
    /// - Components should prioritize completing critical work
    GracePeriodStarted {
        /// The configured shutdown timeout duration.
        timeout: Duration,
    },

    /// Grace period has ended, shutdown will be forced.
    ///
    /// This phase indicates the timeout has expired and the process
    /// will terminate shortly. Any remaining work will be abandoned.
    ///
    /// At this point:
    /// - All cleanup should be complete
    /// - Final log messages should be flushed
    /// - Process termination is imminent
    GracePeriodEnded,
}

/// Manages shutdown notifications for the application.
///
/// `ShutdownNotifier` provides a broadcast channel for shutdown phase notifications
/// and a cancellation token for simple task cancellation. Multiple components can
/// subscribe to receive shutdown events.
///
/// # Creating Subscribers
///
/// ```rust,no_run
/// use axum_conf::{Config, FluentRouter, ShutdownPhase};
///
/// # async fn example() -> axum_conf::Result<()> {
/// let router = FluentRouter::without_state(Config::default())?;
/// let notifier = router.shutdown_notifier();
///
/// // Create multiple subscribers
/// let mut rx1 = notifier.subscribe();
/// let mut rx2 = notifier.subscribe();
///
/// // Each subscriber receives all phases independently
/// # Ok(())
/// # }
/// ```
///
/// # Thread Safety
///
/// `ShutdownNotifier` is `Clone`, `Send`, and `Sync`. Cloning creates a new handle
/// to the same underlying notification system - all clones share the same broadcast
/// channel.
#[derive(Clone)]
pub struct ShutdownNotifier {
    sender: broadcast::Sender<ShutdownPhase>,
    cancel_token: CancellationToken,
}

impl ShutdownNotifier {
    /// Creates a new shutdown notifier with the specified broadcast capacity.
    ///
    /// The capacity determines how many unread messages can be buffered per subscriber.
    /// If a subscriber falls behind by more than this amount, it will start missing messages.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of buffered messages per subscriber
    ///
    /// # Example
    ///
    /// ```rust
    /// use axum_conf::ShutdownNotifier;
    ///
    /// // Default capacity of 16 is usually sufficient
    /// let notifier = ShutdownNotifier::new(16);
    /// ```
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            cancel_token: CancellationToken::new(),
        }
    }

    /// Creates a new subscriber for shutdown phase notifications.
    ///
    /// Each subscriber receives all shutdown phases independently. Subscribers
    /// created after a phase is emitted will not receive that phase.
    ///
    /// # Returns
    ///
    /// A [`broadcast::Receiver<ShutdownPhase>`] that receives shutdown events.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use axum_conf::{ShutdownNotifier, ShutdownPhase};
    ///
    /// # async fn example() {
    /// let notifier = ShutdownNotifier::new(16);
    /// let mut rx = notifier.subscribe();
    ///
    /// // Wait for shutdown phases
    /// tokio::spawn(async move {
    ///     while let Ok(phase) = rx.recv().await {
    ///         println!("Received phase: {:?}", phase);
    ///     }
    /// });
    /// # }
    /// ```
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownPhase> {
        self.sender.subscribe()
    }

    /// Returns a cloned reference to the cancellation token.
    ///
    /// The cancellation token is triggered when [`ShutdownPhase::Initiated`] is emitted.
    /// Use this for simple "stop work" signaling in background tasks.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use axum_conf::ShutdownNotifier;
    ///
    /// # async fn example() {
    /// let notifier = ShutdownNotifier::new(16);
    /// let token = notifier.cancellation_token();
    ///
    /// tokio::spawn(async move {
    ///     loop {
    ///         tokio::select! {
    ///             _ = token.cancelled() => {
    ///                 println!("Cancelled!");
    ///                 break;
    ///             }
    ///             _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
    ///                 println!("Working...");
    ///             }
    ///         }
    ///     }
    /// });
    /// # }
    /// ```
    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    /// Returns `true` if shutdown has been initiated.
    ///
    /// This is equivalent to checking `cancellation_token().is_cancelled()`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use axum_conf::ShutdownNotifier;
    ///
    /// let notifier = ShutdownNotifier::new(16);
    /// assert!(!notifier.is_shutdown_initiated());
    /// ```
    #[must_use]
    pub fn is_shutdown_initiated(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Emits a shutdown phase to all subscribers.
    ///
    /// If the phase is [`ShutdownPhase::Initiated`], the cancellation token
    /// is also triggered.
    ///
    /// # Arguments
    ///
    /// * `phase` - The shutdown phase to emit
    ///
    /// # Returns
    ///
    /// The number of subscribers that received the message. Returns 0 if
    /// there are no active subscribers.
    pub(crate) fn emit(&self, phase: ShutdownPhase) -> usize {
        // Trigger cancellation token on Initiated phase
        if phase == ShutdownPhase::Initiated {
            self.cancel_token.cancel();
        }

        // Send to all subscribers (ignore error if no receivers)
        self.sender.send(phase).unwrap_or(0)
    }
}

impl Default for ShutdownNotifier {
    /// Creates a new shutdown notifier with default capacity of 16.
    fn default() -> Self {
        Self::new(16)
    }
}

impl std::fmt::Debug for ShutdownNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShutdownNotifier")
            .field("subscriber_count", &self.sender.receiver_count())
            .field("is_shutdown_initiated", &self.is_shutdown_initiated())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_shutdown_notifier_creation() {
        let notifier = ShutdownNotifier::new(8);
        assert!(!notifier.is_shutdown_initiated());
    }

    #[tokio::test]
    async fn test_shutdown_notifier_default() {
        let notifier = ShutdownNotifier::default();
        assert!(!notifier.is_shutdown_initiated());
    }

    #[tokio::test]
    async fn test_emit_initiated_triggers_cancellation() {
        let notifier = ShutdownNotifier::new(8);
        let token = notifier.cancellation_token();

        assert!(!token.is_cancelled());
        notifier.emit(ShutdownPhase::Initiated);
        assert!(token.is_cancelled());
        assert!(notifier.is_shutdown_initiated());
    }

    #[tokio::test]
    async fn test_subscriber_receives_phases() {
        let notifier = ShutdownNotifier::new(8);
        let mut rx = notifier.subscribe();

        notifier.emit(ShutdownPhase::Initiated);
        notifier.emit(ShutdownPhase::GracePeriodStarted {
            timeout: Duration::from_secs(30),
        });
        notifier.emit(ShutdownPhase::GracePeriodEnded);

        assert_eq!(rx.recv().await.unwrap(), ShutdownPhase::Initiated);
        assert_eq!(
            rx.recv().await.unwrap(),
            ShutdownPhase::GracePeriodStarted {
                timeout: Duration::from_secs(30)
            }
        );
        assert_eq!(rx.recv().await.unwrap(), ShutdownPhase::GracePeriodEnded);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let notifier = ShutdownNotifier::new(8);
        let mut rx1 = notifier.subscribe();
        let mut rx2 = notifier.subscribe();

        notifier.emit(ShutdownPhase::Initiated);

        assert_eq!(rx1.recv().await.unwrap(), ShutdownPhase::Initiated);
        assert_eq!(rx2.recv().await.unwrap(), ShutdownPhase::Initiated);
    }

    #[tokio::test]
    async fn test_cancellation_token_in_select() {
        let notifier = ShutdownNotifier::new(8);
        let token = notifier.cancellation_token();

        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => true,
                _ = tokio::time::sleep(Duration::from_secs(10)) => false,
            }
        });

        // Small delay to ensure task is waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        notifier.emit(ShutdownPhase::Initiated);

        let result = handle.await.unwrap();
        assert!(result, "Task should have been cancelled");
    }

    #[tokio::test]
    async fn test_clone_shares_state() {
        let notifier1 = ShutdownNotifier::new(8);
        let notifier2 = notifier1.clone();

        let mut rx = notifier1.subscribe();

        // Emit from clone
        notifier2.emit(ShutdownPhase::Initiated);

        // Original subscriber should receive it
        assert_eq!(rx.recv().await.unwrap(), ShutdownPhase::Initiated);

        // Both should show initiated
        assert!(notifier1.is_shutdown_initiated());
        assert!(notifier2.is_shutdown_initiated());
    }

    #[tokio::test]
    async fn test_emit_returns_subscriber_count() {
        let notifier = ShutdownNotifier::new(8);

        // No subscribers
        let count = notifier.emit(ShutdownPhase::GracePeriodEnded);
        assert_eq!(count, 0);

        // Add subscribers
        let _rx1 = notifier.subscribe();
        let _rx2 = notifier.subscribe();

        let count = notifier.emit(ShutdownPhase::GracePeriodEnded);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_shutdown_phase_debug() {
        let phase = ShutdownPhase::GracePeriodStarted {
            timeout: Duration::from_secs(30),
        };
        let debug_str = format!("{:?}", phase);
        assert!(debug_str.contains("GracePeriodStarted"));
        assert!(debug_str.contains("30"));
    }

    #[test]
    fn test_shutdown_notifier_debug() {
        let notifier = ShutdownNotifier::new(8);
        let _rx = notifier.subscribe();
        let debug_str = format!("{:?}", notifier);
        assert!(debug_str.contains("ShutdownNotifier"));
        assert!(debug_str.contains("subscriber_count"));
    }
}
