use std::time::{Duration, Instant};

use runtime_api::{
    BackendKind, BrokerCapacity, BrokerRequest, ExecutionMode, Runtime, TaskDefinition, WorkRange,
};

const TASK_H64: TaskDefinition = TaskDefinition::new("upfirdn-fir-h64");
const TASK_H128: TaskDefinition = TaskDefinition::new("upfirdn-fir-h128");

fn make_input(n: usize) -> Vec<f64> {
    (0..n).map(|i| ((i as f64) * 0.001).sin()).collect()
}

fn make_filter(nh: usize) -> Vec<f64> {
    (0..nh).map(|i| ((i as f64) * 0.03).cos()).collect()
}

fn run_once(
    runtime: &Runtime,
    task: &TaskDefinition,
    x: &[f64],
    h: &[f64],
    mode: ExecutionMode,
) -> (BackendKind, Duration, Vec<f64>) {
    let lo = h.len() - 1;
    let hi = x.len();
    let started = Instant::now();
    let handle = runtime
        .submit_map(task, WorkRange::new(lo, hi), mode, |y| {
            let base = y - h.len() + 1;
            let mut acc = 0.0f64;
            for j in 0..h.len() {
                acc += x[base + j] * h[j];
            }
            acc
        })
        .expect("FIR execution");
    let elapsed = started.elapsed();
    let decision = handle.decision();
    let result = runtime.wait(handle);
    (decision.backend, elapsed, result.value)
}

fn median_duration(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn measure(
    runtime: &Runtime,
    task: &TaskDefinition,
    x: &[f64],
    h: &[f64],
    mode: ExecutionMode,
    reps: usize,
) -> (BackendKind, Duration, Vec<f64>) {
    let mut times = Vec::with_capacity(reps);
    let mut last_backend = BackendKind::Serial;
    let mut last = Vec::new();

    for _ in 0..reps {
        let (backend, elapsed, values) = run_once(runtime, task, x, h, mode);
        last_backend = backend;
        times.push(elapsed);
        last = values;
    }

    (last_backend, median_duration(times), last)
}

fn exact_equal(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| a.to_bits() == b.to_bits())
}

fn train(runtime: &Runtime, task: &TaskDefinition, h: &[f64]) {
    // Warm the reusable Rayon pool before collecting learning samples so the
    // persistent CPU model does not absorb one-time pool construction cost.
    let warm_n = h.len().max(256);
    let warm = make_input(warm_n);
    let _ = run_once(runtime, task, &warm, h, ExecutionMode::Cpu);
    runtime.reset_task_learning(task);

    for n in [512usize, 2_048, 8_192, 32_768, 131_072, 524_288] {
        let n = n.max(h.len() + 1);
        let x = make_input(n);
        let _ = run_once(runtime, task, &x, h, ExecutionMode::Serial);
        let _ = run_once(runtime, task, &x, h, ExecutionMode::Cpu);
    }
}

fn run_suite(task: &TaskDefinition, nh: usize) {
    let runtime = Runtime::new();
    let h = make_filter(nh);
    train(&runtime, task, &h);

    let host = runtime.host_profile();
    let capacity = BrokerCapacity::new(host.logical_cpus.max(1), 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    println!();
    println!("## FIR h={nh}");
    println!("logical_cpus={}", host.logical_cpus);
    println!("| n | pred_serial_us | pred_cpu_us | model_choice | serial_us | cpu_us | cpu_speedup | actual_faster | auto_backend | auto_us | exact | optimal |");
    println!("|---:|---:|---:|---|---:|---:|---:|---|---|---:|---|---|");

    for n in [
        256usize,
        512,
        1_024,
        2_048,
        4_096,
        8_192,
        16_384,
        32_768,
        65_536,
        262_144,
        1_048_576,
    ] {
        let n = n.max(nh + 1);
        let x = make_input(n);

        let decision = runtime.cost_model_decision(task, n - nh + 1, capacity, request);
        let model_choice = decision.backend;
        let pred_serial = decision
            .serial
            .and_then(|v| v.predicted_nanos)
            .unwrap_or(f64::NAN)
            / 1_000.0;
        let pred_cpu = decision
            .cpu
            .and_then(|v| v.predicted_nanos)
            .unwrap_or(f64::NAN)
            / 1_000.0;

        let (_serial_backend, serial_t, serial) =
            measure(&runtime, task, &x, &h, ExecutionMode::Serial, 5);
        let (_cpu_backend, cpu_t, cpu) =
            measure(&runtime, task, &x, &h, ExecutionMode::Cpu, 5);
        let (auto_backend, auto_t, auto) =
            measure(&runtime, task, &x, &h, ExecutionMode::Auto, 5);

        let exact = exact_equal(&serial, &cpu) && exact_equal(&serial, &auto);
        let actually_faster = if cpu_t < serial_t {
            BackendKind::Cpu
        } else {
            BackendKind::Serial
        };
        let optimal = auto_backend == actually_faster;
        let speedup = serial_t.as_secs_f64() / cpu_t.as_secs_f64();

        println!(
            "| {n} | {pred_serial:.2} | {pred_cpu:.2} | {:?} | {:.2} | {:.2} | {:.3}x | {:?} | {:?} | {:.2} | {} | {} |",
            model_choice,
            serial_t.as_secs_f64() * 1e6,
            cpu_t.as_secs_f64() * 1e6,
            speedup,
            actually_faster,
            auto_backend,
            auto_t.as_secs_f64() * 1e6,
            if exact { "yes" } else { "NO" },
            if optimal { "yes" } else { "NO" },
        );

        assert!(exact, "parallel/auto FIR output must be bitwise exact");
    }
}

fn main() {
    println!("# upfirdn-style adaptive FIR benchmark");
    run_suite(&TASK_H64, 64);
    run_suite(&TASK_H128, 128);
}
