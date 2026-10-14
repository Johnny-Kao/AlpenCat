//! v0.5 same-runtime randomized-order paired measurement (screening, not lifecycle proof).
use runtime_api::{
    calibrate_elapsed_map, BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode,
    Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};
use std::env;
use std::hint::black_box;
use std::time::Instant;

fn kernel(index: usize, kind: usize) -> u64 {
    let mut x = index as u64 + 1;
    if kind == 1 {
        x = x.rotate_left(17).wrapping_mul(0x9e3779b97f4a7c15);
    }
    let iters = match kind {
        0 => 128,
        1 => 2,
        _ => 32,
    };
    for _ in 0..iters {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545f4914f6cdd1d);
    }
    x
}
fn run(
    runtime: &Runtime,
    task: &TaskDefinition,
    n: usize,
    mode: ExecutionMode,
    kind: usize,
) -> (u64, BackendKind, Vec<u64>) {
    let start = Instant::now();
    let handle = runtime
        .submit_map(task, WorkRange::new(0, n), mode, |i| kernel(i, kind))
        .expect("valid map");
    let backend = handle.decision().backend;
    let values = runtime.wait(handle).value;
    let dt = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    black_box(&values);
    (dt, backend, values)
}
fn median(v: &[u64]) -> u64 {
    let mut s = v.to_vec();
    s.sort_unstable();
    s[s.len() / 2]
}
fn main() {
    let rounds = env::var("ALPENCAT_V05_ROUNDS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(21);
    assert!(rounds >= 7 && rounds % 2 == 1);
    let sizes: [usize; 4] = [64, 1024, 16384, 65536];
    for kind in 0..3 {
        let runtime = Runtime::with_config(RuntimeConfig {
            boundary: BoundaryProfile::new(1024, usize::MAX),
            ..RuntimeConfig::default()
        });
        let task = TaskDefinition::new("v05-paired");
        let calibration_start = Instant::now();
        let calibration = calibrate_elapsed_map(
            &runtime,
            BoundedRevalidationConfig::new(3, 64, 65536),
            &task,
            |i| kernel(i, kind),
        );
        let calibration_ns = calibration_start.elapsed().as_nanos() as u64;
        println!(
            "CALIB kind={kind} status={:?} ns={calibration_ns} boundary={} stale={}",
            calibration.status,
            runtime.boundary_snapshot().profile.serial_max_items,
            runtime.boundary_is_stale()
        );
        for n in sizes {
            let modes = [
                ExecutionMode::Serial,
                ExecutionMode::Cpu,
                ExecutionMode::Auto,
            ];
            let mut samples: [Vec<u64>; 3] = std::array::from_fn(|_| Vec::new());
            let mut outputs: [Option<Vec<u64>>; 3] = std::array::from_fn(|_| None);
            let mut backends = [BackendKind::Serial; 3];
            // Rotating A/B/C order removes fixed-route-always-first timing bias.
            // Each block has 3 modes and is compared *within that block*.
            for block in 0..(rounds + 3) {
                let first = (block * 2 + kind + n) % 3;
                for offset in 0..3 {
                    let ix = (first + offset) % 3;
                    let (dt, backend, values) = run(&runtime, &task, n, modes[ix], kind);
                    if let Some(expected) = &outputs[ix] {
                        assert_eq!(&values, expected);
                    } else {
                        outputs[ix] = Some(values);
                    }
                    backends[ix] = backend;
                    if block >= 3 {
                        samples[ix].push(dt);
                    }
                }
            }
            assert_eq!(outputs[0], outputs[1], "Serial/CPU mismatch");
            assert_eq!(outputs[0], outputs[2], "Serial/Auto mismatch");
            let paired: Vec<i64> = (0..rounds)
                .map(|i| {
                    let fixed = samples[0][i].min(samples[1][i]);
                    fixed as i64 - samples[2][i] as i64
                })
                .collect();
            let negatives = paired.iter().filter(|v| **v < 0).count();
            let mut sorted = paired.clone();
            sorted.sort_unstable();
            println!("RESULT kind={kind} size={n} serial_p50_ns={} cpu_p50_ns={} auto_p50_ns={} paired_oracle_delta_p50_ns={} regressions={negatives}/{rounds} backend_cpu={:?} backend_auto={:?} rounds={rounds}",
                median(&samples[0]), median(&samples[1]), median(&samples[2]),
                sorted[rounds/2], backends[1], backends[2]);
        }
    }
}
