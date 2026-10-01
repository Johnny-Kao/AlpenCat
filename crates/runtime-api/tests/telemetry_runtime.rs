use runtime_api::{
    BackendKind, ExecutionBudget, ExecutionMode, RangeTaskImplementations, Runtime, RuntimeConfig,
    RuntimeError, TaskDefinition, WorkRange,
};

#[test]
fn serial_execution_updates_runtime_telemetry() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("telemetry-serial");
    let range = WorkRange::new(0, 128);

    runtime
        .submit_map(&task, range, ExecutionMode::Serial, |index| index)
        .expect("serial telemetry task must execute");

    let snapshot = runtime.telemetry_snapshot();
    assert_eq!(snapshot.serial.in_flight, 0);
    assert_eq!(snapshot.serial.completed, 1);
    assert_eq!(snapshot.serial.failed, 0);
    assert_eq!(snapshot.serial.work_items, 128);
    assert_eq!(snapshot.serial.last_work_items, 128);
    assert!(snapshot.serial.last_elapsed_nanos > 0);
    assert!(snapshot.serial.last_items_per_second().is_some());
}

#[test]
fn forced_cpu_execution_updates_cpu_telemetry() {
    let runtime = Runtime::with_config(RuntimeConfig {
        execution_budget: ExecutionBudget::new(2),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("telemetry-cpu");
    let range = WorkRange::new(0, 4096);

    let handle = runtime
        .submit_map(&task, range, ExecutionMode::Cpu, |index| index)
        .expect("CPU telemetry task must execute");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    let snapshot = runtime.telemetry_snapshot();
    assert_eq!(snapshot.cpu.in_flight, 0);
    assert_eq!(snapshot.cpu.completed, 1);
    assert_eq!(snapshot.cpu.failed, 0);
    assert_eq!(snapshot.cpu.work_items, 4096);
}

#[test]
fn failed_gpu_attempt_is_observable() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("telemetry-gpu-failure");
    let range = WorkRange::new(0, 64);

    let result = runtime.submit_range_task(
        &task,
        range,
        ExecutionMode::Gpu,
        RangeTaskImplementations::new(|index| index),
    );

    assert!(matches!(
        result,
        Err(RuntimeError::BackendUnavailable(BackendKind::Gpu))
    ));

    let snapshot = runtime.telemetry_snapshot();
    assert_eq!(snapshot.gpu.in_flight, 0);
    assert_eq!(snapshot.gpu.completed, 0);
    assert_eq!(snapshot.gpu.failed, 1);
    assert_eq!(snapshot.gpu.work_items, 64);
}

#[test]
fn resource_snapshot_combines_current_host_and_runtime_state() {
    let runtime = Runtime::new();
    let before = runtime.resource_snapshot();
    assert!(before.host.logical_cpus >= 1);
    assert_eq!(before.telemetry.serial.completed, 0);

    let task = TaskDefinition::new("resource-snapshot");
    runtime
        .submit_map(
            &task,
            WorkRange::new(0, 32),
            ExecutionMode::Serial,
            |index| index,
        )
        .expect("serial resource snapshot task must execute");

    let after = runtime.resource_snapshot();
    assert_eq!(after.telemetry.serial.completed, 1);
    assert_eq!(after.telemetry.serial.last_work_items, 32);
}
