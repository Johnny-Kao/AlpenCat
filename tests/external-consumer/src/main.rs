use alpencat_resource::{snapshot, Reading};

use runtime_api::{
    calibrate_elapsed_map, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode,
    ResourceNotifier, Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};
use std::sync::Arc;

fn work(i: usize) -> u64 {
    (i as u64).wrapping_mul(37).wrapping_add(11)
}

fn main() {
    let resources = snapshot();
    assert!(resources.completed_at.duration_since(resources.observed_at).is_ok());
    assert!(!matches!(resources.allowed_cpus, Reading::Known(ref v) if v.is_empty()));

    let runtime = Arc::new(Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(256, usize::MAX),
        ..RuntimeConfig::default()
    }));
    let task = TaskDefinition::new("independent-external-consumer");
    let range = WorkRange::new(0, 2048);
    let expected: Vec<_> = (range.begin..range.end).map(work).collect();

    let initial = calibrate_elapsed_map(
        &runtime,
        BoundedRevalidationConfig::new(4, 64, 4096),
        &task,
        work,
    );
    println!("initial_calibration={:?}", initial.status);
    let result = runtime.wait(
        runtime
            .submit_map(&task, range, ExecutionMode::Auto, work)
            .expect("auto execution available"),
    );
    assert_eq!(result.value, expected);

    let notifier = ResourceNotifier::new(Arc::clone(&runtime));
    notifier.notify_changed();
    assert!(runtime.diagnostics().boundary_stale);
    let revalidated = runtime_api::revalidate_elapsed_map(
        &runtime,
        BoundedRevalidationConfig::new(4, 64, 4096),
        &task,
        work,
    );
    println!("revalidation={:?}", revalidated.status);
    let result = runtime.wait(
        runtime
            .submit_map(&task, range, ExecutionMode::Auto, work)
            .expect("auto execution after notification"),
    );
    assert_eq!(result.value, expected);
    println!("EXTERNAL_CONSUMER=PASS");
}
