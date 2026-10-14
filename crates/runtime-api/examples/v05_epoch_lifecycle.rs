//! Same-runtime resource epoch lifecycle and deployable fixed-route comparison.
use runtime_api::{
    calibrate_elapsed_map, revalidate_elapsed_map, BackendKind, BoundaryProfile,
    BoundedRevalidationConfig, ExecutionMode, RevalidationStatus, Runtime, RuntimeConfig,
    TaskDefinition, WorkRange,
};
use std::hint::black_box;
use std::time::Instant;

fn operation(index: usize) -> u64 {
    let mut x = index as u64 + 1;
    for _ in 0..64 {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545f4914f6cdd1d);
    }
    x
}

fn measure(runtime: &Runtime, task: &TaskDefinition, n: usize, mode: ExecutionMode) -> (u64, BackendKind, Vec<u64>) {
    let start = Instant::now();
    let handle = runtime
        .submit_map(task, WorkRange::new(0, n), mode, operation)
        .expect("map");
    let backend = handle.decision().backend;
    let result = runtime.wait(handle).value;
    black_box(&result);
    (start.elapsed().as_nanos() as u64, backend, result)
}

fn median(values: &[u64]) -> u64 {
    let mut copy = values.to_vec();
    copy.sort_unstable();
    copy[copy.len() / 2]
}

fn main() {
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(1024, usize::MAX),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("v05-epoch-lifecycle");
    let config = BoundedRevalidationConfig::new(3, 64, 65_536);
    let mut expected_epoch = runtime.resource_epoch();
    for phase in 0..3 {
        if phase > 0 {
            expected_epoch += 1;
            assert_eq!(runtime.invalidate_resources(), expected_epoch);
            assert!(runtime.boundary_is_stale());
        }
        let start = Instant::now();
        let outcome = if phase == 0 {
            calibrate_elapsed_map(&runtime, config, &task, operation)
        } else {
            revalidate_elapsed_map(&runtime, config, &task, operation)
        };
        let cost = start.elapsed().as_nanos();
        let stale = runtime.boundary_is_stale();
        assert_eq!(runtime.resource_epoch(), expected_epoch);
        assert!(matches!(
            outcome.status,
            RevalidationStatus::Published | RevalidationStatus::NoLocalCrossover
        ));
        if outcome.status == RevalidationStatus::Published {
            assert!(!stale);
        }
        println!(
            "PHASE phase={phase} epoch={expected_epoch} status={:?} cost_ns={cost} stale={stale} boundary={}",
            outcome.status,
            runtime.boundary_snapshot().profile.serial_max_items
        );
        for &n in &[64, 1024, 16_384, 65_536] {
            let modes = [ExecutionMode::Serial, ExecutionMode::Cpu, ExecutionMode::Auto];
            let mut samples: [Vec<u64>; 3] = std::array::from_fn(|_| Vec::new());
            let mut expected: Option<Vec<u64>> = None;
            let mut backend = BackendKind::Serial;
            for block in 0..24 {
                let offset = (block + phase) % 3;
                for step in 0..3 {
                    let mode = (offset + step) % 3;
                    let (ns, actual, values) = measure(&runtime, &task, n, modes[mode]);
                    if let Some(reference) = &expected {
                        assert_eq!(reference, &values);
                    } else {
                        expected = Some(values);
                    }
                    if mode == 2 {
                        backend = actual;
                    }
                    if block >= 3 {
                        samples[mode].push(ns);
                    }
                }
            }
            let serial = median(&samples[0]);
            let cpu = median(&samples[1]);
            let auto = median(&samples[2]);
            let fixed = serial.min(cpu);
            println!(
                "ROUTES phase={phase} size={n} serial_ns={serial} cpu_ns={cpu} auto_ns={auto} fixed_best_ns={fixed} auto_minus_best_ns={} backend={backend:?}",
                auto as i64 - fixed as i64
            );
        }
    }
}
