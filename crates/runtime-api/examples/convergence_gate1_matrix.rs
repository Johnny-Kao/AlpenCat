//! Gate 1 only: test whether execution-regime changes create recoverable route regret.
//! Uses the existing published Runtime::submit_map Serial/Cpu code paths.
#![cfg(target_os = "linux")]

use runtime_api::{
    BackendKind, BoundaryProfile, ExecutionBudget, ExecutionMode, Runtime, RuntimeConfig,
    TaskDefinition, WorkRange,
};
use std::{
    hint::black_box,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::Arc,
    time::{Duration, Instant},
};

#[repr(C)]
struct Timespec { seconds: i64, nanos: i64 }
unsafe extern "C" {
    fn clock_gettime(clock: i32, out: *mut Timespec) -> i32;
}
fn cpu_time_ns() -> u128 {
    let mut now = Timespec { seconds: 0, nanos: 0 };
    assert_eq!(unsafe { clock_gettime(2, &mut now) }, 0);
    now.seconds as u128 * 1_000_000_000 + now.nanos as u128
}
fn compute(mut x: u64) -> u64 {
    for _ in 0..512 {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
    }
    black_box(x)
}
fn memory_read(input: &[u64], index: usize) -> u64 {
    let mask = input.len() - 1;
    let x = input[index & mask];
    let a = input[index.wrapping_mul(1_315_423_911).wrapping_add(17) & mask];
    let b = input[index.wrapping_mul(2_654_435_761).wrapping_add(97) & mask];
    let c = input[index.wrapping_mul(805_459_861).wrapping_add(193) & mask];
    black_box(x ^ a.rotate_left(11) ^ b.rotate_left(29) ^ c.rotate_left(47))
}
fn checksum(v: &[u64]) -> u64 {
    v.iter().fold(0, |a, b| a.wrapping_add(*b))
}
struct Sample {
    backend: BackendKind,
    elapsed_wall_ns: u128,
    elapsed_cpu_ns: u128,
    checksum: u64,
}
fn execute(
    runtime: &Runtime,
    task: &TaskDefinition,
    family: &str,
    size: usize,
    mode: ExecutionMode,
    memory: &Arc<Vec<u64>>,
) -> Sample {
    let wall_start = Instant::now();
    let cpu_start = cpu_time_ns();
    let values = match family {
        "compute" => runtime
            .submit_map(task, WorkRange::new(0, size), mode, |i| compute(i as u64 + 1)),
        "memory" => {
            let data = Arc::clone(memory);
            runtime.submit_map(task, WorkRange::new(0, size), mode, move |i| {
                memory_read(&data, i)
            })
        }
        _ => unreachable!(),
    }
    .expect("runtime route must execute");
    let backend = values.decision().backend;
    let v = runtime.wait(values).value;
    let cpu = cpu_time_ns() - cpu_start;
    let wall = wall_start.elapsed().as_nanos();
    Sample { backend, elapsed_wall_ns: wall, elapsed_cpu_ns: cpu, checksum: checksum(&v) }
}
fn med(v: &[u128]) -> u128 {
    let mut v = v.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}
fn point(
    runtime: &Runtime,
    task: &TaskDefinition,
    family: &str,
    size: usize,
    memory: &Arc<Vec<u64>>,
) -> (BackendKind, u128, u128, u128, u128, u64) {
    const REPEATS: usize = 7;
    // Warm up both candidate implementations: pool creation is not a route cost.
    for mode in [ExecutionMode::Serial, ExecutionMode::Cpu] {
        black_box(execute(runtime, task, family, size, mode, memory).checksum);
    }
    let mut serial_wall = Vec::new();
    let mut serial_cpu = Vec::new();
    let mut parallel_wall = Vec::new();
    let mut parallel_cpu = Vec::new();
    let mut observed_parallel: Option<BackendKind> = None;
    let mut hash: Option<u64> = None;
    for round in 0..REPEATS {
        let order = if round % 2 == 0 {
            [ExecutionMode::Serial, ExecutionMode::Cpu]
        } else {
            [ExecutionMode::Cpu, ExecutionMode::Serial]
        };
        for mode in order {
            let result = execute(runtime, task, family, size, mode, memory);
            if let Some(h) = hash { assert_eq!(h, result.checksum, "route correctness"); }
            else { hash = Some(result.checksum); }
            match mode {
                ExecutionMode::Serial => {
                    assert_eq!(result.backend, BackendKind::Serial);
                    serial_wall.push(result.elapsed_wall_ns);
                    serial_cpu.push(result.elapsed_cpu_ns);
                }
                ExecutionMode::Cpu => {
                    if let Some(b) = observed_parallel {
                        assert_eq!(b, result.backend, "route availability shifted during phase");
                    } else {
                        observed_parallel = Some(result.backend);
                    }
                    parallel_wall.push(result.elapsed_wall_ns);
                    parallel_cpu.push(result.elapsed_cpu_ns);
                }
                _ => unreachable!(),
            }
        }
    }
    (
        observed_parallel.expect("observed backend"), med(&serial_wall), med(&serial_cpu),
        med(&parallel_wall), med(&parallel_cpu), hash.unwrap(),
    )
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 2, "convergence_gate1_matrix <control unix socket>");
    let mut sock = UnixStream::connect(&args[1]).expect("control connection");
    sock.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    let mut reader = BufReader::new(sock.try_clone().unwrap());
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4096, usize::MAX),
        execution_budget: ExecutionBudget::new(2),
        ..RuntimeConfig::default()
    });
    let compute_task = TaskDefinition::new("gate1-compute");
    let memory_task = TaskDefinition::new("gate1-memory");
    let mut initial: Vec<u64> = Vec::with_capacity(1_048_576);
    let mut x = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..initial.capacity() {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        initial.push(x);
    }
    let memory = Arc::new(initial);
    writeln!(sock, "READY pid={}", std::process::id()).unwrap();
    for phase in 0..4 {
        let mut command = String::new();
        assert!(reader.read_line(&mut command).unwrap() > 0, "control unexpectedly closed");
        let requested = command.trim().strip_prefix("GO ").expect("GO command");
        let quota_milli: usize = requested.parse().unwrap();
        let parallelism = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        // The controller independently reads cpu.max and owned cgroup cpu.stat.
        for (family, sizes, task) in [
            ("compute", [256, 2_048, 16_384], &compute_task),
            ("memory", [8_192, 65_536, 524_288], &memory_task),
        ] {
            for n in sizes {
                let (cpu_backend, serial_wall, serial_cpu, parallel_wall, parallel_cpu, sum) =
                    point(&runtime, task, family, n, &memory);
                writeln!(sock, "POINT phase={phase} family={family} n={n} \
                    quota_milli={quota_milli} available_parallelism={parallelism} \
                    cpu_backend={cpu_backend:?} serial_wall_ns={serial_wall} \
                    serial_cpu_ns={serial_cpu} cpu_wall_ns={parallel_wall} \
                    cpu_cpu_ns={parallel_cpu} checksum={sum}").unwrap();
            }
        }
        writeln!(sock, "DONE phase={phase}").unwrap();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kernels_produce_repeatable_results() {
        let d = vec![19_u64; 1_024];
        assert_eq!(compute(7), compute(7));
        assert_eq!(memory_read(&d, 27), memory_read(&d, 27));
    }
}
