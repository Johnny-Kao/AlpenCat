use std::time::Duration;

use runtime_api::{
    BackendKind, CostObservation, ExecutionMode, MachineProfile, RangeTaskImplementations, Runtime,
    RuntimeError, TaskDefinition, WorkRange,
};

#[test]
fn canonical_auto_uses_cached_policy_and_chunked_runtime_path() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("canonical-auto-cpu");
    let range = WorkRange::new(0, 65_536);
    let machine = MachineProfile::host_only();

    for _ in 0..8 {
        runtime.record_detailed_cost_observation(
            &task,
            BackendKind::Serial,
            &machine,
            CostObservation::execution(range.len(), Duration::from_micros(800), true),
        );
        runtime.record_detailed_cost_observation(
            &task,
            BackendKind::Cpu,
            &machine,
            CostObservation::execution(range.len(), Duration::from_micros(200), true),
        );
    }

    let handle = runtime
        .submit_range_task(
            &task,
            range,
            ExecutionMode::Auto,
            RangeTaskImplementations::new(|i| i * 2),
        )
        .expect("canonical Auto execution must succeed");
    let result = runtime.wait(handle);

    assert_eq!(result.value.len(), range.len());
    assert_eq!(result.value[1234], 2468);
    assert!(
        runtime.cached_policy_count() > 0,
        "M11.5 must participate in the canonical Auto lifecycle"
    );

    let telemetry = runtime.telemetry_snapshot();
    assert!(
        telemetry.cpu.completed + telemetry.serial.completed > 1,
        "M10/M11 integration should execute more than one planned WorkUnit"
    );
}

#[test]
fn canonical_auto_range_gpu_failure_replans_and_falls_back() {
    if std::env::var_os("RUNTIME_REQUIRE_GPU").is_none() {
        return;
    }

    let runtime = Runtime::new();
    let task = TaskDefinition::new("canonical-auto-gpu-failure");
    let range = WorkRange::new(0, 65_536);
    let machine = runtime.discover_machine_profile();
    assert!(!machine.gpus.is_empty(), "GPU-required test needs a discovered GPU");

    for _ in 0..8 {
        runtime.record_detailed_cost_observation(
            &task,
            BackendKind::Serial,
            &machine,
            CostObservation::execution(range.len(), Duration::from_micros(1_000), true),
        );
        runtime.record_detailed_cost_observation(
            &task,
            BackendKind::Cpu,
            &machine,
            CostObservation::execution(range.len(), Duration::from_micros(120), true),
        );
        runtime.record_detailed_cost_observation(
            &task,
            BackendKind::Gpu,
            &machine,
            CostObservation::execution(range.len(), Duration::from_micros(100), true),
        );
    }

    let handle = runtime
        .submit_range_task(
            &task,
            range,
            ExecutionMode::Auto,
            RangeTaskImplementations::new(|i| i)
                .with_gpu_range(|_gpu, _work| Err(RuntimeError::BackendExecutionFailed(BackendKind::Gpu))),
        )
        .expect("GPU failure must remain recoverable inside Runtime");

    assert_eq!(handle.decision().fallback_from, Some(BackendKind::Gpu));
    assert_eq!(handle.decision().backend, BackendKind::Cpu);

    let result = runtime.wait(handle);
    assert_eq!(result.value.len(), range.len());
    assert_eq!(result.value[4096], 4096);

    let telemetry = runtime.telemetry_snapshot();
    assert!(telemetry.gpu.failed >= 1);
    assert!(
        telemetry.cpu.completed >= 1,
        "pending work should continue on CPU after the GPU failure triggers replanning"
    );
}
