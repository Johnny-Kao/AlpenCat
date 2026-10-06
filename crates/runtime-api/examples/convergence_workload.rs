use std::hint::black_box;
use std::time::Instant;

use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionBudget, ExecutionMode,
    Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};

fn kernel(mut x: u64) -> u64 {
    for _ in 0..128 {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    }
    x
}

fn execute(
    runtime: &Runtime,
    task: &TaskDefinition,
    n: usize,
    mode: ExecutionMode,
) -> Vec<u64> {
    runtime
        .wait(
            runtime
                .submit_map(task, WorkRange::new(0, n), mode, |index| {
                    kernel(index as u64 + 1)
                })
                .expect("workload route must execute"),
        )
        .value
}

fn measure(runtime: &Runtime, task: &TaskDefinition, n: usize, backend: BackendKind) -> u64 {
    let mode = match backend {
        BackendKind::Serial => ExecutionMode::Serial,
        BackendKind::Cpu => ExecutionMode::Cpu,
        BackendKind::Gpu => unreachable!(),
    };

    let mut samples = [0_u64; 3];
    for sample in &mut samples {
        let start = Instant::now();
        let values = execute(runtime, task, n, mode);
        black_box(values.last().copied().unwrap_or_default());
        *sample = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    }
    samples.sort_unstable();
    samples[1]
}

fn main() {
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(32_768, usize::MAX),
        execution_budget: ExecutionBudget::default(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("compute-mix-128");

    let serial_check = execute(&runtime, &task, 4_096, ExecutionMode::Serial);
    let cpu_check = execute(&runtime, &task, 4_096, ExecutionMode::Cpu);
    assert_eq!(serial_check, cpu_check, "equivalent routes must match");

    runtime.invalidate_resources();
    let outcome = runtime.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(6, 1_024, 262_144),
        |n, backend| measure(&runtime, &task, n, backend),
    );

    let boundary = runtime.boundary_snapshot();
    let probe_n = boundary
        .profile
        .serial_max_items
        .saturating_add(1)
        .min(262_144);
    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, probe_n),
            ExecutionMode::Auto,
            |index| kernel(index as u64 + 1),
        )
        .expect("automatic workload route must execute");
    let decision = handle.decision();
    let result = runtime.wait(handle);
    let checksum = result.value.iter().fold(0_u64, |acc, value| acc ^ value);

    println!(
        "{{\"workload\":\"compute-mix-128\",\"parallelism\":{},\"status\":\"{:?}\",\"measurements\":{},\"serial_max_items\":{},\"probe_items\":{},\"selected_backend\":\"{:?}\",\"boundary_stale\":{},\"checksum\":{}}}",
        std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1),
        outcome.status,
        outcome.evidence.measurements.len(),
        boundary.profile.serial_max_items,
        probe_n,
        decision.backend,
        decision.boundary_stale,
        checksum,
    );
}
