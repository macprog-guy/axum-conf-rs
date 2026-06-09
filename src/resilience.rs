//! Retry helpers for transient failures.
//!
//! [`retry_transient`] re-runs an operation while it returns a transient error
//! ([`Error::is_transient`](crate::Error::is_transient)), backing off with full
//! jitter so a fleet of processes does not hammer a recovering dependency in
//! lockstep after a blip (thundering herd).

use crate::Result;
use std::time::Duration;

/// Controls attempts and backoff for [`retry_transient`].
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Maximum number of attempts, including the first. Must be at least 1.
    pub max_attempts: u32,
    /// Base backoff delay; the cap doubles each attempt up to `max_delay`.
    pub base_delay: Duration,
    /// Upper bound on a single backoff delay.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    /// 3 attempts, 100 ms base, 5 s cap.
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
        }
    }
}

impl RetryPolicy {
    /// Full-jitter backoff for a 1-based attempt number: a uniform random delay
    /// in `[0, min(max_delay, base_delay * 2^(attempt-1)))`.
    fn backoff(&self, attempt: u32) -> Duration {
        let exp = attempt.saturating_sub(1).min(31);
        let cap = self
            .base_delay
            .saturating_mul(1u32 << exp)
            .min(self.max_delay);
        cap.mul_f64(jitter_fraction())
    }
}

/// Retries `op` while it returns a transient error, using full-jitter
/// exponential backoff, up to `policy.max_attempts`.
///
/// A non-transient error (per [`Error::is_transient`](crate::Error::is_transient))
/// is returned immediately, since retrying a deterministic failure cannot help.
///
/// ```rust,no_run
/// # async fn example() -> axum_conf::Result<()> {
/// use axum_conf::resilience::{retry_transient, RetryPolicy};
///
/// let value = retry_transient(RetryPolicy::default(), || async {
///     // some fallible async operation returning axum_conf::Result<T>
///     Ok::<_, axum_conf::Error>(42)
/// })
/// .await?;
/// # let _ = value;
/// # Ok(())
/// # }
/// ```
pub async fn retry_transient<F, Fut, T>(policy: RetryPolicy, mut op: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 1;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                if attempt >= policy.max_attempts || !e.is_transient() {
                    return Err(e);
                }
                let delay = policy.backoff(attempt);
                tracing::warn!(
                    attempt,
                    max_attempts = policy.max_attempts,
                    backoff_ms = delay.as_millis() as u64,
                    error = %e,
                    "transient failure; retrying after backoff"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// A pseudo-random fraction in `[0, 1)` from the wall clock's sub-second
/// nanoseconds. For backoff jitter only (de-correlation), not security — so it
/// deliberately avoids pulling in an RNG dependency.
fn jitter_fraction() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    f64::from(nanos % 1_000_000_000) / 1_000_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test(start_paused = true)]
    async fn retries_transient_then_succeeds() {
        let calls = AtomicU32::new(0);
        let policy = RetryPolicy {
            max_attempts: 5,
            ..Default::default()
        };
        let result: Result<u32> = retry_transient(policy, || async {
            let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if n < 3 {
                Err(Error::database("temporary"))
            } else {
                Ok(n)
            }
        })
        .await;
        assert_eq!(result.unwrap(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_retry_non_transient() {
        let calls = AtomicU32::new(0);
        let result: Result<u32> = retry_transient(RetryPolicy::default(), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::invalid_input("bad"))
        })
        .await;
        assert!(result.is_err());
        // Invalid input is not transient: exactly one attempt, no retries.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_attempts() {
        let calls = AtomicU32::new(0);
        let policy = RetryPolicy {
            max_attempts: 3,
            ..Default::default()
        };
        let result: Result<u32> = retry_transient(policy, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::database("always failing"))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
