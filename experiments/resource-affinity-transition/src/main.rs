//! CPU-bound parallel executor whose main thread's REAL Linux affinity changes between phases.
//! One process per strategy; new worker threads inherit the main thread's Linux affinity.
//! Protocol: stdin "GO" once per phase, stdout "PHASE ..." after completed work.
#![cfg(target_os = "linux")]
use alpencat_resource::{snapshot, Reading, ResourceSnapshot, ResourceState};
use std::{
    hint::black_box,
    io::{self, BufRead, Write},
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::Instant,
};

#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }
unsafe extern "C" {
    fn clock_gettime(clock_id: i32, out: *mut Timespec) -> i32;
}
fn process_cpu_ns() -> u128 {
    let mut out = Timespec { sec: 0, nsec: 0 };
    let rc = unsafe { clock_gettime(2, &mut out) }; // CLOCK_PROCESS_CPUTIME_ID on Linux
    assert_eq!(rc, 0, "clock_gettime(CLOCK_PROCESS_CPUTIME_ID) failed");
    (out.sec as u128) * 1_000_000_000 + out.nsec as u128
}
fn visible_workers(s: &ResourceSnapshot) -> usize {
    let n = match &s.allowed_cpus {
        Reading::Known(ids) if !ids.is_empty() => ids.len(),
        _ => 1
    };
    let cap = match &s.effective_cpu_capacity {
        Reading::Known(n) if n.is_finite() && *n >= 0.0 => n.ceil() as usize,
        _ => n
    };
    n.min(cap).clamp(1, 2)
}
#[inline(never)]
fn compute_block(block: usize, iterations: usize) -> u64 {
    // Arithmetic, not sleep or a simulated metric. Each block's output is
    // deterministic and independent of the worker count or scheduling order.
    let mut state = (block as u64).wrapping_mul(0x9e3779b97f4a7c15)
        .wrapping_add(0xd1b54a32d192ed03);
    for i in 0..iterations {
        let x = (i as u64).wrapping_add((block as u64).rotate_left(19));
        state ^= x.wrapping_mul(0xbf58476d1ce4e5b9);
        state = state.rotate_left(17).wrapping_mul(0x94d049bb133111eb);
    }
    black_box(state)
}
fn parallel_compute(workers: usize, blocks: usize, iterations: usize, expected_cpus: usize) -> u64 {
    let failed_affinity = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for worker in 0..workers {
            let failure = &failed_affinity;
            handles.push(scope.spawn(move || {
                let seen = std::thread::available_parallelism().expect("worker affinity").get();
                if seen != expected_cpus { failure.store(true, Ordering::Relaxed); }
                let mut sum = 0u64;
                for block in (worker..blocks).step_by(workers) {
                    sum = sum.wrapping_add(compute_block(block, iterations));
                }
                sum
            }));
        }
        handles.into_iter().map(|h| h.join().expect("worker panic"))
            .fold(0u64, u64::wrapping_add)
    });
    assert!(!failed_affinity.load(Ordering::Relaxed),
        "new workers did not inherit the expected real Linux affinity");
    result
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: binary <fixed_one|fixed_two|direct_event|alpencat> <light|heavy>");
    let mode = args[1].as_str();
    assert!(["fixed_one","fixed_two","direct_event","alpencat"].contains(&mode));
    let iterations = match args[2].as_str() {
        "light" => 25_000,
        "heavy" => 160_000,
        _ => panic!("unknown workload size")
    };
    const BLOCKS: usize = 96;
    let resource_state = ResourceState::new();
    println!("READY pid={}", std::process::id());
    io::stdout().flush().expect("ready flush");
    let stdin = io::stdin();
    for (phase,line) in stdin.lock().lines().enumerate() {
        assert_eq!(line.expect("protocol read").trim(), "GO");
        let wall_start = Instant::now();
        let cpu_start = process_cpu_ns();
        let observe_start = Instant::now();
        let allowed_now = std::thread::available_parallelism()
            .expect("main thread affinity").get();
        assert!(allowed_now == 1 || allowed_now == 2,
            "controller must set 1 or 2 real available CPUs");
        let workers = match mode {
            "fixed_one" => 1,
            "fixed_two" => 2,
            "direct_event" => visible_workers(&snapshot()),
            "alpencat" => {
                resource_state.invalidate(); // external controller reports resource transition
                let refreshed = resource_state.refresh()
                    .expect("inconsistent resource observation");
                visible_workers(refreshed.fresh().expect("invalidated snapshot cannot be fresh"))
            },
            _ => unreachable!(),
        };
        // Lifecycle caller can read cached state without syscalls at arbitrary points.
        if mode == "alpencat" {
            assert!(resource_state.cached().fresh().is_some());
        }
        let observation_ns = observe_start.elapsed().as_nanos();
        if mode == "direct_event" || mode == "alpencat" {
            let s = snapshot(); // validation outside observation timing
            assert_eq!(visible_workers(&s), allowed_now,
                "observed worker cap differs from actual affinity; unsuitable environment");
        }
        let checksum = parallel_compute(workers, BLOCKS, iterations, allowed_now);
        let cpu_ns = process_cpu_ns() - cpu_start;
        let wall_ns = wall_start.elapsed().as_nanos();
        println!("PHASE phase={} mode={} size={} actual_allowed={} workers={} checksum={} wall_ns={} cpu_ns={} observe_ns={}",
            phase, mode, args[2], allowed_now, workers, checksum, wall_ns, cpu_ns, observation_ns);
        io::stdout().flush().expect("phase flush");
    }
}
