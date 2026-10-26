use super::{
    provider::{
        DataProvider, ProviderBatch, ProviderCapability, ProviderContext, ProviderError,
        ProviderFuture,
    },
    RefreshRequest,
};
use std::{
    collections::HashMap,
    future::Future,
    sync::OnceLock,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

struct CancellationInner {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

#[derive(Clone)]
pub(crate) struct CancellationToken(Arc<CancellationInner>);

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self(Arc::new(CancellationInner {
            cancelled: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }))
    }

    pub(crate) fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notify.notify_waiters();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        loop {
            let notified = self.0.notify.notified();
            tokio::pin!(notified);
            // Register before checking the atomic flag so notify_waiters cannot be lost
            // between the check and await when multiple provider futures are waiting.
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryPolicy {
    pub max_attempts: u8,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(300),
        }
    }
}

impl RetryPolicy {
    pub(crate) fn delay_for(self, failed_attempt: u8, error: &ProviderError) -> Duration {
        error
            .retry_after()
            .unwrap_or_else(|| {
                self.initial_delay.saturating_mul(
                    1_u32
                        .checked_shl(failed_attempt.saturating_sub(1) as u32)
                        .unwrap_or(u32::MAX),
                )
            })
            .min(self.max_delay)
            .min(Duration::from_secs(300))
    }
}

pub(crate) async fn execute_with_retry<T, F, Fut>(
    operation: F,
    policy: RetryPolicy,
    cancellation: CancellationToken,
) -> Result<T, ProviderError>
where
    F: FnMut(u8) -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    execute_with_retry_using(operation, policy, cancellation, tokio::time::sleep).await
}

pub(crate) async fn execute_with_retry_using<T, F, Fut, S, SF>(
    mut operation: F,
    policy: RetryPolicy,
    cancellation: CancellationToken,
    mut sleep: S,
) -> Result<T, ProviderError>
where
    F: FnMut(u8) -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
    S: FnMut(Duration) -> SF,
    SF: Future<Output = ()>,
{
    // Callers may tune test delays, but a run always obeys the production three-attempt cap.
    let max_attempts = policy.max_attempts.clamp(1, 3);
    for attempt in 1..=max_attempts {
        if cancellation.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(ProviderError::cancelled()),
            result = operation(attempt) => result,
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) if !error.retryable() || attempt == max_attempts => return Err(error),
            Err(error) => {
                if cancellation.is_cancelled() {
                    return Err(ProviderError::cancelled());
                }
                let delay = policy.delay_for(attempt, &error);
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(ProviderError::cancelled()),
                    _ = sleep(delay) => {}
                }
            }
        }
    }
    Err(ProviderError::Other {
        message: "retry loop ended without a result".into(),
    })
}

pub(crate) struct ProviderLimiter {
    max_per_source: usize,
    semaphores: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl ProviderLimiter {
    pub(crate) fn new(max_per_source: usize) -> Self {
        Self {
            max_per_source: max_per_source.max(1),
            semaphores: Mutex::new(HashMap::new()),
        }
    }

    async fn acquire(
        &self,
        source_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<OwnedSemaphorePermit, ProviderError> {
        let semaphore = {
            let mut semaphores = self.semaphores.lock().map_err(|_| ProviderError::Other {
                message: "provider limiter lock is unavailable".into(),
            })?;
            semaphores
                .entry(source_id.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.max_per_source)))
                .clone()
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(ProviderError::cancelled()),
            permit = semaphore.acquire_owned() => permit.map_err(|_| ProviderError::Other {
                message: "provider concurrency limiter is closed".into(),
            }),
        }
    }
}

pub(crate) struct RefreshCoordinator {
    limiter: ProviderLimiter,
    retry_policy: RetryPolicy,
}

static SHARED_REFRESH_COORDINATOR: OnceLock<RefreshCoordinator> = OnceLock::new();

pub(crate) fn shared_refresh_coordinator() -> &'static RefreshCoordinator {
    SHARED_REFRESH_COORDINATOR.get_or_init(|| RefreshCoordinator::new(4, RetryPolicy::default()))
}

impl RefreshCoordinator {
    pub(crate) fn new(max_per_source: usize, retry_policy: RetryPolicy) -> Self {
        Self {
            limiter: ProviderLimiter::new(max_per_source),
            retry_policy,
        }
    }

    /// Fetching never holds the SQLite store. The caller commits each validated provider
    /// batch after this future returns, so cache-only reads stay independent of upstream I/O.
    pub(crate) async fn fetch<P: DataProvider + ?Sized>(
        &self,
        provider: &P,
        request: &RefreshRequest,
        context: &ProviderContext,
    ) -> Result<ProviderBatch, ProviderError> {
        let capability = provider
            .capabilities()
            .iter()
            .find(|capability: &&ProviderCapability| {
                capability.domain == request.domain && capability.market == request.market
            })
            .ok_or_else(|| ProviderError::quality("provider does not cover requested scope"))?;
        let _permit = self
            .limiter
            .acquire(provider.source_id(), &context.cancellation)
            .await?;
        let batch = execute_with_retry(
            |_| async {
                match tokio::time::timeout(context.timeout, provider.fetch(request, context)).await
                {
                    Ok(result) => result,
                    Err(_) => Err(ProviderError::timeout(format!(
                        "provider {} exceeded its {} ms request timeout",
                        provider.source_id(),
                        context.timeout.as_millis()
                    ))),
                }
            },
            self.retry_policy,
            context.cancellation.clone(),
        )
        .await?;
        batch.validate(
            request,
            capability,
            context.max_response_bytes,
            context.max_records,
        )?;
        Ok(batch)
    }
}

#[allow(dead_code)]
fn _provider_future_typecheck(_: ProviderFuture<'_>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_data::provider::ProviderError;
    use std::{
        future::ready,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    #[test]
    fn retry_uses_three_attempts_with_bounded_exponential_backoff() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.delay_for(1, &ProviderError::timeout("timeout")),
            Duration::from_millis(500)
        );
        assert_eq!(
            policy.delay_for(2, &ProviderError::timeout("timeout")),
            Duration::from_secs(1)
        );
        assert_eq!(policy.max_attempts, 3);

        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let result: Result<&'static str, _> =
            tauri::async_runtime::block_on(execute_with_retry_using(
                move |_| {
                    let attempt = observed.fetch_add(1, Ordering::SeqCst) + 1;
                    ready(if attempt < 3 {
                        Err(ProviderError::timeout("timeout"))
                    } else {
                        Ok("ok")
                    })
                },
                policy,
                CancellationToken::new(),
                |_| ready(()),
            ));
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn permanent_failure_is_not_retried() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let result: Result<(), _> = tauri::async_runtime::block_on(execute_with_retry_using(
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                ready(Err(ProviderError::invalid_payload("bad response")))
            },
            RetryPolicy::default(),
            CancellationToken::new(),
            |_| ready(()),
        ));
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancellation_prevents_starting_the_next_attempt() {
        let cancellation = CancellationToken::new();
        let to_cancel = cancellation.clone();
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let result: Result<(), _> = tauri::async_runtime::block_on(execute_with_retry_using(
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                to_cancel.cancel();
                ready(Err(ProviderError::timeout("timeout")))
            },
            RetryPolicy::default(),
            cancellation,
            |_| ready(()),
        ));
        assert!(matches!(result, Err(ProviderError::Cancelled { .. })));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
