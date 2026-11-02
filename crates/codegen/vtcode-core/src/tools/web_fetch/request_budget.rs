//! Genuine-request HTTP budgets, scoped privately across registry retries.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use url::Url;

use crate::tools::registry::{ToolErrorType, ToolExecutionError};

#[derive(Default)]
struct OriginAttempts {
    attempts: u32,
    rate_limit: Option<ToolExecutionError>,
    retry_not_before: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct FetchRequestBudget {
    origins: Mutex<HashMap<String, OriginAttempts>>,
    // Redirect callbacks are synchronous. Serialize header acquisition so a
    // concurrent alias cannot send past a newly observed target-origin 429.
    transport: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub(crate) struct RequestBudget {
    shared: Arc<FetchRequestBudget>,
    deadline: Instant,
}

tokio::task_local! {
    static REQUEST_BUDGET: RequestBudget;
}

impl FetchRequestBudget {
    pub(crate) fn clear(&self) {
        self.origins.lock().clear();
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.origins.lock().is_empty()
    }
}

impl RequestBudget {
    pub(crate) fn new(shared: Arc<FetchRequestBudget>, deadline: Instant) -> Self {
        Self { shared, deadline }
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) fn scope<F: Future>(self, future: F) -> impl Future<Output = F::Output> {
        REQUEST_BUDGET.scope(self, future)
    }

    pub(crate) fn current() -> Option<Self> {
        REQUEST_BUDGET.try_with(Clone::clone).ok()
    }

    pub(crate) async fn lock_transport(&self) -> anyhow::Result<tokio::sync::MutexGuard<'_, ()>> {
        tokio::time::timeout_at(tokio::time::Instant::from_std(self.deadline), self.shared.transport.lock())
            .await
            .map_err(|elapsed| {
                let mut error = self.deadline_error();
                error.original_error = Some(elapsed.to_string());
                anyhow::Error::new(error)
            })
    }

    fn deadline_error(&self) -> ToolExecutionError {
        let mut error = ToolExecutionError::new("web_fetch", ToolErrorType::Timeout, "HTTP request deadline expired");
        error.retryable = false;
        error
    }

    /// Called only after permission/security admission, immediately before send.
    pub(crate) fn admit(&self, url: &Url) -> Result<(), Box<ToolExecutionError>> {
        let mut origins = self.shared.origins.lock();
        let state = origins.entry(url.origin().ascii_serialization()).or_default();
        if let Some(error) = &state.rate_limit
            && (state.attempts >= 2 || !error.retryable)
        {
            let mut error = error.clone();
            error.retryable = false;
            return Err(Box::new(error));
        }
        if Instant::now() >= self.deadline {
            if let Some(error) = &state.rate_limit {
                let mut error = error.clone();
                error.retryable = false;
                state.rate_limit = Some(error.clone());
                return Err(Box::new(error));
            }
            return Err(Box::new(self.deadline_error()));
        }
        if let (Some(error), Some(not_before)) = (&state.rate_limit, state.retry_not_before)
            && let Some(remaining) = not_before.checked_duration_since(Instant::now())
        {
            let mut error = error.clone();
            error.retry_after_ms = Some(remaining.as_millis().saturating_add(1).min(u128::from(u64::MAX)) as u64);
            if remaining >= self.deadline.saturating_duration_since(Instant::now()) {
                error.retryable = false;
                state.rate_limit = Some(error.clone());
            }
            return Err(Box::new(error));
        }
        state.attempts = state.attempts.saturating_add(1);
        Ok(())
    }

    /// Record the response origin before another transport can acquire headers.
    pub(crate) fn observe(&self, url: &Url, mut error: ToolExecutionError) -> ToolExecutionError {
        if error.category != vtcode_commons::ErrorCategory::RateLimit {
            return error;
        }
        let mut origins = self.shared.origins.lock();
        let state = origins.entry(url.origin().ascii_serialization()).or_default();
        state.retry_not_before = error.retry_after().and_then(|delay| Instant::now().checked_add(delay));
        if state.attempts >= 2
            || error.retry_after().is_some_and(|delay| {
                delay > Duration::from_secs(5) || delay >= self.deadline.saturating_duration_since(Instant::now())
            })
        {
            error.retryable = false;
        }
        state.rate_limit = Some(error.clone());
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn request_scope_keeps_one_copy_of_the_inner_future() {
        let budget = RequestBudget::new(Arc::default(), Instant::now() + Duration::from_secs(1));
        let mut payload = [0_u8; 8192];
        payload[0] = 3;
        payload[8191] = 7;
        let inner = std::future::ready(payload);
        let inner_size = size_of_val(&inner);
        let scoped = budget.scope(inner);
        assert!(size_of_val(&scoped) <= inner_size + 256, "scope must add only bounded task-local metadata");
        assert_eq!(scoped.await, payload);
    }

    #[tokio::test]
    async fn queued_transport_respects_deadline_and_cancellation_releases_the_permit() {
        let shared = Arc::new(FetchRequestBudget::default());
        let owner = RequestBudget::new(shared.clone(), Instant::now() + Duration::from_secs(2));
        let short = RequestBudget::new(shared.clone(), Instant::now() + Duration::from_millis(10));
        let permit = owner.lock_transport().await.unwrap();
        let error = short.lock_transport().await.unwrap_err();
        assert_eq!(error.downcast_ref::<ToolExecutionError>().unwrap().error_type, ToolErrorType::Timeout);
        assert!(shared.is_empty(), "a queued call has not sent anything");
        drop(permit);
        let cancelled = tokio::time::timeout(Duration::from_millis(10), async {
            let _permit = owner.lock_transport().await.unwrap();
            std::future::pending::<()>().await;
        })
        .await;
        assert!(cancelled.is_err());
        let _permit = tokio::time::timeout(Duration::from_millis(100), owner.lock_transport())
            .await
            .expect("cancellation must release the transport permit")
            .unwrap();
        let expired = short.admit(&Url::parse("https://other.example/resource").unwrap()).unwrap_err();
        assert_eq!(expired.error_type, ToolErrorType::Timeout);
    }
}
