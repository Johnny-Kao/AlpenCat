use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use runtime_core::BackendKind;
use runtime_cost_model::{CostModelContext, OnlineCostModel};
use runtime_machine::{GpuDeviceProfile, HostProfile, MachineProfile};
use runtime_selector::CalibrationProfile;
use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};

#[derive(Clone, Debug)]
struct Obs {
    scenario: String,
    rep: usize,
    taps: usize,
    n: usize,
    cpu_ns: f64,
    gpu_ns: f64,
}

#[derive(Clone, Copy, Debug)]
struct Point {
    taps: usize,
    n: usize,
    cpu_ns: f64,
    gpu_ns: f64,
}

fn parse_line(line: &str, scenario: &str, rep: usize) -> Option<Obs> {
    if !line.starts_with("fir_result ") {
        return None;
    }
    let mut taps = None;
    let mut n = None;
    let mut cpu_ns = None;
    let mut gpu_ns = None;
    for token in line.split_whitespace().skip(1) {
        let mut parts = token.splitn(2, '=');
        let key = parts.next()?;
        let value = parts.next()?;
        match key {
            "taps" => taps = value.parse().ok(),
            "n" => n = value.parse().ok(),
            "cpu_ns" => cpu_ns = value.parse().ok(),
            "gpu_host_ns" => gpu_ns = value.parse().ok(),
            _ => {}
        }
    }
    Some(Obs {
        scenario: scenario.to_string(),
        rep,
        taps: taps?,
        n: n?,
        cpu_ns: cpu_ns?,
        gpu_ns: gpu_ns?,
    })
}

fn parse_filename(name: &str) -> Option<(String, usize)> {
    let stem = name.strip_suffix(".txt")?;
    let pos = stem.rfind('_')?;
    let scenario = &stem[..pos];
    let rep = stem[pos + 1..].parse::<usize>().ok()?;
    Some((scenario.to_string(), rep))
}

fn load(dir: &Path) -> Vec<Obs> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read results dir") {
        let entry = entry.expect("entry");
        if !entry.file_type().expect("type").is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some((scenario, rep)) = parse_filename(&name) else {
            continue;
        };
        let text = fs::read_to_string(entry.path()).expect("read result file");
        for line in text.lines() {
            if let Some(obs) = parse_line(line, &scenario, rep) {
                out.push(obs);
            }
        }
    }
    out
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    values[values.len() / 2]
}

fn aggregate(obs: &[Obs]) -> BTreeMap<String, Vec<Point>> {
    let mut grouped: BTreeMap<(String, usize, usize), Vec<&Obs>> = BTreeMap::new();
    for row in obs {
        grouped
            .entry((row.scenario.clone(), row.taps, row.n))
            .or_default()
            .push(row);
    }
    let mut result: BTreeMap<String, Vec<Point>> = BTreeMap::new();
    for ((scenario, taps, n), rows) in grouped {
        result.entry(scenario).or_default().push(Point {
            taps,
            n,
            cpu_ns: median(rows.iter().map(|r| r.cpu_ns).collect()),
            gpu_ns: median(rows.iter().map(|r| r.gpu_ns).collect()),
        });
    }
    for points in result.values_mut() {
        points.sort_by_key(|p| (p.taps, p.n));
    }
    result
}

fn winner(p: Point) -> BackendKind {
    if p.cpu_ns <= p.gpu_ns {
        BackendKind::Cpu
    } else {
        BackendKind::Gpu
    }
}

fn cost_for(p: Point, route: BackendKind) -> f64 {
    match route {
        BackendKind::Cpu => p.cpu_ns,
        BackendKind::Gpu => p.gpu_ns,
        BackendKind::Serial => p.cpu_ns,
    }
}

fn regret_pct(p: Point, route: BackendKind) -> f64 {
    let oracle = p.cpu_ns.min(p.gpu_ns);
    100.0 * (cost_for(p, route) - oracle) / oracle.max(1.0)
}

fn summarize<F>(points: &[Point], mut select: F) -> (f64, f64, usize)
where
    F: FnMut(Point) -> BackendKind,
{
    let mut regrets = Vec::new();
    let mut wrong = 0usize;
    for &p in points {
        let route = select(p);
        let regret = regret_pct(p, route);
        if route != winner(p) {
            wrong += 1;
        }
        regrets.push(regret);
    }
    let mean = regrets.iter().sum::<f64>() / regrets.len().max(1) as f64;
    let max = regrets
        .into_iter()
        .fold(0.0f64, |acc, value| acc.max(value));
    (mean, max, wrong)
}

fn candidate_thresholds(values: &[usize]) -> Vec<usize> {
    let mut v = values.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut out = vec![0];
    for pair in v.windows(2) {
        out.push(pair[0] + (pair[1] - pair[0]) / 2 + 1);
    }
    if let Some(&last) = v.last() {
        out.push(last.saturating_add(1));
    }
    out
}

fn best_threshold<F>(train: &[Point], feature: F) -> usize
where
    F: Fn(Point) -> usize + Copy,
{
    let values: Vec<usize> = train.iter().copied().map(feature).collect();
    let mut best = (f64::INFINITY, 0usize);
    for threshold in candidate_thresholds(&values) {
        let total = train
            .iter()
            .copied()
            .map(|p| {
                let route = if feature(p) >= threshold {
                    BackendKind::Gpu
                } else {
                    BackendKind::Cpu
                };
                regret_pct(p, route)
            })
            .sum::<f64>();
        if total < best.0 {
            best = (total, threshold);
        }
    }
    best.1
}

fn empty_backend() -> BackendTelemetrySnapshot {
    BackendTelemetrySnapshot {
        in_flight: 0,
        completed: 0,
        failed: 0,
        work_items: 0,
        elapsed_nanos: 0,
        last_work_items: 0,
        last_elapsed_nanos: 0,
    }
}

fn telemetry() -> RuntimeTelemetrySnapshot {
    RuntimeTelemetrySnapshot {
        serial: empty_backend(),
        cpu: empty_backend(),
        gpu: empty_backend(),
    }
}

fn machine() -> MachineProfile {
    MachineProfile {
        host: HostProfile {
            os: "macos",
            architecture: "aarch64",
            logical_cpus: 3,
            memory_total_bytes: Some(7u64 << 30),
            memory_available_bytes: None,
        },
        gpus: vec![GpuDeviceProfile {
            name: "Apple Paravirtual device".into(),
            backend: "Metal".into(),
            device_type: "IntegratedGpu".into(),
            vendor_id: None,
            device_id: None,
            dedicated_memory_bytes: None,
        }],
    }
}

fn context<'a>(machine: &'a MachineProfile) -> CostModelContext<'a> {
    CostModelContext {
        cpu_eligible: true,
        gpu_eligible: true,
        machine,
        telemetry: telemetry(),
        bootstrap: CalibrationProfile::default(),
    }
}

fn train_m12_per_taps(model: &OnlineCostModel, train: &[Point]) {
    for &p in train {
        let task = format!("fir-taps-{}", p.taps);
        model.observe(
            &task,
            BackendKind::Cpu,
            p.n,
            Duration::from_nanos(p.cpu_ns.round() as u64),
            true,
        );
        model.observe(
            &task,
            BackendKind::Gpu,
            p.n,
            Duration::from_nanos(p.gpu_ns.round() as u64),
            true,
        );
    }
}

fn train_m12_work(model: &OnlineCostModel, train: &[Point]) {
    for &p in train {
        let work = p.n.saturating_mul(p.taps);
        model.observe(
            "fir-family",
            BackendKind::Cpu,
            work,
            Duration::from_nanos(p.cpu_ns.round() as u64),
            true,
        );
        model.observe(
            "fir-family",
            BackendKind::Gpu,
            work,
            Duration::from_nanos(p.gpu_ns.round() as u64),
            true,
        );
    }
}


fn train_m12_from_observations(model: &OnlineCostModel, obs: &[Obs], scenario: &str) {
    let mut rows: Vec<&Obs> = obs.iter().filter(|r| r.scenario == scenario).collect();
    rows.sort_by_key(|r| (r.rep, r.taps, r.n));
    for row in rows {
        let task = format!("fir-taps-{}", row.taps);
        model.observe(
            &task,
            BackendKind::Cpu,
            row.n,
            Duration::from_nanos(row.cpu_ns.round() as u64),
            true,
        );
        model.observe(
            &task,
            BackendKind::Gpu,
            row.n,
            Duration::from_nanos(row.gpu_ns.round() as u64),
            true,
        );
    }
}

fn online_selected_feedback(
    obs: &[Obs],
    scenario: &str,
    machine: &MachineProfile,
) -> (f64, f64, usize, usize) {
    let model = OnlineCostModel::default();
    train_m12_from_observations(&model, obs, "idle_pre");

    let mut rows: Vec<&Obs> = obs.iter().filter(|r| r.scenario == scenario).collect();
    rows.sort_by_key(|r| (r.rep, r.taps, r.n));

    let mut regrets = Vec::new();
    let mut wrong = 0usize;
    let mut switches = 0usize;
    let mut previous: BTreeMap<(usize, usize), BackendKind> = BTreeMap::new();

    for row in rows {
        let task = format!("fir-taps-{}", row.taps);
        let route = model.decide(&task, row.n, context(machine)).backend;
        let point = Point {
            taps: row.taps,
            n: row.n,
            cpu_ns: row.cpu_ns,
            gpu_ns: row.gpu_ns,
        };
        let actual = winner(point);
        wrong += usize::from(route != actual);
        regrets.push(regret_pct(point, route));

        let key = (row.taps, row.n);
        if previous.insert(key, route).is_some_and(|prev| prev != route) {
            switches += 1;
        }

        // Realistic one-arm feedback: after the decision, only the selected
        // backend's observed duration is fed back into M12.
        let elapsed = match route {
            BackendKind::Gpu => row.gpu_ns,
            BackendKind::Cpu | BackendKind::Serial => row.cpu_ns,
        };
        model.observe(
            &task,
            route,
            row.n,
            Duration::from_nanos(elapsed.round() as u64),
            true,
        );
    }

    let mean = regrets.iter().sum::<f64>() / regrets.len().max(1) as f64;
    let max = regrets.into_iter().fold(0.0f64, |acc, x| acc.max(x));
    (mean, max, wrong, switches)
}

fn online_dual_feedback_upper_bound(
    obs: &[Obs],
    scenario: &str,
    machine: &MachineProfile,
) -> (f64, f64, usize, usize) {
    let model = OnlineCostModel::default();
    train_m12_from_observations(&model, obs, "idle_pre");

    let mut rows: Vec<&Obs> = obs.iter().filter(|r| r.scenario == scenario).collect();
    rows.sort_by_key(|r| (r.rep, r.taps, r.n));

    let mut regrets = Vec::new();
    let mut wrong = 0usize;
    let mut switches = 0usize;
    let mut previous: BTreeMap<(usize, usize), BackendKind> = BTreeMap::new();

    for row in rows {
        let task = format!("fir-taps-{}", row.taps);
        let route = model.decide(&task, row.n, context(machine)).backend;
        let point = Point {
            taps: row.taps,
            n: row.n,
            cpu_ns: row.cpu_ns,
            gpu_ns: row.gpu_ns,
        };
        let actual = winner(point);
        wrong += usize::from(route != actual);
        regrets.push(regret_pct(point, route));

        let key = (row.taps, row.n);
        if previous.insert(key, route).is_some_and(|prev| prev != route) {
            switches += 1;
        }

        // Research-only upper bound: observe both paths after each point.
        model.observe(
            &task,
            BackendKind::Cpu,
            row.n,
            Duration::from_nanos(row.cpu_ns.round() as u64),
            true,
        );
        model.observe(
            &task,
            BackendKind::Gpu,
            row.n,
            Duration::from_nanos(row.gpu_ns.round() as u64),
            true,
        );
    }

    let mean = regrets.iter().sum::<f64>() / regrets.len().max(1) as f64;
    let max = regrets.into_iter().fold(0.0f64, |acc, x| acc.max(x));
    (mean, max, wrong, switches)
}

fn main() {
    let dir = env::args().nth(1).expect("usage: route-policy-replay RESULTS_DIR");
    let obs = load(Path::new(&dir));
    assert!(!obs.is_empty(), "no FIR observations found");
    let aggregated = aggregate(&obs);
    let idle = aggregated.get("idle_pre").expect("idle_pre scenario");
    let machine = machine();

    let n_threshold = best_threshold(idle, |p| p.n);
    let work_threshold = best_threshold(idle, |p| p.n.saturating_mul(p.taps));

    let mut per_taps_threshold = BTreeMap::new();
    let taps_values: BTreeSet<usize> = idle.iter().map(|p| p.taps).collect();
    for taps in taps_values {
        let subset: Vec<Point> = idle.iter().copied().filter(|p| p.taps == taps).collect();
        per_taps_threshold.insert(taps, best_threshold(&subset, |p| p.n));
    }

    let m12_per_taps = OnlineCostModel::default();
    train_m12_per_taps(&m12_per_taps, idle);

    let m12_work = OnlineCostModel::default();
    train_m12_work(&m12_work, idle);

    println!("# Route-policy replay");
    println!("train_scenario=idle_pre");
    println!("global_n_threshold={n_threshold}");
    println!("global_work_threshold={work_threshold}");
    println!("per_taps_thresholds={per_taps_threshold:?}");
    println!();
    println!("| scenario | policy | mean_regret_pct | max_regret_pct | wrong_routes | points |");
    println!("|---|---|---:|---:|---:|---:|");

    for (scenario, points) in &aggregated {
        let policies: Vec<(&str, Box<dyn FnMut(Point) -> BackendKind>)> = vec![
            ("oracle", Box::new(|p| winner(p))),
            (
                "global_n",
                Box::new(|p| {
                    if p.n >= n_threshold {
                        BackendKind::Gpu
                    } else {
                        BackendKind::Cpu
                    }
                }),
            ),
            (
                "global_nxtaps",
                Box::new(|p| {
                    if p.n.saturating_mul(p.taps) >= work_threshold {
                        BackendKind::Gpu
                    } else {
                        BackendKind::Cpu
                    }
                }),
            ),
            (
                "per_taps_threshold",
                Box::new(|p| {
                    if p.n >= *per_taps_threshold.get(&p.taps).expect("taps threshold") {
                        BackendKind::Gpu
                    } else {
                        BackendKind::Cpu
                    }
                }),
            ),
            (
                "m12_per_taps",
                Box::new(|p| {
                    let task = format!("fir-taps-{}", p.taps);
                    m12_per_taps.decide(&task, p.n, context(&machine)).backend
                }),
            ),
            (
                "m12_nxtaps",
                Box::new(|p| {
                    m12_work
                        .decide(
                            "fir-family",
                            p.n.saturating_mul(p.taps),
                            context(&machine),
                        )
                        .backend
                }),
            ),
        ];

        for (name, mut policy) in policies {
            let (mean, max, wrong) = summarize(points, |p| policy(p));
            println!(
                "| {scenario} | {name} | {mean:.3} | {max:.3} | {wrong} | {} |",
                points.len()
            );
        }
    }


    println!();
    println!("# Online replay with measured per-call feedback");
    println!("The selected-feedback row is implementable: only the chosen backend is observed.");
    println!("The dual-feedback row is a research upper bound because both paths are observed.");
    println!("| scenario | policy | mean_regret_pct | max_regret_pct | wrong_calls | route_switches | calls |");
    println!("|---|---|---:|---:|---:|---:|---:|");
    for scenario in aggregated.keys().filter(|s| s.as_str() != "idle_pre") {
        let call_count = obs.iter().filter(|r| &r.scenario == scenario).count();
        let (mean, max, wrong, switches) =
            online_selected_feedback(&obs, scenario, &machine);
        println!(
            "| {scenario} | m12_online_selected_feedback | {mean:.3} | {max:.3} | {wrong} | {switches} | {call_count} |"
        );

        let (mean, max, wrong, switches) =
            online_dual_feedback_upper_bound(&obs, scenario, &machine);
        println!(
            "| {scenario} | m12_online_dual_feedback_upper_bound | {mean:.3} | {max:.3} | {wrong} | {switches} | {call_count} |"
        );
    }

    println!();
    println!("# Leave-one-size-out on idle_pre");
    println!("| model | correct | points | mean_regret_pct | max_regret_pct |");
    println!("|---|---:|---:|---:|---:|");

    let mut correct = 0usize;
    let mut regrets = Vec::new();
    for (idx, &test) in idle.iter().enumerate() {
        let model = OnlineCostModel::default();
        let train: Vec<Point> = idle
            .iter()
            .enumerate()
            .filter_map(|(i, &p)| (i != idx).then_some(p))
            .collect();
        train_m12_per_taps(&model, &train);
        let task = format!("fir-taps-{}", test.taps);
        let route = model.decide(&task, test.n, context(&machine)).backend;
        correct += usize::from(route == winner(test));
        regrets.push(regret_pct(test, route));
    }
    let mean = regrets.iter().sum::<f64>() / regrets.len() as f64;
    let max = regrets.into_iter().fold(0.0f64, |acc, x| acc.max(x));
    println!("| m12_per_taps | {correct} | {} | {mean:.3} | {max:.3} |", idle.len());
}
