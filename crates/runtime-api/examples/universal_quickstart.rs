//! Minimal runnable Universal v0.1 integration, with explicit invalidation.
use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode, Runtime,
    RuntimeConfig, TaskDefinition, WorkRange,
};

fn main() {
    let runtime = Runtime::with_config(RuntimeConfig {
        // Illustration only: applications must calibrate their own crossover.
        boundary: BoundaryProfile::new(4_096, usize::MAX),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("universal-v0.1");
    let range = WorkRange::new(0, 2_000);
    let before = runtime
        .submit_map(&task, range, ExecutionMode::Auto, |i| i * 2)
        .expect("route available");
    let reference = runtime.wait(before).value;

    // Application or adapter explicitly reports a resource transition.
    runtime.invalidate_resources();
    assert!(runtime.boundary_is_stale());

    // Illustrative deterministic route costs, NOT real benchmarking.
    let outcome = runtime.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8_192),
        |work_items, backend| match backend {
            BackendKind::Serial => Some(work_items as u64),
            BackendKind::Cpu => Some(1_500),
            BackendKind::Gpu => None,
        },
    );
    println!("revalidation={:?}", outcome.status);

    let after = runtime
        .submit_map(&task, range, ExecutionMode::Auto, |i| i * 2)
        .expect("route available");
    assert_eq!(reference, runtime.wait(after).value);
    println!("Universal v0.1 example: output equivalence PASS");
}
