use runtime_api::{
    BackendKind, ExecutionConstraint, ExecutionMode, ExternalParallelism, Runtime, RuntimeConfig,
    TaskDefinition, WorkRange,
};

#[test]
fn external_parallelism_hint_constrains_cpu_execution() {
    let runtime = Runtime::with_config(RuntimeConfig {
        external_parallelism: ExternalParallelism::active(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, 16_384),
            ExecutionMode::Cpu,
            |index| index,
        )
        .expect("externally nested CPU request must execute safely");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
    assert_eq!(
        handle.decision().constraint,
        Some(ExecutionConstraint::ExternalParallelism)
    );
    assert!(runtime.external_parallelism().active);
}

#[test]
fn external_parallelism_hint_constrains_auto_cpu_selection() {
    let runtime = Runtime::with_config(RuntimeConfig {
        external_parallelism: ExternalParallelism::active(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, 16_384),
            ExecutionMode::Auto,
            |index| index,
        )
        .expect("auto request must execute safely");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
    assert_eq!(
        handle.decision().constraint,
        Some(ExecutionConstraint::ExternalParallelism)
    );
}

#[test]
fn inactive_external_parallelism_does_not_constrain_cpu_execution() {
    let runtime = Runtime::with_config(RuntimeConfig {
        external_parallelism: ExternalParallelism::inactive(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, 16_384),
            ExecutionMode::Cpu,
            |index| index,
        )
        .expect("CPU request must execute");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    assert_eq!(handle.decision().constraint, None);
}

#[test]
fn serial_path_is_unchanged_by_external_parallelism_hint() {
    let runtime = Runtime::with_config(RuntimeConfig {
        external_parallelism: ExternalParallelism::active(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, 32),
            ExecutionMode::Serial,
            |index| index,
        )
        .expect("serial request must execute");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
    assert_eq!(handle.decision().constraint, None);
}
