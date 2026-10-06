use std::collections::BTreeMap;
use std::env;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use runtime_api::{
    BackendKind, ExecutionBudget, ExecutionMode, Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};

const WORKLOAD: &str = "memory-column-transform-u64";
const MIN_ITEMS: usize = 4_096;
const MAX_ITEMS: usize = 4_194_304;
const VALIDATION_SIZES: [usize; 6] =
    [4_096, 16_384, 65_536, 262_144, 1_048_576, 4_194_304];

#[derive(Debug, Clone)]
struct Point {
    work_items: usize,
    serial_ns: u64,
    cpu_ns: Option<u64>,
    consistency: f64,
    margin_pct: f64,
}

impl Point {
    fn winner(&self) -> BackendKind {
        match self.cpu_ns {
            Some(cpu) if cpu < self.serial_ns => BackendKind::Cpu,
            _ => BackendKind::Serial,
        }
    }
}

#[derive(Debug)]
struct Discovery {
    status: &'static str,
    lower_serial_max: Option<usize>,
    upper_cpu_max: Option<usize>,
    probe_elapsed_ns: u64,
    points: Vec<Point>,
}

fn median(values: &[u64]) -> u64 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn checksum(values: &[u64]) -> u64 {
    values.iter().fold(0_u64, |acc, value| acc ^ value)
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
        .expect("interval probe route must execute");
    let backend = handle.decision().backend;
    let result = runtime.wait(handle);
    (backend, result.value)
}

fn measure_pair(
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &Arc<Vec<u64>>,
    n: usize,
    repeats: usize,
) -> Point {
    let mut serial_samples = Vec::with_capacity(repeats);
    let mut cpu_samples = Vec::with_capacity(repeats);
    let mut pair_winners = Vec::with_capacity(repeats);
    let mut expected_checksum = None;
    let mut cpu_available = true;

    for repeat in 0..repeats {
        let modes = if repeat % 2 == 0 {
            [ExecutionMode::Serial, ExecutionMode::Cpu]
        } else {
            [ExecutionMode::Cpu, ExecutionMode::Serial]
        };
        let mut serial_elapsed = None;
        let mut cpu_elapsed = None;

        for mode in modes {
            let started = Instant::now();
            let (backend, values) = execute(runtime, task, input, n, mode);
            let elapsed = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            black_box(values.last().copied().unwrap_or_default());
            let sum = checksum(&values);

            if let Some(expected) = expected_checksum {
                assert_eq!(sum, expected, "route output mismatch");
            } else {
                expected_checksum = Some(sum);
            }

            match mode {
                ExecutionMode::Serial => {
                    assert_eq!(backend, BackendKind::Serial);
                    serial_samples.push(elapsed);
                    serial_elapsed = Some(elapsed);
                }
                ExecutionMode::Cpu => {
                    if backend == BackendKind::Cpu {
                        cpu_samples.push(elapsed);
                        cpu_elapsed = Some(elapsed);
                    } else {
                        cpu_available = false;
                    }
                }
                ExecutionMode::Gpu | ExecutionMode::Auto => unreachable!(),
            }
        }

        if let (Some(serial), Some(cpu)) = (serial_elapsed, cpu_elapsed) {
            pair_winners.push(if cpu < serial {
                BackendKind::Cpu
            } else {
                BackendKind::Serial
            });
        }
    }

    let serial_ns = median(&serial_samples);
    let cpu_ns = if cpu_available && cpu_samples.len() == repeats {
        Some(median(&cpu_samples))
    } else {
        None
    };
    let median_winner = match cpu_ns {
        Some(cpu) if cpu < serial_ns => BackendKind::Cpu,
        _ => BackendKind::Serial,
    };
    let agreeing = pair_winners
        .iter()
        .filter(|winner| **winner == median_winner)
        .count();
    let consistency = if pair_winners.is_empty() {
        1.0
    } else {
        agreeing as f64 / pair_winners.len() as f64
    };
    let margin_pct = match cpu_ns {
        Some(cpu) => {
            let best = serial_ns.min(cpu) as f64;
            if best == 0.0 {
                0.0
            } else {
                100.0 * serial_ns.abs_diff(cpu) as f64 / best
            }
        }
        None => 0.0,
    };

    Point {
        work_items: n,
        serial_ns,
        cpu_ns,
        consistency,
        margin_pct,
    }
}

fn get_point(
    cache: &mut BTreeMap<usize, Point>,
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &Arc<Vec<u64>>,
    n: usize,
    repeats: usize,
    max_points: usize,
) -> Option<Point> {
    if let Some(point) = cache.get(&n) {
        return Some(point.clone());
    }
    if cache.len() >= max_points {
        return None;
    }
    let point = measure_pair(runtime, task, input, n, repeats);
    cache.insert(n, point.clone());
    Some(point)
}

fn discovery(
    status: &'static str,
    lower_serial_max: Option<usize>,
    upper_cpu_max: Option<usize>,
    started: Instant,
    cache: BTreeMap<usize, Point>,
) -> Discovery {
    Discovery {
        status,
        lower_serial_max,
        upper_cpu_max,
        probe_elapsed_ns: started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        points: cache.into_values().collect(),
    }
}

fn discover_interval(
    runtime: &Runtime,
    task: &TaskDefinition,
    input: &Arc<Vec<u64>>,
    start_boundary: usize,
    repeats: usize,
    max_points: usize,
) -> Discovery {
    let started = Instant::now();
    let mut cache = BTreeMap::new();
    let start = start_boundary.clamp(MIN_ITEMS, MAX_ITEMS);
    let first = get_point(
        &mut cache,
        runtime,
        task,
        input,
        start,
        repeats,
        max_points,
    )
    .expect("first interval probe point");

    if first.cpu_ns.is_none() {
        return discovery("CpuUnavailable", None, None, started, cache);
    }

    let (lower_serial_max, first_cpu) = if first.winner() == BackendKind::Serial {
        let mut last_serial = start;
        let mut current = start;
        let cpu_point = loop {
            let next = current.saturating_mul(2).min(MAX_ITEMS);
            if next == current {
                break None;
            }
            let Some(point) = get_point(
                &mut cache,
                runtime,
                task,
                input,
                next,
                repeats,
                max_points,
            ) else {
                break None;
            };
            if point.cpu_ns.is_none() {
                return discovery(
                    "CpuUnavailable",
                    Some(last_serial),
                    None,
                    started,
                    cache,
                );
            }
            if point.winner() == BackendKind::Cpu {
                break Some(next);
            }
            last_serial = next;
            current = next;
        };

        match cpu_point {
            Some(cpu) => (Some(last_serial), cpu),
            None => {
                let status = if cache.len() >= max_points {
                    "BudgetExhaustedBeforeCpu"
                } else {
                    "NoCpuWindow"
                };
                return discovery(status, Some(last_serial), None, started, cache);
            }
        }
    } else {
        let mut lowest_cpu = start;
        let mut current = start;
        let lower = loop {
            let next = (current / 2).max(MIN_ITEMS);
            if next == current {
                break Some(0);
            }
            let Some(point) = get_point(
                &mut cache,
                runtime,
                task,
                input,
                next,
                repeats,
                max_points,
            ) else {
                break None;
            };
            if point.cpu_ns.is_none() {
                return discovery("CpuUnavailable", None, None, started, cache);
            }
            if point.winner() == BackendKind::Serial {
                break Some(next);
            }
            lowest_cpu = next;
            current = next;
        };

        match lower {
            Some(lower) => (Some(lower), lowest_cpu),
            None => {
                return discovery(
                    "BudgetExhaustedFindingLower",
                    None,
                    None,
                    started,
                    cache,
                );
            }
        }
    };

    let mut last_cpu = first_cpu;
    let mut current = first_cpu;
    loop {
        let next = current.saturating_mul(2).min(MAX_ITEMS);
        if next == current {
            return discovery(
                "CpuThroughMax",
                lower_serial_max,
                None,
                started,
                cache,
            );
        }
        let Some(point) = get_point(
            &mut cache,
            runtime,
            task,
            input,
            next,
            repeats,
            max_points,
        ) else {
            return discovery(
                "BudgetExhaustedFindingUpper",
                lower_serial_max,
                Some(last_cpu),
                started,
                cache,
            );
        };
        if point.cpu_ns.is_none() {
            return discovery(
                "CpuUnavailable",
                lower_serial_max,
                Some(last_cpu),
                started,
                cache,
            );
        }
        if point.winner() == BackendKind::Serial {
            return discovery(
                "IntervalFound",
                lower_serial_max,
                Some(last_cpu),
                started,
                cache,
            );
        }
        last_cpu = next;
        current = next;
    }
}

fn interval_route(n: usize, lower: usize, upper: Option<usize>) -> BackendKind {
    if n > lower && upper.map(|value| n <= value).unwrap_or(true) {
        BackendKind::Cpu
    } else {
        BackendKind::Serial
    }
}

fn main() {
    let regime = env::var("ALPENCAT_REGIME").unwrap_or_else(|_| "half".to_string());
    let start_boundary = env::var("ALPENCAT_START_BOUNDARY")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16_384);
    let repeats = env::var("ALPENCAT_REPEATS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0 && value % 2 == 1)
        .unwrap_or(3);
    let max_points = env::var("ALPENCAT_INTERVAL_MAX_POINTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value >= 2)
        .unwrap_or(7);

    let runtime = Runtime::with_config(RuntimeConfig {
        execution_budget: ExecutionBudget::default(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new(WORKLOAD);
    let input = build_input(MAX_ITEMS);
    let result = discover_interval(
        &runtime,
        &task,
        &input,
        start_boundary,
        repeats,
        max_points,
    );

    let validation: Vec<Point> = VALIDATION_SIZES
        .iter()
        .map(|n| measure_pair(&runtime, &task, &input, *n, repeats))
        .collect();
    let oracle_cost: f64 = validation
        .iter()
        .map(|point| {
            point
                .cpu_ns
                .map(|cpu| cpu.min(point.serial_ns))
                .unwrap_or(point.serial_ns) as f64
        })
        .sum();

    let mut best_scalar_cost = f64::INFINITY;
    for boundary in std::iter::once(0).chain(VALIDATION_SIZES) {
        let cost: f64 = validation
            .iter()
            .map(|point| {
                if point.work_items <= boundary {
                    point.serial_ns as f64
                } else {
                    point.cpu_ns.unwrap_or(point.serial_ns) as f64
                }
            })
            .sum();
        best_scalar_cost = best_scalar_cost.min(cost);
    }

    let interval_cost = match (result.lower_serial_max, result.status) {
        (Some(lower), "IntervalFound" | "CpuThroughMax") => validation
            .iter()
            .map(|point| {
                if interval_route(point.work_items, lower, result.upper_cpu_max)
                    == BackendKind::Cpu
                {
                    point.cpu_ns.unwrap_or(point.serial_ns) as f64
                } else {
                    point.serial_ns as f64
                }
            })
            .sum::<f64>(),
        _ => f64::NAN,
    };
    let scalar_gap_pct = 100.0 * (best_scalar_cost - oracle_cost) / oracle_cost;
    let interval_gap_pct = if interval_cost.is_nan() {
        f64::NAN
    } else {
        100.0 * (interval_cost - oracle_cost) / oracle_cost
    };
    let min_consistency = result
        .points
        .iter()
        .map(|point| point.consistency)
        .fold(1.0_f64, f64::min);
    let min_margin = result
        .points
        .iter()
        .map(|point| point.margin_pct)
        .fold(f64::INFINITY, f64::min);

    let lower_json = result
        .lower_serial_max
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let upper_json = result
        .upper_cpu_max
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let interval_gap_json = if interval_gap_pct.is_nan() {
        "null".to_string()
    } else {
        format!("{interval_gap_pct:.6}")
    };

    println!(
        "{{\"record_type\":\"interval_summary\",\"schema_version\":1,\"workload\":\"{}\",\"regime\":\"{}\",\"max_points\":{},\"start_boundary\":{},\"status\":\"{}\",\"probe_count\":{},\"probe_elapsed_ns\":{},\"lower_serial_max\":{},\"upper_cpu_max\":{},\"min_probe_consistency\":{:.6},\"min_probe_margin_pct\":{:.6},\"best_scalar_gap_pct\":{:.6},\"inferred_interval_gap_pct\":{}}}",
        WORKLOAD,
        regime,
        max_points,
        start_boundary,
        result.status,
        result.points.len(),
        result.probe_elapsed_ns,
        lower_json,
        upper_json,
        min_consistency,
        if min_margin.is_infinite() { 0.0 } else { min_margin },
        scalar_gap_pct,
        interval_gap_json,
    );

    for point in &result.points {
        println!(
            "{{\"record_type\":\"interval_probe_point\",\"schema_version\":1,\"regime\":\"{}\",\"work_items\":{},\"winner\":\"{:?}\",\"serial_ns\":{},\"cpu_ns\":{},\"consistency\":{:.6},\"margin_pct\":{:.6}}}",
            regime,
            point.work_items,
            point.winner(),
            point.serial_ns,
            point
                .cpu_ns
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            point.consistency,
            point.margin_pct,
        );
    }
}
