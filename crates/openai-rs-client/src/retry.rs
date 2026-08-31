use std::time::Duration;

/// Retry policy applied before an HTTP response body is delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub(crate) max_retries: u32,
    pub(crate) retry_replayable_mutations: bool,
    pub(crate) max_server_delay: Duration,
}

impl RetryPolicy {
    /// OpenAI-compatible defaults: two retries for replayable requests and a
    /// 120-second upper bound on server-requested delay, matching
    /// openai-python's `MAX_RETRY_AFTER_DELAY`.
    #[must_use]
    pub const fn openai_compatible() -> Self {
        Self {
            max_retries: 2,
            retry_replayable_mutations: true,
            max_server_delay: Duration::from_secs(120),
        }
    }

    /// Retries only operations classified as read-only/safe.
    #[must_use]
    pub const fn conservative() -> Self {
        Self {
            max_retries: 2,
            retry_replayable_mutations: false,
            max_server_delay: Duration::from_secs(60),
        }
    }

    /// Disables automatic retries.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            max_retries: 0,
            retry_replayable_mutations: false,
            max_server_delay: Duration::ZERO,
        }
    }

    /// Sets the number of attempts after the initial request.
    #[must_use]
    pub const fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Sets the maximum honored `Retry-After` delay. A larger server value
    /// falls back to local exponential backoff instead of causing an
    /// unbounded wait.
    #[must_use]
    pub const fn max_server_delay(mut self, max_server_delay: Duration) -> Self {
        self.max_server_delay = max_server_delay;
        self
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::openai_compatible()
    }
}
