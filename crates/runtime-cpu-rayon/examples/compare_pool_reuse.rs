use std::hint::black_box;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

const SIZES: &[usize] = &[256, 1_024, 4_096, 16_384, 65_536, 262_144, 1_048_576];

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
        .unwrap_or(21);

    let threads = std::thread::available_parallelism()
        .map(|value| value.get().min(4))
        .unwrap_or(1)
        .max(2);

    let reused_pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("reused Rayon pool must build");

    println!("threads={threads}");
    println!("repetitions={reps}");
    println!("n,new_pool_us,reused_pool_us,setup_overhead_us,speedup");

    for &size in SIZES {
        let mut new_pool_samples = Vec::with_capacity(reps);
        let mut reused_pool_samples = Vec::with_capacity(reps);

        for _ in 0..reps {
            let started = Instant::now();
            let pool = ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .expect("per-call Rayon pool must build");
            let values: Vec<f32> = pool.install(|| {
                (0..size)
                    .into_par_iter()
                    .map(|index| index as f32 * 1.75)
                    .collect()
            });
            black_box(values);
            new_pool_samples.push(started.elapsed());

            let started = Instant::now();
            let values: Vec<f32> = reused_pool.install(|| {
                (0..size)
                    .into_par_iter()
                    .map(|index| index as f32 * 1.75)
                    .collect()
            });
            black_box(values);
            reused_pool_samples.push(started.elapsed());
        }

        let new_pool = median(new_pool_samples);
        let reused_pool = median(reused_pool_samples);
        let overhead = new_pool.saturating_sub(reused_pool);
        let speedup = new_pool.as_secs_f64() / reused_pool.as_secs_f64();

        println!(
            "{size},{:.3},{:.3},{:.3},{:.3}",
            micros(new_pool),
            micros(reused_pool),
            micros(overhead),
            speedup
        );
    }
}
