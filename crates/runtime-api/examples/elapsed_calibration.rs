//! Real elapsed-time calibration for serial/CPU route comparison.
//!
//! Cold path only: applications provide representative work, not synthetic costs.
//! Measurements include execution overhead and are not universal performance claims.
use std::hint::black_box;
use std::time::Instant;

use runtime_api::{
    BackendKind, BoundedRevalidationConfig, RevalidationStatus, Runtime, RuntimeRevalidationOutcome,
};

/// Measure a representative workload on each backend using monotonic elapsed time.
///
/// `run` must execute the requested route and return `None` when unavailable.
/// This does not automatically invalidate resources; call `invalidate_resources`
/// when a real resource change is observed.
pub fn revalidate_with_elapsed_measurements<F>(
    runtime: &Runtime,
    config: BoundedRevalidationConfig,
    mut run: F,
) -> RuntimeRevalidationOutcome
where
    F: FnMut(usize, BackendKind) -> Option<()>,
{
    runtime.revalidate_serial_cpu(config, |items, backend| {
        let started = Instant::now();
        run(items, backend)?;
        black_box(());
        Some(started.elapsed().as_nanos().min(u64::MAX as u128) as u64)
    })
}

fn main() {
    let runtime = Runtime::new();
    runtime.invalidate_resources();
    let result = revalidate_with_elapsed_measurements(
        &runtime,
        BoundedRevalidationConfig::new(3, 128, 4096),
        |items, backend| {
            let mut checksum = 0usize;
            match backend {
                BackendKind::Serial => {
                    for index in 0..items {
                        checksum = checksum.wrapping_add(black_box(index).wrapping_mul(3));
                    }
                }
                BackendKind::Cpu => {
                    // This example intentionally uses a serial implementation for both routes.
                    // It demonstrates the measurement contract, not a CPU speedup.
                    for index in 0..items {
                        checksum = checksum.wrapping_add(black_box(index).wrapping_mul(3));
                    }
                }
                BackendKind::Gpu => return None,
            }
            black_box(checksum);
            Some(())
        },
    );
    assert!(matches!(
        result.status,
        RevalidationStatus::Published | RevalidationStatus::NoLocalCrossover
    ));
    println!("elapsed-measurement integration: {:?}", result.status);
}
