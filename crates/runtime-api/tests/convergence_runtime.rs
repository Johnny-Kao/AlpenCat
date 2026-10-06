use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionBudget, ExecutionMode,
    RevalidationStatus, Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};

#[test]
fn stale_boundary_revalidates_and_changes_real_execution_route() {
    if std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
        < 2
    {
        return;
    }

    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4_096, 262_144),
        execution_budget: ExecutionBudget::new(2),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("convergence-map");
    let range = WorkRange::new(0, 2_000);

    let before = runtime
        .submit_map(&task, range, ExecutionMode::Auto, |index| index * 3 + 1)
        .expect("initial route must execute");
    assert_eq!(before.decision().backend, BackendKind::Serial);

    runtime.invalidate_resources();
    assert!(runtime.boundary_is_stale());

    let outcome = runtime.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8_192),
        |work_items, backend| match backend {
            BackendKind::Serial => work_items as u64,
            BackendKind::Cpu => 1_500,
            BackendKind::Gpu => unreachable!(),
        },
    );

    assert_eq!(outcome.status, RevalidationStatus::Published);
    assert_eq!(
        runtime.boundary_snapshot().profile,
        BoundaryProfile::new(1_024, 262_144)
    );
    assert!(!runtime.boundary_is_stale());

    let after = runtime
        .submit_map(&task, range, ExecutionMode::Auto, |index| index * 3 + 1)
        .expect("updated route must execute");

    assert_eq!(after.decision().backend, BackendKind::Cpu);
    assert_eq!(runtime.wait(before).value, runtime.wait(after).value);
}

#[test]
fn bounded_failure_keeps_boundary_stale() {
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(1_024, 262_144),
        ..RuntimeConfig::default()
    });

    runtime.invalidate_resources();

    let outcome = runtime.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 256, 8_192),
        |_work_items, backend| match backend {
            BackendKind::Serial => 100,
            BackendKind::Cpu => 200,
            BackendKind::Gpu => unreachable!(),
        },
    );

    assert_eq!(outcome.status, RevalidationStatus::NoLocalCrossover);
    assert!(runtime.boundary_is_stale());
    assert_eq!(
        runtime.boundary_snapshot().profile,
        BoundaryProfile::new(1_024, 262_144)
    );
}

#[test]
fn resource_change_during_measurement_cannot_publish_fresh_state() {
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4_096, 262_144),
        ..RuntimeConfig::default()
    });

    runtime.invalidate_resources();
    let epoch_before = runtime.resource_epoch();
    let mut invalidated_again = false;

    let outcome = runtime.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8_192),
        |work_items, backend| {
            if !invalidated_again {
                runtime.invalidate_resources();
                invalidated_again = true;
            }
            match backend {
                BackendKind::Serial => work_items as u64,
                BackendKind::Cpu => 1_500,
                BackendKind::Gpu => unreachable!(),
            }
        },
    );

    assert_eq!(
        outcome.status,
        RevalidationStatus::InvalidatedDuringMeasurement
    );
    assert_eq!(runtime.resource_epoch(), epoch_before + 1);
    assert!(runtime.boundary_is_stale());
    assert_eq!(
        runtime.boundary_snapshot().profile,
        BoundaryProfile::new(4_096, 262_144)
    );
}
