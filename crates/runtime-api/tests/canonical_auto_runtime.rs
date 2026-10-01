use std::time::Duration;

use runtime_api::{
    BackendKind, CostObservation, ExecutionMode, MachineProfile, RangeTaskImplementations, Runtime,
    RuntimeError, TaskDefinition, WorkRange,
};

const VECTOR_SCALE_SHADER: &str = r#"
@group(0) @binding(0)
var<storage, read_write> data: array<f32>;

@group(0) @binding(1)
var<storage, read> params: array<f32>;

@compute @workgroup_size(64)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let row_width = groups.x * 64u;
    let i = gid.x + gid.y * row_width;
    if (i < arrayLength(&data)) {
        data[i] = data[i] * params[0];
    }
}
"#;

#[test]
fn canonical_auto_cpu_uses_control_plane_without_overchunking() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("canonical-auto-cpu").with_cached_policy();
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
    let trace = handle.trace();
    let result = runtime.wait(handle);

    assert_eq!(result.value.len(), range.len());
    assert_eq!(result.value[1234], 2468);
    assert!(
        runtime.cached_policy_count() > 0,
        "M11.5 must participate in the canonical Auto lifecycle"
    );

    let telemetry = runtime.telemetry_snapshot();
    assert_eq!(
        telemetry.cpu.completed + telemetry.serial.completed,
        1,
        "CPU-only Auto should preserve one whole-range execution instead of sequential Rayon chunks"
    );
    assert_eq!(trace.gpu_units, 0);
    assert_eq!(trace.replans, 0);
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
    assert!(
        !machine.gpus.is_empty(),
        "GPU-required test needs a discovered GPU"
    );

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
            RangeTaskImplementations::new(|i| i).with_gpu_range(|_gpu, _work| {
                Err(RuntimeError::BackendExecutionFailed(BackendKind::Gpu))
            }),
        )
        .expect("GPU failure must remain recoverable inside Runtime");

    let trace = handle.trace();
    assert_eq!(handle.decision().fallback_from, Some(BackendKind::Gpu));
    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    assert!(trace.gpu_units >= 1);
    assert!(trace.gpu_failures >= 1);
    assert!(trace.cpu_units >= 1);
    assert!(trace.replans >= 1);

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

#[test]
fn canonical_auto_range_gpu_executes_chunked_registered_work() {
    if std::env::var_os("RUNTIME_REQUIRE_GPU").is_none() {
        return;
    }

    let runtime = Runtime::new();
    let task = TaskDefinition::new("canonical-auto-gpu-success");
    let input: Vec<f32> = (0..65_536).map(|i| i as f32 * 0.25).collect();
    let alpha = 2.5_f32;
    let range = WorkRange::new(0, input.len());
    let machine = runtime.discover_machine_profile();
    assert!(
        !machine.gpus.is_empty(),
        "GPU-required test needs a discovered GPU"
    );

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
            CostObservation::execution(range.len(), Duration::from_micros(150), true),
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
            RangeTaskImplementations::new(|i| alpha * input[i]).with_gpu_range(|gpu, work| {
                gpu.dispatch_f32(
                    VECTOR_SCALE_SHADER,
                    &input[work.begin..work.end],
                    &[alpha],
                    64,
                )
            }),
        )
        .expect("range-aware GPU Auto execution must succeed");

    let trace = handle.trace();
    assert_eq!(handle.decision().backend, BackendKind::Gpu);
    assert_eq!(handle.decision().fallback_from, None);
    assert!(trace.gpu_units > 1);
    assert_eq!(trace.gpu_failures, 0);

    let result = runtime.wait(handle);
    let expected: Vec<f32> = input.iter().map(|value| alpha * *value).collect();
    assert_eq!(result.value, expected);

    let telemetry = runtime.telemetry_snapshot();
    assert!(
        telemetry.gpu.completed > 1,
        "M10/M11 should dispatch multiple GPU WorkUnits through the registered wrapper"
    );
    assert!(runtime.cached_policy_count() > 0);
}
