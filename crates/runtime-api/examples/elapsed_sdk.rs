use runtime_api::{
    calibrate_elapsed_map, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode, Runtime,
    RuntimeConfig, TaskDefinition, WorkRange,
};

fn expensive_element(i: usize) -> u64 {
    let mut value = i as u64 + 1;
    for _ in 0..500 {
        value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
    }
    value
}

fn main() {
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(512, usize::MAX),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("elapsed-sdk");
    let range = WorkRange::new(0, 2048);
    let before = runtime
        .submit_map(&task, range, ExecutionMode::Serial, expensive_element)
        .expect("serial route");
    let reference = runtime.wait(before).value;

    let outcome = calibrate_elapsed_map(
        &runtime,
        BoundedRevalidationConfig::new(4, 128, 4096),
        &task,
        expensive_element,
    );
    println!("elapsed calibration status={:?}", outcome.status);
    println!("evidence={:?}", outcome.evidence.measurements);

    let after = runtime
        .submit_map(&task, range, ExecutionMode::Auto, expensive_element)
        .expect("auto route");
    assert_eq!(reference, runtime.wait(after).value);
    println!("output equivalence PASS");
}
