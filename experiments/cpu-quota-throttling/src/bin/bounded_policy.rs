//! Explicit optional bounded worker-selection study under *real* Linux cgroup quota.
//! No background scheduler, automatic production policy, or fake work.
#![cfg(target_os = "linux")]
use alpencat_resource::{snapshot, Reading, ResourceSnapshot, ResourceState};
use std::{
    env,
    hint::black_box,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Fixed1, Fixed2, Fixed4, Fixed8,
    DirectQuota, ResourceQuota,
    DirectSearch, ResourceSearch,
}
impl Mode {
    fn parse(s: &str) -> Self {
        match s {
            "fixed1" => Self::Fixed1, "fixed2" => Self::Fixed2,
            "fixed4" => Self::Fixed4, "fixed8" => Self::Fixed8,
            "direct" => Self::DirectQuota, "alpencat" => Self::ResourceQuota,
            "direct_search" => Self::DirectSearch, "alpencat_search" => Self::ResourceSearch,
            _ => panic!("unknown mode {s}"),
        }
    }
    fn is_search(self) -> bool { matches!(self, Self::DirectSearch | Self::ResourceSearch) }
}

fn capacity(s: &ResourceSnapshot) -> Option<f64> {
    match &s.effective_cpu_capacity {
        Reading::Known(n) if n.is_finite() && *n > 0.0 => Some(*n),
        _ => None,
    }
}
fn quota_workers(q: f64) -> usize { (q.ceil() as usize).clamp(1, 8) }

#[inline(never)]
fn compute(index: usize, iterations: usize) -> u64 {
    let mut v = (index as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ 0xd1b54a32d192ed03;
    for i in 0..iterations {
        v ^= (i as u64).wrapping_add((index as u64) << 19)
            .wrapping_mul(0xbf58476d1ce4e5b9);
        v = v.rotate_left(17).wrapping_mul(0x94d049bb133111eb);
    }
    black_box(v)
}

/// All policies compute the exact same request indices in the same windows.
/// Window-level latency is from enqueue to completion, not an HTTP response.
fn window(workers: usize, request_base: usize, requests: usize, iterations: usize)
    -> (u64, u128, u128, Vec<u64>)
{
    let started = Instant::now();
    let cpu_started = cpu_ns();
    let cursor = AtomicUsize::new(0);
    let completions: Vec<_> = (0..requests).map(|_| AtomicU64::new(0)).collect();
    let sum = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers).map(|_| {
            let cursor = &cursor;
            let completions = &completions;
            scope.spawn(move || {
                let mut local = 0u64;
                loop {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    if i >= requests { break; }
                    local = local.wrapping_add(compute(request_base + i, iterations));
                    completions[i].store(started.elapsed().as_nanos() as u64, Ordering::Release);
                }
                local
            })
        }).collect();
        handles.into_iter().map(|h| h.join().expect("worker panicked"))
            .fold(0_u64, u64::wrapping_add)
    });
    let wall = started.elapsed().as_nanos();
    let cpu = cpu_ns() - cpu_started;
    let times: Vec<u64> = completions.iter().map(|n| n.load(Ordering::Acquire)).collect();
    assert!(times.iter().all(|x| *x > 0));
    (sum, wall, cpu, times)
}
fn p95(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values[((values.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)]
}
fn median_two(a: u128, b: u128) -> u128 { (a + b) / 2 }

/// Objective: choose lowest observed p95 among candidate workers whose CPU
/// work consumption is no more than 4% above the minimum observed CPU.
/// Hysteresis: require >5% p95 improvement to switch from incumbent.
/// The policy does not know ground truth; all eight exploratory windows count.
fn pick(probes: &[Vec<(u128,u128)>; 4], incumbent: usize) -> usize {
    let candidates = [1,2,4,8];
    let observed: Vec<(u128,u128)> = probes.iter().map(|p| {
        (median_two(p[0].0,p[1].0), median_two(p[0].1,p[1].1))
    }).collect();
    let min_cpu = observed.iter().map(|x| x.1).min().unwrap();
    let eligible = |i: usize| observed[i].1 <= min_cpu * 104 / 100;
    let winner = (0..4).filter(|i| eligible(*i))
        .min_by_key(|i| observed[*i].0).unwrap();
    let incumbent_index = candidates.iter().position(|v| *v == incumbent).unwrap();
    if winner != incumbent_index && observed[winner].0 * 100 >= observed[incumbent_index].0 * 95 {
        incumbent
    } else {
        candidates[winner]
    }
}

fn main() {
    let a: Vec<_> = env::args().collect();
    assert_eq!(a.len(), 4, "usage: bounded_policy <mode> <short|long> <socket>");
    let mode = Mode::parse(&a[1]);
    let (request_windows, iterations) = match a[2].as_str() {
        "short" => (16usize, 900_000usize),
        "long" => (28usize, 1_800_000usize),
        _ => panic!("invalid workload"),
    };
    const WINDOWS: usize = 12;
    // 8 real-workload exploration windows, followed by 4 exploitation windows.
    const PROBE_ORDER: [usize; 8] = [1,2,4,8,8,4,2,1];
    let mut stream = {
        let started = Instant::now();
        loop {
            match UnixStream::connect(&a[3]) {
                Ok(v) => break v,
                Err(e) if started.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                    black_box(e);
                },
                Err(e) => panic!("controller connect failed: {e}"),
            }
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(35))).unwrap();
    writeln!(stream, "READY pid={}", std::process::id()).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let state = ResourceState::new();
    let mut incumbent = 4;
    for phase in 0..3 {
        let mut command = String::new();
        assert!(reader.read_line(&mut command).unwrap() > 0, "controller disconnected");
        let expected_milli: u32 = command.trim().strip_prefix("GO ").unwrap().parse().unwrap();
        let verifier = snapshot(); // same premeasurement environmental verification for all modes
        let actual = capacity(&verifier).expect("cgroup quota unavailable");
        assert_eq!((actual*1000.0).round() as u32, expected_milli);
        let start = Instant::now();
        let cpu_start = cpu_ns();
        let obs_start = Instant::now();
        let mode_quota = match mode {
            Mode::DirectQuota | Mode::DirectSearch => capacity(&snapshot()),
            Mode::ResourceQuota | Mode::ResourceSearch => {
                state.invalidate();
                let view = state.refresh().expect("inconsistent cgroup observation");
                capacity(view.fresh().expect("refresh not fresh"))
            },
            _ => Some(actual),
        }.expect("quota unavailable");
        assert!((mode_quota*1000.0).round() as u32 == expected_milli);
        let observe_ns = obs_start.elapsed().as_nanos();
        let fixed = match mode {
            Mode::Fixed1 => 1, Mode::Fixed2 => 2,
            Mode::Fixed4 => 4, Mode::Fixed8 => 8,
            _ => quota_workers(mode_quota),
        };
        let mut probes: [Vec<(u128,u128)>;4] = std::array::from_fn(|_| Vec::new());
        let mut selected = incumbent;
        let mut switches: usize = 0;
        let mut rollbacks: usize = 0;
        let mut checksum = 0u64;
        let mut window_latencies = Vec::with_capacity(WINDOWS*request_windows);
        let mut worker_history = Vec::with_capacity(WINDOWS);
        let mut reference_p95: u64 = 0;
        for w in 0..WINDOWS {
            let workers = if mode.is_search() {
                if w < PROBE_ORDER.len() { PROBE_ORDER[w] } else { selected }
            } else { fixed };
            worker_history.push(workers);
            let (result, wall_ns, cpu_ns, mut completions) =
                window(workers, w*request_windows, request_windows, iterations);
            checksum = checksum.wrapping_add(result);
            let local_p95 = p95(&mut completions);
            // Probe windows are real served requests and count toward wall/CPU/p95.
            window_latencies.extend(completions);
            if mode.is_search() {
                if w < PROBE_ORDER.len() {
                    let idx = [1,2,4,8].iter().position(|v| *v==workers).unwrap();
                    probes[idx].push((local_p95 as u128,cpu_ns));
                    if w == PROBE_ORDER.len()-1 {
                        selected = pick(&probes, incumbent);
                        if selected != incumbent { switches += 1; }
                        let idx = [1,2,4,8].iter().position(|v| *v == selected).unwrap();
                        reference_p95 = median_two(probes[idx][0].0,probes[idx][1].0) as u64;
                    }
                } else if w == PROBE_ORDER.len()+1 &&
                    (local_p95 as u128) * 100 > (reference_p95 as u128)*125 {
                    // Conservative one-time rollback: restore prior incumbent.
                    // The action is fully measured and never spins in a control loop.
                    if selected != incumbent {
                        selected = incumbent;
                        rollbacks += 1;
                        switches += 1;
                    }
                }
            }
            black_box(wall_ns);
        }
        if mode.is_search() { incumbent = selected; }
        let p95_ns = p95(&mut window_latencies);
        let wall_ns = start.elapsed().as_nanos();
        let process_cpu_ns = cpu_ns().saturating_sub(cpu_start);
        let average_workers = worker_history.iter().sum::<usize>() as f64 / WINDOWS as f64;
        writeln!(stream,
            "PHASE phase={} mode={} size={} quota_milli={} workers={} avg_workers={:.3} requests={} checksum={} wall_ns={} cpu_ns={} p95_ns={} observe_ns={} switches={} rollbacks={} probes={}",
            phase, a[1], a[2], expected_milli, selected, average_workers,
            WINDOWS*request_windows, checksum, wall_ns, process_cpu_ns,
            p95_ns, observe_ns, switches, rollbacks,
            if mode.is_search() { PROBE_ORDER.len() } else { 0 }
        ).unwrap();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_equivalent_across_parallelism() {
        for workers in [1,2,4,8] {
            let total = (0..12).map(|w|
                window(workers,w*4,4,300).0).fold(0u64,u64::wrapping_add);
            let reference = (0..48).map(|i| compute(i,300))
                .fold(0u64,u64::wrapping_add);
            assert_eq!(total,reference);
        }
    }
    #[test]
    fn hysteresis_does_not_change_for_small_gain() {
        // Tuple = measured p95, CPU. Candidate 4 improves 3% vs incumbent 2:
        let mut probes: [Vec<(u128,u128)>;4] = std::array::from_fn(|_| Vec::new());
        for (i,p95) in [150,100,97,120].iter().enumerate() {
            probes[i].push((*p95,100));
            probes[i].push((*p95,100));
        }
        assert_eq!(pick(&probes,2),2);
        for p in &mut probes[2] { p.0=80; }
        assert_eq!(pick(&probes,2),4);
    }
    #[test]
    fn cpu_budget_rejects_cpu_expensive_candidate() {
        let mut probes: [Vec<(u128,u128)>;4] = std::array::from_fn(|_| Vec::new());
        for (i,(p95,cpu)) in [(150,100),(120,100),(100,100),(70,115)].iter().enumerate(){
            probes[i].push((*p95,*cpu));
            probes[i].push((*p95,*cpu));
        }
        assert_eq!(pick(&probes,2),4);
    }
}
