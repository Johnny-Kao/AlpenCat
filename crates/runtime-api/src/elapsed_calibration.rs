//! Cold-path elapsed-time calibration using actual runtime execution.
//!
//! This helper does not claim that timing evidence generalizes beyond the
//! bounded workload neighborhood supplied by the caller.
use std::hint::black_box;
use std::time::Instant;

use crate::{
    BackendKind, BoundedRevalidationConfig, ExecutionMode, RevalidationStatus, Runtime,
    RuntimeRevalidationOutcome, TaskDefinition, WorkRange,
};

/// Execute real serial and CPU map workloads and measure their elapsed time.
///
/// `operation` must be repeatable, side-effect-free, and representative of the
/// caller's production workload. Samples are deliberately not cached.
pub fn revalidate_elapsed_map<T, F>(
    runtime: &Runtime,
    config: BoundedRevalidationConfig,
    task: &TaskDefinition,
    operation: F,
) -> RuntimeRevalidationOutcome
where
    T: Send,
    F: Fn(usize) -> T + Send + Sync,
{
    runtime.revalidate_serial_cpu(config, |items, backend| {
        let mode = match backend {
            BackendKind::Serial => ExecutionMode::Serial,
            BackendKind::Cpu => ExecutionMode::Cpu,
            BackendKind::Gpu => return None,
        };
        let start = Instant::now();
        let handle = runtime
            .submit_map(task, WorkRange::new(0, items), mode, &operation)
            .ok()?;
        if handle.decision().backend != backend {
            return None;
        }
        let result = runtime.wait(handle);
        black_box(result.value);
        Some(start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64)
    })
}

/// Whether elapsed calibration actually published a new boundary.
pub fn published(outcome: &RuntimeRevalidationOutcome) -> bool {
    outcome.status == RevalidationStatus::Published
}
