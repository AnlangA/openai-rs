use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

/// A cloneable cancellation signal for a local MCP execution.
///
/// This token is intentionally independent of a concrete RMCP transport. The
/// [`crate::ResponsesToolExecutor`] implementation decides how to propagate
/// cancellation to its peer.
#[derive(Clone, Default)]
pub struct CancellationToken {
    inner: Arc<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    reason: Mutex<Option<String>>,
    notify: tokio::sync::Notify,
}

impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CancellationToken")
            .field("is_cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl CancellationToken {
    /// Create a token in the active state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Signal cancellation without an application-specific reason.
    pub fn cancel(&self) {
        self.cancel_inner(None);
    }

    /// Signal cancellation with a caller-visible reason.
    pub fn cancel_with_reason(&self, reason: impl Into<String>) {
        self.cancel_inner(Some(reason.into()));
    }

    fn cancel_inner(&self, reason: Option<String>) {
        if let Some(reason) = reason {
            let mut slot = self
                .inner
                .reason
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot.is_none() {
                *slot = Some(reason);
            }
        }
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    /// Return whether cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Clone the first cancellation reason, if one was supplied.
    pub fn reason(&self) -> Option<String> {
        self.inner
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Wait until this token is cancelled.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// Deadline and cancellation settings for one catalog or tool operation.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ExecutionControl {
    timeout: Option<Duration>,
    cancellation: Option<CancellationToken>,
}

impl ExecutionControl {
    /// Create control settings with no deadline or cancellation signal.
    pub const fn unbounded() -> Self {
        Self {
            timeout: None,
            cancellation: None,
        }
    }

    /// Set an overall operation timeout.
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Attach a cooperative cancellation signal.
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Return the configured timeout.
    pub const fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Return the configured cancellation token.
    pub const fn cancellation(&self) -> Option<&CancellationToken> {
        self.cancellation.as_ref()
    }
}

#[cfg(feature = "client")]
pub(crate) async fn wait_for_cancellation(token: Option<&CancellationToken>) {
    match token {
        Some(token) => token.cancelled().await,
        None => std::future::pending().await,
    }
}

#[cfg(feature = "client")]
pub(crate) async fn wait_for_timeout(timeout: Option<Duration>) {
    match timeout {
        Some(timeout) => tokio::time::sleep(timeout).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_is_cloneable_and_remembers_first_reason() {
        let token = CancellationToken::new();
        let waiter = token.clone();
        let task = tokio::spawn(async move {
            waiter.cancelled().await;
            waiter.reason()
        });

        token.cancel_with_reason("caller stopped");
        token.cancel_with_reason("later reason");

        assert!(token.is_cancelled());
        assert_eq!(task.await.ok().flatten().as_deref(), Some("caller stopped"));
    }
}
