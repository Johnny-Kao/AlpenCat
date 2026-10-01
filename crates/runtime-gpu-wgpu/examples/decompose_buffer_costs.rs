use std::hint::black_box;
use std::time::{Duration, Instant};

use runtime_gpu_wgpu::GpuAdapter;

const SIZES: &[usize] = &[4_096, 65_536, 1_048_576];

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
    let reps = std::env::var("RUNTIME_GPU_STAGE_REPS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(9);

    let gpu = GpuAdapter::new().expect("GPU adapter must initialize");
    let kernel = gpu.prepare_f32_kernel(SHADER, 64);

    println!("repetitions={reps}");
    println!("n,fresh_buffers_us,resident_buffers_us,resident_speedup,upload_us,compute_wait_us,readback_map_us");

    for &size in SIZES {
        let input: Vec<f32> = (0..size).map(|index| index as f32 * 0.25).collect();
        let params = [1.75_f32];
        let resident = gpu.prepare_resident_f32(&kernel, input.len(), params.len());

        black_box(
            gpu.dispatch_f32(SHADER, &input, &params, 64)
                .expect("fresh-buffer warmup must execute"),
        );
        black_box(
            gpu.dispatch_resident_f32(&kernel, &resident, &input, &params)
                .expect("resident-buffer warmup must execute"),
        );

        let mut fresh_samples = Vec::with_capacity(reps);
        let mut resident_samples = Vec::with_capacity(reps);
        let mut upload_samples = Vec::with_capacity(reps);
        let mut compute_samples = Vec::with_capacity(reps);
        let mut readback_samples = Vec::with_capacity(reps);

        for _ in 0..reps {
            let started = Instant::now();
            black_box(
                gpu.dispatch_f32(SHADER, &input, &params, 64)
                    .expect("fresh-buffer dispatch must execute"),
            );
            fresh_samples.push(started.elapsed());

            let started = Instant::now();
            black_box(
                gpu.dispatch_resident_f32(&kernel, &resident, &input, &params)
                    .expect("resident-buffer dispatch must execute"),
            );
            resident_samples.push(started.elapsed());

            let (output, timings) = gpu
                .run_resident_f32_profiled(&kernel, &resident, &input, &params)
                .expect("profiled resident dispatch must execute");
            black_box(output);
            upload_samples.push(timings.upload);
            compute_samples.push(timings.compute_wait);
            readback_samples.push(timings.readback_map);
        }

        let fresh = median(fresh_samples);
        let resident_total = median(resident_samples);
        let upload = median(upload_samples);
        let compute = median(compute_samples);
        let readback = median(readback_samples);
        let speedup = fresh.as_secs_f64() / resident_total.as_secs_f64();

        println!(
            "{size},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            micros(fresh),
            micros(resident_total),
            speedup,
            micros(upload),
            micros(compute),
            micros(readback)
        );
    }
}
