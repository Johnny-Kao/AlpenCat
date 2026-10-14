//! Optional Linux affinity observer.
//!
//! This adapter observes the process CPU allowance; it is NOT Intel HFI or AMD CPPC.
//! Polling occurs on a separate cold-path thread, never in FastRoute.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::Runtime;

fn allowed_cpus() -> std::io::Result<String> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
        .map(|value| value.trim().to_owned())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Cpus_allowed_list"))
}

/// Owns an observer worker. Dropping it stops and joins the worker.
pub struct LinuxAffinityObserver {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxAffinityObserver {
    /// Start a cold-path watcher. Returns an error if the procfs source is unavailable.
    pub fn start(runtime: Arc<Runtime>, interval: Duration) -> std::io::Result<Self> {
        if interval.is_zero() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "interval must be positive"));
        }
        let initial = allowed_cpus()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("alpencat-affinity".to_owned())
            .spawn(move || {
                let mut previous = initial;
                while !flag.load(Ordering::Acquire) {
                    thread::park_timeout(interval);
                    if flag.load(Ordering::Acquire) {
                        break;
                    }
                    if let Ok(current) = allowed_cpus() {
                        if current != previous {
                            previous = current;
                            runtime.invalidate_resources();
                        }
                    }
                }
            })?;
        Ok(Self { stop, worker: Some(worker) })
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

impl Drop for LinuxAffinityObserver {
    fn drop(&mut self) {
        self.stop();
    }
}
