use rayon::prelude::*;
use std::hint::black_box;
use std::time::Instant;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

fn serial_kernel(data: &[f64]) -> f64 {
    data.iter().enumerate().map(|(i, &x)| {
        let y = x * 1.000_000_119 + (i as f64 * 1e-9);
        y.mul_add(y, x)
    }).sum()
}

fn parallel_kernel(data: &[f64]) -> f64 {
    data.par_iter().enumerate().map(|(i, &x)| {
        let y = x * 1.000_000_119 + (i as f64 * 1e-9);
        y.mul_add(y, x)
    }).sum()
}

fn timed<F: FnMut() -> f64>(mut f: F) -> (f64, f64) {
    let start = Instant::now();
    let value = black_box(f());
    (start.elapsed().as_nanos() as f64, value)
}

fn main() {
    let phase = std::env::var("ALPENCAT_ECON_PHASE").unwrap_or_else(|_| "unknown".into());
    let sizes = [512usize, 1024, 2048, 4096, 8192, 16_384, 32_768, 65_536,
        131_072, 262_144, 524_288, 1_048_576, 2_097_152];

    let max_n = *sizes.last().unwrap();
    let data: Vec<f64> = (0..max_n)
        .map(|i| ((i * 17 + 13) % 997) as f64 / 997.0)
        .collect();

    black_box(parallel_kernel(&data[..65_536]));
    black_box(serial_kernel(&data[..65_536]));
    println!("phase,n,serial_ns,parallel_ns,winner,rayon_threads");

    for &n in &sizes {
        let slice = &data[..n];

        let serial_check = serial_kernel(slice);
        let parallel_check = parallel_kernel(slice);
        let tolerance = serial_check.abs().max(1.0) * 1e-10;
        if (serial_check - parallel_check).abs() > tolerance {
            eprintln!("correctness_failed phase={} n={}", phase, n);
            std::process::exit(7);
        }

        for _ in 0..3 {
            black_box(serial_kernel(slice));
            black_box(parallel_kernel(slice));
        }

        let iterations = if n <= 8192 { 21 } else if n <= 65_536 { 15 } else if n <= 524_288 { 11 } else { 7 };
        let mut serial_samples = Vec::with_capacity(iterations);
        let mut parallel_samples = Vec::with_capacity(iterations);

        for i in 0..iterations {
            if i & 1 == 0 {
                let (ns, v) = timed(|| serial_kernel(slice));
                black_box(v);
                serial_samples.push(ns);
                let (ns, v) = timed(|| parallel_kernel(slice));
                black_box(v);
                parallel_samples.push(ns);
            } else {
                let (ns, v) = timed(|| parallel_kernel(slice));
                black_box(v);
                parallel_samples.push(ns);
                let (ns, v) = timed(|| serial_kernel(slice));
                black_box(v);
                serial_samples.push(ns);
            }
        }

        let serial_ns = median(serial_samples);
        let parallel_ns = median(parallel_samples);
        let winner = if serial_ns <= parallel_ns { "SERIAL" } else { "PARALLEL" };
        println!("{},{},{:.3},{:.3},{},{}", phase, n, serial_ns, parallel_ns, winner, rayon::current_num_threads());
    }
}
