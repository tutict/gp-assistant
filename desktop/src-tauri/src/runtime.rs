use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, OnceLock,
    },
};

#[derive(Default)]
pub(crate) struct BlockingWork {
    active: AtomicUsize,
    idle: tokio::sync::Notify,
}
impl BlockingWork {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire) != 0
    }
    pub(crate) async fn wait_idle(&self) {
        while self.is_active() {
            self.idle.notified().await;
        }
    }
    fn begin(self: &Arc<Self>) -> BlockingGuard {
        self.active.fetch_add(1, Ordering::AcqRel);
        BlockingGuard(Arc::clone(self))
    }
}
struct BlockingGuard(Arc<BlockingWork>);
impl Drop for BlockingGuard {
    fn drop(&mut self) {
        if self.0.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.idle.notify_one();
        }
    }
}
tokio::task_local! { static BLOCKING_WORK: Arc<BlockingWork>; }
pub(crate) async fn with_blocking_work<F: Future>(work: Arc<BlockingWork>, future: F) -> F::Output {
    BLOCKING_WORK.scope(work, future).await
}

use tokio::sync::{Semaphore, SemaphorePermit};

const CPU_BOUND_PERMITS: usize = if cfg!(target_os = "android") { 2 } else { 4 };
const IO_BOUND_PERMITS: usize = if cfg!(target_os = "android") { 1 } else { 2 };
const HEAVY_NETWORK_PERMITS: usize = if cfg!(target_os = "android") { 2 } else { 8 };
const MARKET_REFRESH_PERMITS: usize = 1;

static CPU_BOUND_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static IO_BOUND_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static HEAVY_NETWORK_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static MARKET_REFRESH_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static TRANSPORT_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static USER_STATE_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

fn cpu_bound_semaphore() -> &'static Arc<Semaphore> {
    CPU_BOUND_SEMAPHORE.get_or_init(|| Arc::new(Semaphore::new(CPU_BOUND_PERMITS)))
}

fn io_bound_semaphore() -> &'static Semaphore {
    IO_BOUND_SEMAPHORE.get_or_init(|| Semaphore::new(IO_BOUND_PERMITS))
}
fn heavy_network_semaphore() -> &'static Semaphore {
    HEAVY_NETWORK_SEMAPHORE.get_or_init(|| Semaphore::new(HEAVY_NETWORK_PERMITS))
}

fn market_refresh_semaphore() -> &'static Semaphore {
    MARKET_REFRESH_SEMAPHORE.get_or_init(|| Semaphore::new(MARKET_REFRESH_PERMITS))
}

async fn acquire_permit(
    semaphore: &'static Semaphore,
    label: &str,
) -> Result<SemaphorePermit<'static>, String> {
    semaphore
        .acquire()
        .await
        .map_err(|_| format!("{label} concurrency limiter is closed"))
}

pub(crate) async fn run_cpu_bound<F, R>(label: &'static str, task: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    run_cpu_bound_on(Arc::clone(cpu_bound_semaphore()), label, task).await
}

async fn run_cpu_bound_on<F, R>(
    lane: Arc<Semaphore>,
    label: &'static str,
    task: F,
) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let permit = lane
        .acquire_owned()
        .await
        .map_err(|_| format!("{label} concurrency limiter is closed"))?;
    let work = BLOCKING_WORK.try_with(|work| work.begin()).ok();
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        // Dropping the async waiter cannot abort an already running blocking task.
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| format!("{label} worker panicked or was cancelled: {error}"))
}

pub(crate) async fn run_io_bound<F, R>(label: &'static str, task: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let permit = acquire_permit(io_bound_semaphore(), label).await?;
    let work = BLOCKING_WORK.try_with(|work| work.begin()).ok();
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        // Dropping the async waiter cannot abort an already running blocking task.
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| format!("{label} worker panicked or was cancelled: {error}"))
}
/// Network fallbacks must not occupy the lane used to durably save drafts.
pub(crate) async fn run_transport_bound<F, R>(label: &'static str, task: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    run_dedicated_blocking(
        TRANSPORT_SEMAPHORE.get_or_init(|| Semaphore::new(HEAVY_NETWORK_PERMITS)),
        label,
        task,
    )
    .await
}
pub(crate) async fn run_user_state_bound<F, R>(label: &'static str, task: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    run_dedicated_blocking(
        USER_STATE_SEMAPHORE.get_or_init(|| Semaphore::new(IO_BOUND_PERMITS)),
        label,
        task,
    )
    .await
}
async fn run_dedicated_blocking<F, R>(
    lane: &'static Semaphore,
    label: &'static str,
    task: F,
) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let permit = acquire_permit(lane, label).await?;
    let work = BLOCKING_WORK.try_with(|work| work.begin()).ok();
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let _work = work;
        task()
    })
    .await
    .map_err(|error| format!("{label} worker panicked or was cancelled: {error}"))
}
pub(crate) async fn with_heavy_network_permit<F, R>(
    label: &'static str,
    future: F,
) -> Result<R, String>
where
    F: Future<Output = Result<R, String>>,
{
    let _permit = acquire_permit(heavy_network_semaphore(), label).await?;
    future.await
}

pub(crate) async fn with_market_refresh_permit<F, R>(
    label: &'static str,
    future: F,
) -> Result<R, String>
where
    F: Future<Output = Result<R, String>>,
{
    let _permit = acquire_permit(market_refresh_semaphore(), label).await?;
    future.await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    fn run_cpu_test(scenario: impl Future<Output = ()>) {
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(Duration::from_secs(10), scenario)
                .await
                .expect("CPU test scenario exceeded 10 seconds");
        });
    }

    #[test]
    fn market_refresh_permit_serializes_work() {
        tauri::async_runtime::block_on(async {
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            let mut handles = Vec::new();

            for _ in 0..3 {
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                handles.push(tauri::async_runtime::spawn(async move {
                    with_market_refresh_permit("test market refresh", async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok::<_, String>(())
                    })
                    .await
                    .expect("permit should be available");
                }));
            }

            for handle in handles {
                handle.await.expect("task should complete");
            }
            assert_eq!(peak.load(Ordering::SeqCst), 1);
        });
    }
    #[test]
    fn aborting_waiter_does_not_release_running_worker_permit() {
        run_cpu_test(async {
            let lane = Arc::new(Semaphore::new(1));
            let work = Arc::new(BlockingWork::default());
            let worker_lane = Arc::clone(&lane);
            let worker_work = Arc::clone(&work);
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (finish_tx, finish_rx) = std::sync::mpsc::channel();
            let task = tauri::async_runtime::spawn(async move {
                with_blocking_work(
                    worker_work,
                    run_cpu_bound_on(worker_lane, "permit cancellation", move || {
                        started_tx.send(()).unwrap();
                        finish_rx.recv().unwrap();
                    }),
                )
                .await
            });
            started_rx.await.unwrap();
            task.abort();
            assert!(task.await.is_err(), "waiter should be cancelled");
            assert_eq!(
                lane.available_permits(),
                0,
                "blocking worker must own its permit until it actually exits"
            );
            assert!(work.is_active(), "running worker must stay tracked");

            finish_tx.send(()).unwrap();
            work.wait_idle().await;
            assert_eq!(lane.available_permits(), 1);
            assert_eq!(
                run_cpu_bound_on(Arc::clone(&lane), "subsequent worker", || 7).await,
                Ok(7)
            );
            assert_eq!(lane.available_permits(), 1);
        });
    }

    #[test]
    fn cancelling_pending_cpu_acquire_does_not_start_worker_or_hold_permit() {
        run_cpu_test(async {
            let lane = Arc::new(Semaphore::new(1));
            let held_permit = Arc::clone(&lane).acquire_owned().await.unwrap();
            let work = Arc::new(BlockingWork::default());
            let executions = Arc::new(AtomicUsize::new(0));
            let worker_executions = Arc::clone(&executions);
            let mut pending = Box::pin(with_blocking_work(
                Arc::clone(&work),
                run_cpu_bound_on(Arc::clone(&lane), "pending cancellation", move || {
                    worker_executions.fetch_add(1, Ordering::SeqCst);
                }),
            ));

            assert!(futures::poll!(pending.as_mut()).is_pending());
            assert!(!work.is_active(), "queued work must not be marked running");
            assert_eq!(executions.load(Ordering::SeqCst), 0);
            drop(pending);
            drop(held_permit);

            assert_eq!(lane.available_permits(), 1);
            assert_eq!(
                run_cpu_bound_on(Arc::clone(&lane), "after pending cancellation", || 11).await,
                Ok(11)
            );
            assert_eq!(executions.load(Ordering::SeqCst), 0);
            assert!(!work.is_active());
            assert_eq!(lane.available_permits(), 1);
        });
    }

    #[test]
    fn closed_cpu_lane_does_not_start_worker() {
        run_cpu_test(async {
            let lane = Arc::new(Semaphore::new(1));
            lane.close();
            let work = Arc::new(BlockingWork::default());
            let result = with_blocking_work(
                Arc::clone(&work),
                run_cpu_bound_on(lane, "closed CPU", || panic!("worker must not start")),
            )
            .await;

            assert_eq!(
                result,
                Err("closed CPU concurrency limiter is closed".into())
            );
            assert!(!work.is_active());
        });
    }

    #[test]
    fn panicking_cpu_worker_releases_permit_and_tracking() {
        run_cpu_test(async {
            let lane = Arc::new(Semaphore::new(1));
            let work = Arc::new(BlockingWork::default());
            let result = with_blocking_work(
                Arc::clone(&work),
                run_cpu_bound_on(Arc::clone(&lane), "panicking CPU", || {
                    panic!("test worker panic")
                }),
            )
            .await;

            assert!(result
                .unwrap_err()
                .starts_with("panicking CPU worker panicked or was cancelled: "));
            assert!(!work.is_active());
            assert_eq!(lane.available_permits(), 1);
            assert_eq!(
                run_cpu_bound_on(Arc::clone(&lane), "after panic", || 13).await,
                Ok(13)
            );
            assert_eq!(lane.available_permits(), 1);
        });
    }
}
