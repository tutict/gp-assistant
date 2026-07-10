use std::{future::Future, sync::OnceLock};

use tokio::sync::{Semaphore, SemaphorePermit};

const CPU_BOUND_PERMITS: usize = 4;
const IO_BOUND_PERMITS: usize = 2;
const HEAVY_NETWORK_PERMITS: usize = 4;
const MARKET_REFRESH_PERMITS: usize = 1;

static CPU_BOUND_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static IO_BOUND_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static HEAVY_NETWORK_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static MARKET_REFRESH_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

fn cpu_bound_semaphore() -> &'static Semaphore {
    CPU_BOUND_SEMAPHORE.get_or_init(|| Semaphore::new(CPU_BOUND_PERMITS))
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
    let _permit = acquire_permit(cpu_bound_semaphore(), label).await?;
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("{label} worker panicked or was cancelled: {error}"))
}

pub(crate) async fn run_io_bound<F, R>(label: &'static str, task: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let _permit = acquire_permit(io_bound_semaphore(), label).await?;
    tauri::async_runtime::spawn_blocking(task)
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
}
