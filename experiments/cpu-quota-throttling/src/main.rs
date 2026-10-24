//! Real quota-limited queued CPU work. No AlpineCat production policy is changed.
#![cfg(target_os = "linux")]
use alpencat_resource::{snapshot, Reading, ResourceSnapshot, ResourceState};
use std::{
    env,
    hint::black_box,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[repr(C)]
struct Timespec { sec: i64, nsec: i64 }
unsafe extern "C" {
    fn clock_gettime(clock_id: i32, tp: *mut Timespec) -> i32;
}
fn cpu_ns() -> u128 {
    let mut t = Timespec { sec: 0, nsec: 0 };
    assert_eq!(unsafe { clock_gettime(2, &mut t) }, 0);
    t.sec as u128 * 1_000_000_000 + t.nsec as u128
}

#[derive(Clone, Copy)]
enum Mode { Fixed1, Fixed2, Fixed4, Fixed8, Direct, Resource }
impl Mode {
    fn parse(s: &str) -> Self {
        match s {
            "fixed1" => Self::Fixed1, "fixed2" => Self::Fixed2,
            "fixed4" => Self::Fixed4, "fixed8" => Self::Fixed8,
            "direct" => Self::Direct, "alpencat" => Self::Resource,
            _ => panic!("invalid mode: {s}"),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Fixed1 => "fixed1", Self::Fixed2 => "fixed2",
            Self::Fixed4 => "fixed4", Self::Fixed8 => "fixed8",
            Self::Direct => "direct", Self::Resource => "alpencat",
        }
    }
}

fn proposed_workers(s: &ResourceSnapshot) -> usize {
    // Conservative experimental policy: never exceed CPU quota rounded up;
    // no guarantee this is optimal for the workload.
    let allowed = match &s.allowed_cpus {
        Reading::Known(v) if !v.is_empty() => v.len(), _ => 1
    };
    match &s.effective_cpu_capacity {
        Reading::Known(q) if q.is_finite() && *q > 0.0 =>
            allowed.min(q.ceil() as usize).clamp(1, 8),
        _ => 1, // fail closed on ambiguous quota rather than assume 8 CPUs
    }
}

fn quota_from_snapshot(s: &ResourceSnapshot) -> Option<u32> {
    match s.effective_cpu_capacity {
        Reading::Known(n) if n.is_finite() && n >= 0.0 => Some((n * 1000.0).round() as u32),
        _ => None
    }
}

#[inline(never)]
fn compute(request: usize, iterations: usize) -> u64 {
    let mut v = (request as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ 0xd1b54a32d192ed03;
    for i in 0..iterations {
        v ^= (i as u64).wrapping_add((request as u64) << 19)
            .wrapping_mul(0xbf58476d1ce4e5b9);
        v = v.rotate_left(17).wrapping_mul(0x94d049bb133111eb);
    }
    black_box(v)
}

fn run_queue(workers: usize, requests: usize, iterations: usize, start: Instant)
    -> (u64, u128, u128, usize)
{
    let cursor = AtomicUsize::new(0);
    let completed: Vec<_> = (0..requests).map(|_| AtomicU64::new(0)).collect();
    let checksum = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers).map(|_| {
            let cursor = &cursor;
            let completed = &completed;
            scope.spawn(move || {
                let mut sum = 0_u64;
                loop {
                    let index = cursor.fetch_add(1, Ordering::Relaxed);
                    if index >= requests { break; }
                    sum = sum.wrapping_add(compute(index, iterations));
                    completed[index].store(start.elapsed().as_nanos() as u64, Ordering::Release);
                }
                sum
            })
        }).collect();
        handles.into_iter().map(|h| h.join().expect("worker panic"))
            .fold(0_u64, u64::wrapping_add)
    });
    let mut completion: Vec<_> = completed.iter()
        .map(|v| v.load(Ordering::Acquire)).collect();
    assert!(completion.iter().all(|n| *n > 0));
    completion.sort_unstable();
    let idx = ((requests as f64) * 0.95).ceil() as usize - 1;
    let p95_ns = completion[idx] as u128;
    let wall_ns = start.elapsed().as_nanos();
    (checksum, p95_ns, wall_ns, requests)
}

fn main() {
    let a: Vec<_> = env::args().collect();
    assert_eq!(a.len(), 4, "usage: binary mode size unix-socket");
    let mode = Mode::parse(&a[1]);
    let (requests, iterations) = match a[2].as_str() {
        "short" => (160, 900_000),
        "long" => (320, 1_800_000),
        _ => panic!("invalid workload"),
    };
    let mut stream = {
        let start = Instant::now();
        loop {
            match UnixStream::connect(&a[3]) {
                Ok(s) => break s,
                Err(e) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                    black_box(e);
                }
                Err(e) => panic!("connect controller failed: {e}"),
            }
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    writeln!(stream, "READY pid={}", std::process::id()).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let state = Arc::new(ResourceState::new());
    for phase in 0..3 {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "controller disconnected");
        let expected_milli: u32 = line.trim()
            .strip_prefix("GO ").expect("bad command").parse().unwrap();

        // Verify genuine cgroup2 CPU quotas before the measurement interval.
        // This identical validation read is excluded for all six policies.
        let measured = snapshot();
        let visible = quota_from_snapshot(&measured)
            .expect("cpu.max quota unavailable: invalid test environment");
        assert_eq!(visible, expected_milli, "CPU quota did not actually change");

        let start = Instant::now();
        let cpu_before = cpu_ns();
        let sample_start = Instant::now();
        let workers = match mode {
            Mode::Fixed1 => 1,
            Mode::Fixed2 => 2,
            Mode::Fixed4 => 4,
            Mode::Fixed8 => 8,
            Mode::Direct => proposed_workers(&snapshot()),
            Mode::Resource => {
                state.invalidate(); // controller provides the same phase change signal
                let fresh = state.refresh().expect("cgroup membership changed during read");
                proposed_workers(fresh.fresh().expect("fresh observation required"))
            }
        };
        let observation_ns = sample_start.elapsed().as_nanos();
        assert!((1..=8).contains(&workers));
        if matches!(mode, Mode::Direct | Mode::Resource) {
            let expected_workers = if expected_milli < 1000 { 1 } else { 2 };
            assert_eq!(workers, expected_workers, "adaptive policy mismatch");
        }

        let (checksum, p95_ns, wall_ns, _) =
            run_queue(workers, requests, iterations, start);
        let process_cpu_ns = cpu_ns().saturating_sub(cpu_before);
        writeln!(stream,
            "PHASE phase={} mode={} size={} quota_milli={} workers={} requests={} checksum={} wall_ns={} cpu_ns={} p95_ns={} observe_ns={}",
            phase, mode.label(), a[2], visible, workers, requests, checksum,
            wall_ns, process_cpu_ns, p95_ns, observation_ns
        ).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invariant_same_checksum_for_varied_worker_counts() {
        for requests in [3, 19, 40] {
            let reference = run_queue(1, requests, 300, Instant::now()).0;
            for workers in [2, 4, 8] {
                assert_eq!(run_queue(workers, requests, 300, Instant::now()).0, reference);
            }
        }
    }
}
