use runtime_api::{BackendKind, ExecutionMode, Runtime, RuntimeError, TaskDefinition, WorkRange};

#[test]
fn forced_cpu_vector_scale_runs_through_rayon_adapter() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = [1.0_f32, 2.0, 3.0, 4.0];
    let alpha = 2.5;
    let range = WorkRange::new(0, input.len());

    let handle = runtime
        .submit_map(&task, range, ExecutionMode::Cpu, |index| {
            alpha * input[index]
        })
        .expect("CPU backend must be available in M3");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);

    let result = runtime.wait(handle);

    assert_eq!(result.task_id, "vector_scale");
    assert_eq!(result.decision.backend, BackendKind::Cpu);
    assert_eq!(result.value, vec![2.5, 5.0, 7.5, 10.0]);
}

#[test]
fn cpu_and_serial_map_results_match() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("square");
    let range = WorkRange::new(0, 4096);

    let serial = runtime
        .submit_map(&task, range, ExecutionMode::Serial, |index| index * index)
        .expect("serial backend must be available");
    let cpu = runtime
        .submit_map(&task, range, ExecutionMode::Cpu, |index| index * index)
        .expect("CPU backend must be available");

    assert_eq!(runtime.wait(serial).value, runtime.wait(cpu).value);
}

#[test]
fn gpu_map_backend_remains_explicitly_unavailable() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("noop");
    let range = WorkRange::new(0, 1);

    let gpu = runtime.submit_map(&task, range, ExecutionMode::Gpu, |_| 0_u8);

    assert_eq!(
        gpu.expect_err("GPU is not implemented in M3"),
        RuntimeError::BackendUnavailable(BackendKind::Gpu)
    );
}
