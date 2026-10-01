use runtime_api::{BackendKind, ExecutionMode, Runtime, RuntimeError, TaskDefinition, WorkRange};

fn vector_scale(input: &[f32], alpha: f32, range: WorkRange) -> Vec<f32> {
    input[range.begin..range.end]
        .iter()
        .map(|value| alpha * value)
        .collect()
}

#[test]
fn serial_vector_scale_runs_through_submit_wait() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let input = vec![1.0_f32, 2.0, 3.0, 4.0];
    let alpha = 2.5;
    let range = WorkRange::new(0, input.len());

    let handle = runtime
        .submit(&task, range, ExecutionMode::Serial, |work| {
            vector_scale(&input, alpha, work)
        })
        .expect("serial backend must be available");

    assert_eq!(handle.decision().backend, BackendKind::Serial);

    let result = runtime.wait(handle);

    assert_eq!(result.task_id, "vector_scale");
    assert_eq!(result.decision.backend, BackendKind::Serial);
    assert_eq!(result.value, vec![2.5, 5.0, 7.5, 10.0]);
}

#[test]
fn auto_falls_back_to_serial_in_m2() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("identity");
    let range = WorkRange::new(2, 5);

    let handle = runtime
        .submit(&task, range, ExecutionMode::Auto, |work| work.len())
        .expect("auto must have the serial fallback");

    let result = runtime.wait(handle);

    assert_eq!(result.decision.backend, BackendKind::Serial);
    assert_eq!(result.value, 3);
}

#[test]
fn unavailable_backends_are_explicit() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("noop");
    let range = WorkRange::new(0, 1);

    let cpu = runtime.submit(&task, range, ExecutionMode::Cpu, |_| ());
    let gpu = runtime.submit(&task, range, ExecutionMode::Gpu, |_| ());

    assert_eq!(
        cpu.expect_err("CPU is not implemented in M2"),
        RuntimeError::BackendUnavailable(BackendKind::Cpu)
    );
    assert_eq!(
        gpu.expect_err("GPU is not implemented in M2"),
        RuntimeError::BackendUnavailable(BackendKind::Gpu)
    );
}
