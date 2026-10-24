//! Bounded streamed integer transform. Identical output for all observation strategies.
//! Modes differ only in resource-observation and caller-owned chunk-grain policy.
use alpencat_resource::{snapshot, Reading, ResourceSnapshot, ResourceState};
use std::{env, hint::black_box, time::Instant};

const PERIOD: usize = 128;

#[derive(Clone, Copy)]
enum Mode { Static, PerChunk, Periodic, Cached }
impl Mode {
    fn parse(s: &str) -> Self {
        match s {
            "static" => Self::Static,
            "per_chunk" => Self::PerChunk,
            "periodic" => Self::Periodic,
            "cached" => Self::Cached,
            _ => panic!("mode: static|per_chunk|periodic|cached"),
        }
    }
    fn label(self) -> &'static str {
        match self { Self::Static => "static", Self::PerChunk => "per_chunk",
            Self::Periodic => "periodic", Self::Cached => "cached" }
    }
}
fn cpu_grain(s: &ResourceSnapshot) -> usize {
    let allowed = match &s.allowed_cpus {
        Reading::Known(ids) if !ids.is_empty() => ids.len(),
        _ => 1,
    };
    let effective = match s.effective_cpu_capacity {
        Reading::Known(n) if n.is_finite() && n >= 1.0 =>
            (n.floor() as usize).max(1),
        _ => allowed,
    };
    allowed.min(effective).clamp(1, 8) * 64
}
#[inline(never)]
fn compute_chunk(index: usize, units: usize, grain: usize) -> u64 {
    let mut acc = (index as u64).wrapping_add(0x9e3779b97f4a7c15);
    let mut offset = 0;
    while offset < units {
        let end = (offset + grain).min(units);
        for i in offset..end {
            let v = (index as u64).wrapping_mul(0x5851f42d4c957f2d) ^ i as u64;
            acc = acc.wrapping_add(v.wrapping_mul(0xbf58476d1ce4e5b9));
            acc = acc.rotate_left(13) ^ 0x94d049bb133111eb;
        }
        offset = end;
    }
    black_box(acc)
}
fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 3, "usage: workload_runner <mode> <light|moderate>");
    let mode = Mode::parse(&args[1]);
    let (batches, units) = match args[2].as_str() {
        "light" => (2400usize, 5000usize),
        "moderate" => (800usize, 45_000usize),
        _ => panic!("scenario: light|moderate"),
    };
    let state = ResourceState::new();
    let mut grain = std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 8) * 64).unwrap_or(64);
    let mut observations: usize = 0;
    let mut invalidations: usize = 0;
    if matches!(mode, Mode::Cached) {
        state.refresh().expect("consistent initial observation");
        observations += 1;
    }
    let start = Instant::now();
    let mut checksum = 0_u64;
    let mut grain_sum = 0_u64;
    for chunk in 0..batches {
        match mode {
            Mode::Static => {}, // calibrated once by the OS standard API above
            Mode::PerChunk => {
                grain = cpu_grain(&snapshot());
                observations += 1;
            },
            Mode::Periodic if chunk % PERIOD == 0 => {
                grain = cpu_grain(&snapshot());
                observations += 1;
            },
            Mode::Cached => {
                if chunk > 0 && chunk % PERIOD == 0 {
                    // Synthetic caller event, NOT a real CPU/cgroup change.
                    state.invalidate();
                    invalidations += 1;
                    state.refresh().expect("consistent observation after invalidation");
                    observations += 1;
                }
                grain = state.cached().fresh()
                    .map(cpu_grain)
                    .unwrap_or(64);
            },
            Mode::Periodic => {},
        }
        grain_sum += grain as u64;
        checksum = checksum.wrapping_add(compute_chunk(chunk, units, grain)
            .rotate_left((chunk % 64) as u32));
    }
    let wall = start.elapsed().as_nanos();
    println!("WORKLOAD_RESULT mode={} scenario={} checksum={} policy_grain_sum={}         wall_ns={} observations={} invalidations={} batches={} units_per_batch={}",
        mode.label(), args[2], checksum, grain_sum, wall,
        observations, invalidations, batches, units);
}
