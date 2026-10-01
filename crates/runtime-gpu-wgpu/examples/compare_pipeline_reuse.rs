use std::hint::black_box;
use std::time::{Duration, Instant};

use runtime_gpu_wgpu::GpuAdapter;

const SIZES: &[usize] = &[256, 1_024, 4_096, 16_384, 65_536, 262_144, 1_048_576];

const SHADER: &str = r#"
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

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn main() {
    let reps = std::env::var("RUNTIME_SETUP_REPS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(11);

    let gpu = GpuAdapter::new().expect("GPU adapter must initialize");
    let prepared = gpu.prepare_f32_kernel(SHADER, 64);

    println!("repetitions={reps}");
    println!("n,rebuild_pipeline_us,cached_dispatch_us,reused_prepared_us,rebuild_vs_cached");

    for &size in SIZES {
        let input: Vec<f32> = (0..size).map(|index| index as f32 * 0.25).collect();
        let params = [1.75_f32];

        black_box(
            gpu.dispatch_f32(SHADER, &input, &params, 64)
                .expect("cached warmup must run"),
        );

        let mut rebuild_samples = Vec::with_capacity(reps);
        let mut cached_samples = Vec::with_capacity(reps);
        let mut prepared_samples = Vec::with_capacity(reps);

        for _ in 0..reps {
            let started = Instant::now();
            let fresh = gpu.prepare_f32_kernel(SHADER, 64);
            black_box(
                gpu.dispatch_prepared_f32(&fresh, &input, &params)
                    .expect("rebuild dispatch must run"),
            );
            rebuild_samples.push(started.elapsed());

            let started = Instant::now();
            black_box(
                gpu.dispatch_f32(SHADER, &input, &params, 64)
                    .expect("cached dispatch must run"),
            );
            cached_samples.push(started.elapsed());

            let started = Instant::now();
            black_box(
                gpu.dispatch_prepared_f32(&prepared, &input, &params)
                    .expect("prepared dispatch must run"),
            );
            prepared_samples.push(started.elapsed());
        }

        let rebuild = median(rebuild_samples);
        let cached = median(cached_samples);
        let prepared = median(prepared_samples);
        let speedup = rebuild.as_secs_f64() / cached.as_secs_f64();

        println!(
            "{size},{:.3},{:.3},{:.3},{:.3}",
            micros(rebuild),
            micros(cached),
            micros(prepared),
            speedup
        );
    }

    println!("cached_kernel_count={}", gpu.cached_f32_kernel_count());
}
