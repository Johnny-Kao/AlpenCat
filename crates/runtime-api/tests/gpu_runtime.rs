use runtime_api::{
    BackendKind, ExecutionMode, GpuWorkGranularity, RangeTaskImplementations, Runtime,
    RuntimeError, TaskDefinition, TaskHandle, WorkRange,
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

fn submit_vector_scale(
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &[f32],
    alpha: f32,
    mode: ExecutionMode,
) -> Result<TaskHandle<Vec<f32>>, RuntimeError> {
    runtime.submit_range_task(
        task,
        WorkRange::new(0, input.len()),
        mode,
        RangeTaskImplementations::new(|index| alpha * input[index])
            .with_gpu(|gpu| gpu.dispatch_f32(VECTOR_SCALE_SHADER, input, &[alpha], 64)),
    )
}

#[test]
fn registered_vector_scale_serial_and_cpu_match() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = vec![1.0_f32, 2.0, 3.0, 4.0, 5.0];
    let alpha = 1.75;

    let serial = submit_vector_scale(&runtime, &task, &input, alpha, ExecutionMode::Serial)
        .expect("serial vector_scale must be available");
    let cpu = submit_vector_scale(&runtime, &task, &input, alpha, ExecutionMode::Cpu)
        .expect("CPU vector_scale must be available");

    assert_eq!(runtime.wait(serial).value, runtime.wait(cpu).value);
}

#[test]
fn forced_gpu_registered_vector_scale_matches_serial_when_gpu_is_available() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input: Vec<f32> = (0..4096).map(|i| i as f32 * 0.25).collect();
    let alpha = 2.5;

    let serial = submit_vector_scale(&runtime, &task, &input, alpha, ExecutionMode::Serial)
        .expect("serial vector_scale must be available");
    let expected = runtime.wait(serial).value;

    let gpu = match submit_vector_scale(&runtime, &task, &input, alpha, ExecutionMode::Gpu) {
        Ok(handle) => handle,
        Err(RuntimeError::BackendUnavailable(BackendKind::Gpu))
            if std::env::var_os("RUNTIME_REQUIRE_GPU").is_none() =>
        {
            return;
        }
        Err(error) => panic!("GPU backend failed unexpectedly: {error:?}"),
    };

    assert_eq!(gpu.decision().backend, BackendKind::Gpu);

    let actual = runtime.wait(gpu).value;
    assert_eq!(actual, expected);
}

#[test]
fn gpu_registration_distinguishes_whole_task_from_range_aware() {
    let whole = RangeTaskImplementations::new(|index: usize| index)
        .with_gpu(|_gpu| Ok::<Vec<usize>, RuntimeError>(Vec::new()));
    assert_eq!(whole.gpu_granularity(), Some(GpuWorkGranularity::WholeTask));
    assert!(!whole.gpu_range_eligible());

    let ranged = RangeTaskImplementations::new(|index: usize| index)
        .with_gpu_range(|_gpu, _range| Ok::<Vec<usize>, RuntimeError>(Vec::new()));
    assert_eq!(
        ranged.gpu_granularity(),
        Some(GpuWorkGranularity::RangeAware)
    );
    assert!(ranged.gpu_range_eligible());
}

#[test]
fn auto_registered_vector_scale_uses_serial_for_small_work() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = vec![1.0_f32, 2.0, 3.0];

    let handle = submit_vector_scale(&runtime, &task, &input, 3.0, ExecutionMode::Auto)
        .expect("auto must preserve the serial fallback");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
}
