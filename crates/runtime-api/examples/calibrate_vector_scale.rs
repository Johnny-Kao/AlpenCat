use std::hint::black_box;
use std::time::{Duration, Instant};

use runtime_api::{
    ExecutionMode, RangeTaskImplementations, Runtime, RuntimeError, TaskDefinition, WorkRange,
};

const SIZES: &[usize] = &[
    256, 1_024, 4_096, 16_384, 65_536, 262_144, 1_048_576, 4_194_304,
];

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

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn implementations<'a>(
    input: &'a [f32],
    alpha: f32,
) -> RangeTaskImplementations<'a, f32, impl Fn(usize) -> f32 + Sync + Send + 'a> {
    RangeTaskImplementations::new(move |index| alpha * input[index])
        .with_gpu(move |gpu| gpu.dispatch_f32(VECTOR_SCALE_SHADER, input, &[alpha], 64))
}

fn measure(
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &[f32],
    alpha: f32,
    mode: ExecutionMode,
    reps: usize,
) -> Result<Duration, RuntimeError> {
    let range = WorkRange::new(0, input.len());

    let warmup = runtime.submit_range_task(task, range, mode, implementations(input, alpha))?;
    black_box(runtime.wait(warmup).value);

    let mut samples = Vec::with_capacity(reps);

    for _ in 0..reps {
        let started = Instant::now();
        let handle = runtime.submit_range_task(task, range, mode, implementations(input, alpha))?;
        black_box(runtime.wait(handle).value);
        samples.push(started.elapsed());
    }

    Ok(median(samples))
}

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn main() {
    let reps = std::env::var("RUNTIME_CALIBRATION_REPS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(7);

    let runtime = Runtime::new();
    let task = TaskDefinition::new("vector_scale");
    let alpha = 1.75_f32;

    println!("repetitions={reps}");
    println!("n,serial_us,cpu_us,gpu_us");

    let mut serial_cpu_cross = None;
    let mut cpu_gpu_cross = None;
    let mut previous_size = None;

    for &size in SIZES {
        let input: Vec<f32> = (0..size).map(|index| index as f32 * 0.25).collect();

        let serial = measure(&runtime, &task, &input, alpha, ExecutionMode::Serial, reps)
            .expect("serial calibration path must execute");

        let cpu = measure(&runtime, &task, &input, alpha, ExecutionMode::Cpu, reps)
            .expect("CPU calibration path must execute");

        let gpu = measure(&runtime, &task, &input, alpha, ExecutionMode::Gpu, reps).ok();

        println!(
            "{size},{:.3},{:.3},{}",
            micros(serial),
            micros(cpu),
            gpu.map(|value| format!("{:.3}", micros(value)))
                .unwrap_or_else(|| "NA".to_string())
        );

        if serial_cpu_cross.is_none() && cpu < serial {
            serial_cpu_cross = Some((previous_size.unwrap_or(0), size));
        }

        if let Some(gpu) = gpu {
            if cpu_gpu_cross.is_none() && gpu < cpu {
                cpu_gpu_cross = Some((previous_size.unwrap_or(0), size));
            }
        }

        previous_size = Some(size);
    }

    println!();
    match serial_cpu_cross {
        Some((lower, upper)) => {
            println!("serial_to_cpu_candidate_between={lower}..{upper}");
        }
        None => println!("serial_to_cpu_candidate_between=not_observed"),
    }

    match cpu_gpu_cross {
        Some((lower, upper)) => {
            println!("cpu_to_gpu_candidate_between={lower}..{upper}");
        }
        None => println!("cpu_to_gpu_candidate_between=not_observed"),
    }

    println!(
        "note=calibration is environment-specific; do not promote software-GPU results to hardware defaults"
    );
}
