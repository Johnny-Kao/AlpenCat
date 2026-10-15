//! Research: one long-lived process, three runtime policies and external affinity transitions.
//! Lines "NEXT" trigger an external supervisor to change this PID's CPU affinity.
use runtime_api::{calibrate_elapsed_map, revalidate_elapsed_map, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode, Runtime, RuntimeConfig, TaskDefinition, WorkRange};
use std::hint::black_box;
use std::io::{self, BufRead, Write};
use std::time::Instant;

fn task(i: usize) -> u64 {
    let mut v = i as u64 + 1;
    for _ in 0..96 {
        v ^= v >> 12;
        v ^= v << 25;
        v ^= v >> 27;
        v = v.wrapping_mul(0x2545f4914f6cdd1d);
    }
    v
}
fn execute(rt: &Runtime, t: &TaskDefinition, size: usize, mode: ExecutionMode) -> (u128, Vec<u64>) {
    let before = Instant::now();
    let handle = rt.submit_map(t, WorkRange::new(0, size), mode, task).expect("submit");
    let result = rt.wait(handle).value;
    black_box(&result);
    (before.elapsed().as_nanos(), result)
}
fn main() {
    let make = || Runtime::with_config(RuntimeConfig { boundary: BoundaryProfile::new(1024, usize::MAX), ..RuntimeConfig::default() });
    let fixed = make();
    let eager = make();
    let lazy = make();
    let t = TaskDefinition::new("v05-lazy-affinity");
    let cfg = BoundedRevalidationConfig::new(3, 64, 65_536);
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut last_parallelism = 0;
    for phase in 0..5 {
        println!("READY {phase}");
        io::stdout().flush().expect("flush");
        assert_eq!(lines.next().expect("supervisor").expect("stdin"), "GO");
        let available = std::thread::available_parallelism().expect("parallelism").get();
        let changed = available != last_parallelism;
        let begin_eager = Instant::now();
        if phase == 0 {
            let _ = calibrate_elapsed_map(&eager, cfg, &t, task);
        } else {
            eager.invalidate_resources();
            let _ = revalidate_elapsed_map(&eager, cfg, &t, task);
        }
        let eager_calib = begin_eager.elapsed().as_nanos();
        let begin_lazy = Instant::now();
        if changed {
            if phase == 0 {
                let _ = calibrate_elapsed_map(&lazy, cfg, &t, task);
            } else {
                lazy.invalidate_resources();
                let _ = revalidate_elapsed_map(&lazy, cfg, &t, task);
            }
        }
        let lazy_calib = begin_lazy.elapsed().as_nanos();
        let sizes = [64, 1024, 16384, 65536];
        let mut totals = [0u128; 5];
        let mut reference: Option<Vec<u64>> = None;
        for trial in 0..9 {
            for &size in &sizes {
                // Interleave counterfactual fixed routes with actual policies.
                let runs = [
                    execute(&fixed, &t, size, ExecutionMode::Serial),
                    execute(&fixed, &t, size, ExecutionMode::Cpu),
                    execute(&fixed, &t, size, ExecutionMode::Auto),
                    execute(&eager, &t, size, ExecutionMode::Auto),
                    execute(&lazy, &t, size, ExecutionMode::Auto),
                ];
                for i in 0..5 {
                    assert_eq!(runs[i].1, runs[0].1);
                    if trial >= 2 {
                        totals[i] += runs[i].0;
                    }
                }
                if size == 64 {
                    reference = Some(runs[0].1.clone());
                }
            }
        }
        black_box(reference);
        println!("RESULT phase={phase} effective_cpus={available} changed={changed} serial_ns={} cpu_ns={} static_auto_ns={} eager_auto_ns={} lazy_auto_ns={} eager_calibration_ns={eager_calib} lazy_calibration_ns={lazy_calib} eager_total_ns={} lazy_total_ns={} static_total_ns={}",
            totals[0], totals[1], totals[2], totals[3], totals[4],
            totals[3]+eager_calib,totals[4]+lazy_calib,totals[2]);
        io::stdout().flush().expect("flush");
        last_parallelism = available;
    }
}
