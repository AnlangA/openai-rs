//! Bounded polling shared by Vector Stores, Fine-tuning, and Evals.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::Notify;

use crate::{ApiResponse, Error};

/// Cooperative cancellation shared with one or more polling futures.
#[derive(Clone, Default)]
pub struct PollCancellationToken {
    inner: Arc<PollCancellationInner>,
}

#[derive(Default)]
struct PollCancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl PollCancellationToken {
    /// Creates an uncancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels every poller holding a clone of this token.
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    /// Returns whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.inner.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

impl std::fmt::Debug for PollCancellationToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PollCancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Interval, deadline, and cancellation controls for resource polling.
#[derive(Clone, Debug)]
pub struct PollOptions {
    pub(crate) interval: Duration,
    pub(crate) timeout: Duration,
    pub(crate) cancellation: Option<PollCancellationToken>,
}

impl PollOptions {
    /// Creates options with a one-second interval and ten-minute deadline.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            interval: Duration::from_secs(1),
            timeout: Duration::from_secs(10 * 60),
            cancellation: None,
        }
    }

    /// Creates options with fine-tuning defaults (5-second interval, 24-hour timeout).
    #[must_use]
    pub const fn for_fine_tuning() -> Self {
        Self {
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(24 * 60 * 60),
            cancellation: None,
        }
    }

    /// Creates options with eval-run defaults (1-second interval, 30-minute timeout).
    #[must_use]
    pub const fn for_eval_runs() -> Self {
        Self {
            interval: Duration::from_secs(1),
            timeout: Duration::from_secs(30 * 60),
            cancellation: None,
        }
    }

    /// Replaces the interval. Zero is rejected when polling starts.
    #[must_use]
    pub const fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Replaces the overall polling deadline. Zero is rejected when polling
    /// starts.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Adds cooperative cancellation.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: PollCancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Polling interval.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        self.interval
    }

    /// Overall polling timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl Default for PollOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Failures produced by a bounded polling helper.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PollError {
    /// Interval and timeout must both be non-zero.
    #[error("poll interval and timeout must be non-zero")]
    InvalidConfiguration,
    /// The caller-provided deadline elapsed.
    #[error("resource polling deadline elapsed")]
    DeadlineExceeded {
        /// Most recent status string reported by the resource before timeout.
        last_status: Option<String>,
    },
    /// Cooperative cancellation was requested.
    #[error("resource polling was cancelled")]
    Cancelled,
    /// A resource retrieval failed.
    #[error(transparent)]
    Client(#[from] Error),
}

pub(crate) trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
    fn cancelled(&self) -> impl Future<Output = ()> + Send;
}

impl Cancellation for PollCancellationToken {
    fn is_cancelled(&self) -> bool {
        self.is_cancelled()
    }

    fn cancelled(&self) -> impl Future<Output = ()> + Send {
        self.cancelled()
    }
}

pub(crate) enum SharedPollError {
    InvalidConfiguration,
    DeadlineExceeded { last_status: Option<String> },
    Cancelled,
    Client(Error),
}

impl From<SharedPollError> for PollError {
    fn from(error: SharedPollError) -> Self {
        match error {
            SharedPollError::InvalidConfiguration => Self::InvalidConfiguration,
            SharedPollError::DeadlineExceeded { last_status } => {
                Self::DeadlineExceeded { last_status }
            }
            SharedPollError::Cancelled => Self::Cancelled,
            SharedPollError::Client(error) => Self::Client(error),
        }
    }
}

pub(crate) async fn poll_until<T, F, Fut, P, S, C>(
    mut fetch: F,
    terminal: P,
    status: S,
    interval: Duration,
    timeout: Duration,
    cancellation: Option<&C>,
) -> Result<ApiResponse<T>, SharedPollError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<ApiResponse<T>, Error>>,
    P: Fn(&T) -> bool,
    S: Fn(&T) -> String,
    C: Cancellation,
{
    if interval.is_zero() || timeout.is_zero() {
        return Err(SharedPollError::InvalidConfiguration);
    }
    let started = Instant::now();
    let mut last_status = None;
    loop {
        if cancellation.is_some_and(|token| token.is_cancelled()) {
            return Err(SharedPollError::Cancelled);
        }
        let remaining = timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| SharedPollError::DeadlineExceeded {
                last_status: last_status.clone(),
            })?;
        let response = if let Some(cancellation) = cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => return Err(SharedPollError::Cancelled),
                response = tokio::time::timeout(remaining, fetch()) => {
                    response.map_err(|_| SharedPollError::DeadlineExceeded {
                        last_status: last_status.clone(),
                    })?
                    .map_err(SharedPollError::Client)?
                }
            }
        } else {
            tokio::time::timeout(remaining, fetch())
                .await
                .map_err(|_| SharedPollError::DeadlineExceeded {
                    last_status: last_status.clone(),
                })?
                .map_err(SharedPollError::Client)?
        };
        last_status = Some(status(response.body()));
        if terminal(response.body()) {
            return Ok(response);
        }

        let remaining = timeout
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| SharedPollError::DeadlineExceeded {
                last_status: last_status.clone(),
            })?;
        let delay = interval.min(remaining);
        if let Some(cancellation) = cancellation {
            tokio::select! {
                _ = cancellation.cancelled() => return Err(SharedPollError::Cancelled),
                () = tokio::time::sleep(delay) => {}
            }
        } else {
            tokio::time::sleep(delay).await;
        }
    }
}

pub(crate) async fn poll_resource_with_status<T, F, Fut, P, S>(
    fetch: F,
    terminal: P,
    status: S,
    options: PollOptions,
) -> Result<ApiResponse<T>, PollError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<ApiResponse<T>, Error>>,
    P: Fn(&T) -> bool,
    S: Fn(&T) -> String,
{
    poll_until(
        fetch,
        terminal,
        status,
        options.interval,
        options.timeout,
        options.cancellation.as_ref(),
    )
    .await
    .map_err(PollError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;

    fn make_response<T>(body: T) -> ApiResponse<T> {
        ApiResponse::new(
            body,
            crate::ResponseMeta::new(StatusCode::OK, None, crate::RateLimitMetadata::default()),
        )
    }

    #[tokio::test]
    async fn poll_preserves_last_status_on_timeout() {
        let fetch = || async { Ok(make_response("in_progress".to_string())) };
        let options = PollOptions::new()
            .with_interval(Duration::from_millis(10))
            .with_timeout(Duration::from_millis(30));
        let error = poll_resource_with_status(fetch, |s| s == "completed", |s| s.clone(), options)
            .await
            .expect_err("should time out");

        match error {
            PollError::DeadlineExceeded { last_status } => {
                assert_eq!(last_status.as_deref(), Some("in_progress"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
