use std::hint::black_box;
use std::time::{Duration, Instant};

use runtime_gpu_wgpu::GpuAdapter;

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn micros(value: Duration) -> f64 {
    value.as_secs_f64() * 1_000_000.0
}

fn main() {
    let reps = std::env::var("RUNTIME_CALIBRATION_REPS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(7);

    let input: Vec<f32> = (0..1_048_576).map(|i| i as f32 * 0.25).collect();

    let init_started = Instant::now();
    let gpu = GpuAdapter::new().expect("GPU adapter must initialize for GPU calibration");
    let init = init_started.elapsed();

    let first_started = Instant::now();
    black_box(
        gpu.vector_scale(&input, 1.75)
            .expect("first GPU vector_scale must execute"),
    );
    let first_vector_scale = first_started.elapsed();

    let mut round_trip = Vec::with_capacity(reps);
    let mut vector_scale = Vec::with_capacity(reps);

    for _ in 0..reps {
        let started = Instant::now();
        black_box(
            gpu.round_trip_f32(&input)
                .expect("GPU round trip must execute"),
        );
        round_trip.push(started.elapsed());

        let started = Instant::now();
        black_box(
            gpu.vector_scale(&input, 1.75)
                .expect("GPU vector_scale must execute"),
        );
        vector_scale.push(started.elapsed());
    }

    println!("elements={}", input.len());
    println!("gpu_init_us={:.3}", micros(init));
    println!(
        "gpu_first_vector_scale_us={:.3}",
        micros(first_vector_scale)
    );
    println!("gpu_round_trip_median_us={:.3}", micros(median(round_trip)));
    println!(
        "gpu_vector_scale_median_us={:.3}",
        micros(median(vector_scale))
    );
    println!("note=round_trip includes upload/copy/readback/map; vector_scale includes upload/shader-dispatch/readback");
}
