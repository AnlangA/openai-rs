//! Bounded polling shared by Vector Stores, Files, Fine-tuning, Evals,
//! Batches, and background-mode Responses.

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
///
/// The interval is a pacing *mode*: either the caller pinned an explicit
/// interval through [`PollOptions::with_interval`] or one of the presets that
/// documents one, or the interval stays server-paced and each sleep follows
/// the `openai-poll-after-ms` header of the previous retrieve response when
/// the server sent one (falling back to [`PollOptions::fallback_interval`]
/// when it did not).
#[derive(Clone, Debug)]
pub struct PollOptions {
    /// Pinned interval; `None` keeps the poller server-paced.
    pub(crate) interval: Option<Duration>,
    /// Interval used when the options are neither pinned nor server-hinted.
    pub(crate) fallback_interval: Duration,
    pub(crate) timeout: Duration,
    pub(crate) cancellation: Option<PollCancellationToken>,
}

impl PollOptions {
    /// Creates server-paced options with a one-second fallback interval and
    /// ten-minute deadline.
    ///
    /// Because no interval is pinned, the poller prefers the server's
    /// `openai-poll-after-ms` hint for each sleep and only falls back to the
    /// one-second interval when the response carries no parseable hint.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            interval: None,
            fallback_interval: Duration::from_secs(1),
            timeout: Duration::from_secs(10 * 60),
            cancellation: None,
        }
    }

    /// Creates options with fine-tuning defaults (5-second interval, 24-hour timeout).
    #[must_use]
    pub const fn for_fine_tuning() -> Self {
        Self {
            interval: Some(Duration::from_secs(5)),
            fallback_interval: Duration::from_secs(5),
            timeout: Duration::from_secs(24 * 60 * 60),
            cancellation: None,
        }
    }

    /// Creates options with eval-run defaults (1-second interval, 30-minute timeout).
    #[must_use]
    pub const fn for_eval_runs() -> Self {
        Self {
            interval: Some(Duration::from_secs(1)),
            fallback_interval: Duration::from_secs(1),
            timeout: Duration::from_secs(30 * 60),
            cancellation: None,
        }
    }

    /// Creates options with file-processing defaults (5-second interval, 30-minute timeout).
    ///
    /// Matches the official SDK `wait_for_processing` helpers: uploads
    /// typically finish processing within seconds, but large files can take
    /// minutes, so the window is widened to thirty minutes while the interval
    /// stays pinned at five seconds to keep request volume proportionate.
    /// Consumed by [`crate::files::Files::wait_for_processing`].
    #[must_use]
    pub const fn for_files() -> Self {
        Self {
            interval: Some(Duration::from_secs(5)),
            fallback_interval: Duration::from_secs(5),
            timeout: Duration::from_secs(30 * 60),
            cancellation: None,
        }
    }

    /// Creates options with batch defaults (5-second interval, 24-hour timeout).
    ///
    /// The pinned Batch API accepts exactly one `completion_window` value,
    /// `24h`, so a batch may legitimately take most of a day to finish. The
    /// generic [`PollOptions::new`] deadline of ten minutes therefore expires
    /// structurally before any batch can complete. This preset matches the
    /// 24-hour window and pins a 5-second interval to keep request volume
    /// proportionate to runs measured in hours.
    #[must_use]
    pub const fn for_batches() -> Self {
        Self {
            interval: Some(Duration::from_secs(5)),
            fallback_interval: Duration::from_secs(5),
            timeout: Duration::from_secs(24 * 60 * 60),
            cancellation: None,
        }
    }

    /// Creates server-paced options with Vector Stores defaults.
    ///
    /// Both official SDKs pace Vector Stores polling with the retrieve
    /// response's `openai-poll-after-ms` header whenever the caller did not
    /// pin an interval — openai-python's helpers fall back to 1000 ms
    /// (`src/openai/lib/_vector_stores.py:16-22`) and openai-node's shared
    /// poller to 5000 ms (`src/lib/polling.ts:117-145`). This preset keeps the
    /// interval unpinned so the hint wins and mirrors openai-python's 1000 ms
    /// fallback; a hinted sleep is deliberately uncapped (neither SDK caps
    /// it) apart from the deadline clamp that keeps the final wait inside the
    /// remaining budget.
    ///
    /// Divergence, kept on purpose: both SDKs impose **no** polling deadline
    /// on these helpers, while this preset keeps this crate's default
    /// ten-minute deadline so a wedged store cannot poll forever. Widen the
    /// deadline with [`PollOptions::with_timeout`] for known-slow ingestion
    /// instead of losing it for every caller.
    #[must_use]
    pub const fn for_vector_stores() -> Self {
        Self {
            interval: None,
            fallback_interval: Duration::from_secs(1),
            timeout: Duration::from_secs(10 * 60),
            cancellation: None,
        }
    }

    /// Pins the polling interval, disabling server pacing.
    ///
    /// A pinned interval is honored exactly, including over a server-sent
    /// `openai-poll-after-ms` hint — the same precedence both official SDKs
    /// give an explicit caller interval. Zero is rejected when polling starts.
    #[must_use]
    pub const fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = Some(interval);
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

    /// The pinned polling interval, when the caller set one.
    ///
    /// `None` means the poller is server-paced: it prefers the previous
    /// response's `openai-poll-after-ms` hint and falls back to
    /// [`PollOptions::fallback_interval`] when there is none.
    #[must_use]
    pub const fn interval(&self) -> Option<Duration> {
        self.interval
    }

    /// The interval used when the poller is neither pinned nor server-hinted.
    #[must_use]
    pub const fn fallback_interval(&self) -> Duration {
        self.fallback_interval
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
    interval: Option<Duration>,
    fallback_interval: Duration,
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
    if interval.is_some_and(|interval| interval.is_zero())
        || fallback_interval.is_zero()
        || timeout.is_zero()
    {
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
        // Pacing precedence: an explicitly pinned interval always wins; an
        // unpinned poller prefers the server's `openai-poll-after-ms` hint
        // from the response it just received and only falls back to the
        // configured default interval when the header is absent or does not
        // parse. This mirrors openai-python's Vector Stores poll helpers
        // (`src/openai/lib/_vector_stores.py:16-22`) and openai-node's shared
        // poller (`src/lib/polling.ts:117-145`), which also leave a hinted
        // delay uncapped; the only ceiling here is the deadline clamp that
        // keeps the final wait inside the remaining budget.
        let delay = interval
            .or_else(|| response.meta().poll_after_ms().map(Duration::from_millis))
            .unwrap_or(fallback_interval)
            .min(remaining);
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
        options.fallback_interval,
        options.timeout,
        options.cancellation.as_ref(),
    )
    .await
    .map_err(PollError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, HeaderValue, StatusCode};

    fn make_response<T>(body: T) -> ApiResponse<T> {
        ApiResponse::new(
            body,
            crate::ResponseMeta::new(StatusCode::OK, None, crate::RateLimitMetadata::default()),
        )
    }

    fn make_response_with_poll_hint<T>(body: T, poll_after_ms: &'static str) -> ApiResponse<T> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "openai-poll-after-ms",
            HeaderValue::from_static(poll_after_ms),
        );
        ApiResponse::new(
            body,
            crate::ResponseMeta::from_headers(StatusCode::OK, &headers),
        )
    }

    #[test]
    fn for_batches_preset_covers_the_only_batch_completion_window() {
        let options = PollOptions::for_batches();
        assert_eq!(options.interval(), Some(Duration::from_secs(5)));
        assert_eq!(options.timeout(), Duration::from_secs(24 * 60 * 60));
        assert!(options.cancellation.is_none());

        // The generic defaults cannot cover the pinned `24h` completion window.
        let defaults = PollOptions::new();
        assert_eq!(defaults.interval(), None);
        assert_eq!(defaults.fallback_interval(), Duration::from_secs(1));
        assert_eq!(defaults.timeout(), Duration::from_secs(10 * 60));
    }

    #[test]
    fn for_files_preset_matches_the_official_wait_for_processing_defaults() {
        let options = PollOptions::for_files();
        assert_eq!(options.interval(), Some(Duration::from_secs(5)));
        assert_eq!(options.timeout(), Duration::from_secs(30 * 60));
        assert!(options.cancellation.is_none());

        // File processing is bounded by upload size, not a pinned completion
        // window, so the preset only widens the generic ten-minute deadline.
        let defaults = PollOptions::new();
        assert_eq!(defaults.interval(), None);
        assert_eq!(defaults.fallback_interval(), Duration::from_secs(1));
        assert_eq!(defaults.timeout(), Duration::from_secs(10 * 60));
    }

    #[test]
    fn for_vector_stores_preset_is_server_paced_with_the_python_fallback() {
        // openai-python's Vector Stores poll helpers read
        // `openai-poll-after-ms` with a 1000 ms fallback and no deadline
        // (`src/openai/lib/_vector_stores.py:16-22`); the preset adopts the
        // pacing and fallback but deliberately keeps this crate's ten-minute
        // deadline where python and node impose none.
        let options = PollOptions::for_vector_stores();
        assert_eq!(options.interval(), None);
        assert_eq!(options.fallback_interval(), Duration::from_secs(1));
        assert_eq!(options.timeout(), Duration::from_secs(10 * 60));
        assert!(options.cancellation.is_none());

        // Pinning an interval afterwards opts out of server pacing, exactly
        // like an explicit interval in either official SDK.
        let pinned = options.with_interval(Duration::from_millis(250));
        assert_eq!(pinned.interval(), Some(Duration::from_millis(250)));
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

    /// Records the mocked-clock instant of every fetch so a test can assert
    /// the exact sleep that preceded each one (`start_paused` auto-advances
    /// the clock to each sleep deadline, so the gaps are the slept delays).
    #[derive(Clone, Default)]
    struct FetchClock {
        fetches: std::sync::Arc<std::sync::Mutex<Vec<tokio::time::Instant>>>,
    }

    impl FetchClock {
        fn record(&self) {
            self.fetches
                .lock()
                .expect("fetch clock lock")
                .push(tokio::time::Instant::now());
        }

        fn gap(&self, index: usize) -> Duration {
            let fetches = self.fetches.lock().expect("fetch clock lock");
            fetches[index]
                .checked_duration_since(fetches[index - 1])
                .expect("monotonic mocked clock")
        }
    }

    #[tokio::test(start_paused = true)]
    async fn unpinned_polling_prefers_the_server_hint() {
        let clock = FetchClock::default();
        let fetch = {
            let clock = clock.clone();
            move || {
                clock.record();
                async {
                    Ok(make_response_with_poll_hint(
                        "in_progress".to_string(),
                        "50",
                    ))
                }
            }
        };
        let options = PollOptions::new().with_timeout(Duration::from_secs(60));
        poll_resource_with_status(fetch, |s| s == "completed", |s| s.clone(), options)
            .await
            .expect_err("never terminal");

        // The hinted 50 ms slept between fetch 1 and 2, not the 1 s fallback.
        assert_eq!(clock.gap(1), Duration::from_millis(50));
        assert_eq!(clock.gap(2), Duration::from_millis(50));
    }

    #[tokio::test(start_paused = true)]
    async fn pinned_polling_ignores_the_server_hint() {
        let clock = FetchClock::default();
        let fetch = {
            let clock = clock.clone();
            move || {
                clock.record();
                async {
                    Ok(make_response_with_poll_hint(
                        "in_progress".to_string(),
                        "50",
                    ))
                }
            }
        };
        let options = PollOptions::new()
            .with_interval(Duration::from_millis(25))
            .with_timeout(Duration::from_secs(60));
        poll_resource_with_status(fetch, |s| s == "completed", |s| s.clone(), options)
            .await
            .expect_err("never terminal");

        assert_eq!(clock.gap(1), Duration::from_millis(25));
    }

    #[tokio::test(start_paused = true)]
    async fn unparseable_or_absent_hints_fall_back_to_the_default_interval() {
        for (hint, expected) in [
            (Some("1.5"), Duration::from_secs(1)),
            (Some("soon"), Duration::from_secs(1)),
            (None, Duration::from_secs(1)),
        ] {
            let clock = FetchClock::default();
            let fetch = {
                let clock = clock.clone();
                move || {
                    clock.record();
                    let hint = hint;
                    async move {
                        Ok(match hint {
                            Some(hint) => {
                                make_response_with_poll_hint("in_progress".to_string(), hint)
                            }
                            None => make_response("in_progress".to_string()),
                        })
                    }
                }
            };
            let options = PollOptions::new().with_timeout(Duration::from_secs(60));
            poll_resource_with_status(fetch, |s| s == "completed", |s| s.clone(), options)
                .await
                .expect_err("never terminal");
            assert_eq!(clock.gap(1), expected);
        }
    }
}
