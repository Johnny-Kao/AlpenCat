use runtime_api::{
    BackendKind, ExecutionMode, RangeTaskImplementations, Runtime, TaskDefinition, WorkRange,
    CPU_MAX_ITEMS, SERIAL_MAX_ITEMS,
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

fn registered_vector_scale<'a>(
    input: &'a [f32],
    alpha: f32,
) -> RangeTaskImplementations<'a, f32, impl Fn(usize) -> f32 + Sync + Send + 'a> {
    RangeTaskImplementations::new(move |index| alpha * input[index])
        .with_gpu(move |gpu| gpu.dispatch_f32(VECTOR_SCALE_SHADER, input, &[alpha], 64))
}

#[test]
fn auto_generic_map_uses_serial_for_small_work() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("map");
    let range = WorkRange::new(0, SERIAL_MAX_ITEMS);

    let handle = runtime
        .submit_map(&task, range, ExecutionMode::Auto, |index| index)
        .expect("auto map must execute");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
    assert_eq!(handle.decision().fallback_from, None);
}

#[test]
fn auto_generic_map_uses_cpu_for_medium_and_large_work() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("map");

    for size in [SERIAL_MAX_ITEMS + 1, CPU_MAX_ITEMS + 1] {
        let handle = runtime
            .submit_map(
                &task,
                WorkRange::new(0, size),
                ExecutionMode::Auto,
                |index| index,
            )
            .expect("auto map must execute");

        assert_eq!(handle.decision().backend, BackendKind::Cpu);
        assert_eq!(handle.decision().fallback_from, None);
    }
}

#[test]
fn auto_registered_gpu_task_uses_serial_for_small_work() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = vec![1.0_f32; SERIAL_MAX_ITEMS];

    let handle = runtime
        .submit_range_task(
            &task,
            WorkRange::new(0, input.len()),
            ExecutionMode::Auto,
            registered_vector_scale(&input, 2.0),
        )
        .expect("auto registered task must execute");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
    assert_eq!(handle.decision().fallback_from, None);
}

#[test]
fn auto_registered_gpu_task_uses_cpu_for_medium_work() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = vec![1.0_f32; SERIAL_MAX_ITEMS + 1];

    let handle = runtime
        .submit_range_task(
            &task,
            WorkRange::new(0, input.len()),
            ExecutionMode::Auto,
            registered_vector_scale(&input, 2.0),
        )
        .expect("auto registered task must execute");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    assert_eq!(handle.decision().fallback_from, None);
}

#[test]
fn auto_registered_gpu_task_large_work_uses_gpu_or_observable_cpu_fallback() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = vec![1.0_f32; CPU_MAX_ITEMS + 1];

    let handle = runtime
        .submit_range_task(
            &task,
            WorkRange::new(0, input.len()),
            ExecutionMode::Auto,
            registered_vector_scale(&input, 2.0),
        )
        .expect("auto registered task must execute");

    if std::env::var_os("RUNTIME_REQUIRE_GPU").is_some() {
        assert_eq!(handle.decision().backend, BackendKind::Gpu);
        assert_eq!(handle.decision().fallback_from, None);
    } else {
        match handle.decision().backend {
            BackendKind::Gpu => assert_eq!(handle.decision().fallback_from, None),
            BackendKind::Cpu => {
                assert!(
                    handle.decision().fallback_from.is_none()
                        || handle.decision().fallback_from == Some(BackendKind::Gpu)
                )
            }
            BackendKind::Serial => panic!("large GPU-eligible work must not remain serial"),
        }
    }

    let result = runtime.wait(handle);
    assert!(result.value.iter().all(|value| *value == 2.0));
}
