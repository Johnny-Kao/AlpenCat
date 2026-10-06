use std::env;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionBudget, ExecutionMode,
    RevalidationStatus, Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};

const WORKLOAD: &str = "memory-column-transform-u64";
const SIZES: [usize; 6] = [4_096, 16_384, 65_536, 262_144, 1_048_576, 4_194_304];

#[derive(Debug)]
struct Measurement {
    samples_ns: Vec<u64>,
    actual_backend: BackendKind,
    checksum: u64,
}

fn checksum(values: &[u64]) -> u64 {
    values.iter().fold(0_u64, |acc, value| acc ^ value)
}

fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn samples_json(samples: &[u64]) -> String {
    samples
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn optional_samples_json(samples: Option<&[u64]>) -> String {
    match samples {
        Some(values) => format!("[{}]", samples_json(values)),
        None => "null".to_string(),
    }
}

fn optional_bool_json(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

fn build_input(len: usize) -> Arc<Vec<u64>> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        values.push(state);
    }
    Arc::new(values)
}

fn execute(
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &Arc<Vec<u64>>,
    n: usize,
    mode: ExecutionMode,
) -> (BackendKind, Vec<u64>) {
    let input = Arc::clone(input);
    let handle = runtime
        .submit_map(task, WorkRange::new(0, n), mode, move |index| {
            input[index]
                .rotate_left(7)
                .wrapping_add(0xD6E8_FEB8_6659_FD93)
        })
        .expect("memory workload route must execute");
    let backend = handle.decision().backend;
    let result = runtime.wait(handle);
    (backend, result.value)
}

fn measure(
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &Arc<Vec<u64>>,
    n: usize,
    mode: ExecutionMode,
    repeats: usize,
) -> Measurement {
    let mut samples_ns = Vec::with_capacity(repeats);
    let mut actual_backend = BackendKind::Serial;
    let mut result_checksum = 0_u64;

    for repeat in 0..repeats {
        let start = Instant::now();
        let (backend, values) = execute(runtime, task, input, n, mode);
        let elapsed = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        black_box(values.last().copied().unwrap_or_default());

        if repeat == 0 {
            actual_backend = backend;
            result_checksum = checksum(&values);
        } else {
            assert_eq!(
                backend, actual_backend,
                "backend changed within one measurement"
            );
            assert_eq!(
                checksum(&values),
                result_checksum,
                "memory workload output changed across repetitions"
            );
        }
        samples_ns.push(elapsed);
    }

    Measurement {
        samples_ns,
        actual_backend,
        checksum: result_checksum,
    }
}

fn main() {
    let regime = env::var("ALPENCAT_REGIME").unwrap_or_else(|_| "baseline-full".to_string());
    let repeats = env::var("ALPENCAT_REPEATS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0 && value % 2 == 1)
        .unwrap_or(3);
    let start_boundary = env::var("ALPENCAT_START_BOUNDARY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(262_144);
    let bootstrap = env::var("ALPENCAT_BOOTSTRAP")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let revalidation_points = env::var("ALPENCAT_REVALIDATION_POINTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(3);

    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(start_boundary, usize::MAX),
        execution_budget: ExecutionBudget::default(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new(WORKLOAD);
    let input = build_input(SIZES[SIZES.len() - 1]);

    let mut route_rows = Vec::new();
    for n in SIZES {
        let serial = measure(&runtime, &task, &input, n, ExecutionMode::Serial, repeats);
        assert_eq!(serial.actual_backend, BackendKind::Serial);

        let cpu = measure(&runtime, &task, &input, n, ExecutionMode::Cpu, repeats);
        let cpu_available = cpu.actual_backend == BackendKind::Cpu;
        let equivalent = cpu_available.then_some(serial.checksum == cpu.checksum);
        if let Some(false) = equivalent {
            panic!("serial and CPU memory routes are not equivalent at n={n}");
        }
        route_rows.push((n, serial, cpu, cpu_available, equivalent));
    }

    let (status, measurement_count, revalidation_elapsed_ns) = if bootstrap {
        let mut boundary = SIZES[SIZES.len() - 1];
        let mut previous = 0;
        for (n, serial, cpu, cpu_available, _) in &route_rows {
            if *cpu_available && median(&cpu.samples_ns) < median(&serial.samples_ns) {
                boundary = previous;
                break;
            }
            previous = *n;
        }
        runtime.publish_boundary(BoundaryProfile::new(boundary, usize::MAX));
        let bootstrap_cost = route_rows
            .iter()
            .map(|(_, serial, cpu, cpu_available, _)| {
                median(&serial.samples_ns)
                    + if *cpu_available {
                        median(&cpu.samples_ns)
                    } else {
                        0
                    }
            })
            .sum();
        ("Bootstrap", route_rows.len(), bootstrap_cost)
    } else {
        runtime.invalidate_resources();
        let start = Instant::now();
        let outcome = runtime.revalidate_serial_cpu(
            BoundedRevalidationConfig::new(revalidation_points, SIZES[0], SIZES[SIZES.len() - 1]),
            |n, backend| {
                let mode = match backend {
                    BackendKind::Serial => ExecutionMode::Serial,
                    BackendKind::Cpu => ExecutionMode::Cpu,
                    BackendKind::Gpu => unreachable!(),
                };
                let measurement = measure(&runtime, &task, &input, n, mode, repeats);
                (measurement.actual_backend == backend).then(|| median(&measurement.samples_ns))
            },
        );
        let elapsed = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let status = match outcome.status {
            RevalidationStatus::NotStale => "NotStale",
            RevalidationStatus::Published => "Published",
            RevalidationStatus::NoLocalCrossover => "NoLocalCrossover",
            RevalidationStatus::RouteUnavailable(BackendKind::Serial) => "RouteUnavailable(Serial)",
            RevalidationStatus::RouteUnavailable(BackendKind::Cpu) => "RouteUnavailable(Cpu)",
            RevalidationStatus::RouteUnavailable(BackendKind::Gpu) => "RouteUnavailable(Gpu)",
            RevalidationStatus::InvalidatedDuringMeasurement => "InvalidatedDuringMeasurement",
        };
        (status, outcome.evidence.measurements.len(), elapsed)
    };

    let published = runtime.boundary_snapshot();
    println!(
        "{{\"record_type\":\"revalidation\",\"schema_version\":1,\"workload\":\"{}\",\"regime\":\"{}\",\"status\":\"{}\",\"measurement_count\":{},\"revalidation_elapsed_ns\":{},\"published_serial_max_items\":{},\"boundary_stale\":{},\"start_boundary\":{}}}",
        WORKLOAD,
        regime,
        status,
        measurement_count,
        revalidation_elapsed_ns,
        published.profile.serial_max_items,
        runtime.boundary_is_stale(),
        start_boundary,
    );

    for (n, serial, cpu, cpu_available, equivalent) in route_rows {
        let auto = measure(&runtime, &task, &input, n, ExecutionMode::Auto, repeats);
        let cpu_samples = cpu_available.then_some(cpu.samples_ns.as_slice());

        println!(
            "{{\"record_type\":\"point\",\"schema_version\":1,\"workload\":\"{}\",\"regime\":\"{}\",\"work_items\":{},\"weight\":1.0,\"parallelism\":{},\"serial_samples_ns\":[{}],\"cpu_samples_ns\":{},\"cpu_route_available\":{},\"output_equivalent\":{},\"auto_samples_ns\":[{}],\"auto_backend\":\"{:?}\",\"boundary_stale\":{},\"serial_checksum\":{},\"cpu_checksum\":{}}}",
            WORKLOAD,
            regime,
            n,
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1),
            samples_json(&serial.samples_ns),
            optional_samples_json(cpu_samples),
            cpu_available,
            optional_bool_json(equivalent),
            samples_json(&auto.samples_ns),
            auto.actual_backend,
            runtime.boundary_is_stale(),
            serial.checksum,
            if cpu_available {
                cpu.checksum.to_string()
            } else {
                "null".to_string()
            },
        );
    }
}
