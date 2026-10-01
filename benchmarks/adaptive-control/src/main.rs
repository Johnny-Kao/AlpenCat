use std::hint::black_box;
use std::time::{Duration, Instant};

use runtime_core::BackendKind;
use runtime_cost_model::{CostModelContext, MachineFingerprint, OnlineCostModel};
use runtime_execution_planner::{AdaptiveExecutionPlanner, PlannerContext};
use runtime_machine::{GpuDeviceProfile, HostProfile, MachineProfile};
use runtime_rebalancer::{RebalanceAction, RebalanceSession};
use runtime_selector::CalibrationProfile;
use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};

fn backend(
    in_flight: usize,
    completed: u64,
    failed: u64,
    work_items: u64,
    elapsed_nanos: u64,
) -> BackendTelemetrySnapshot {
    BackendTelemetrySnapshot {
        in_flight,
        completed,
        failed,
        work_items,
        elapsed_nanos,
        last_work_items: 0,
        last_elapsed_nanos: 0,
    }
}

fn telemetry(
    cpu: BackendTelemetrySnapshot,
    gpu: BackendTelemetrySnapshot,
) -> RuntimeTelemetrySnapshot {
    RuntimeTelemetrySnapshot {
        serial: backend(0, 0, 0, 0, 0),
        cpu,
        gpu,
    }
}

fn machine() -> MachineProfile {
    MachineProfile {
        host: HostProfile {
            os: "benchmark",
            architecture: "x86_64",
            logical_cpus: 8,
            memory_total_bytes: Some(16 << 30),
            memory_available_bytes: Some(12 << 30),
        },
        gpus: vec![GpuDeviceProfile {
            name: "synthetic-gpu".into(),
            backend: "benchmark".into(),
            device_type: "DiscreteGpu".into(),
            vendor_id: Some(0x10de),
            device_id: Some(1),
            dedicated_memory_bytes: Some(8 << 30),
        }],
    }
}

fn context<'a>(
    machine: &'a MachineProfile,
    telemetry: RuntimeTelemetrySnapshot,
) -> CostModelContext<'a> {
    CostModelContext {
        cpu_eligible: true,
        gpu_eligible: true,
        machine,
        telemetry,
        bootstrap: CalibrationProfile::default(),
    }
}

fn train(model: &OnlineCostModel, machine: &MachineProfile) {
    let fp = MachineFingerprint::from_machine(machine);
    for &n in &[100usize, 1_000, 10_000, 100_000] {
        let cpu = Duration::from_nanos(10_000 + 5 * n as u64);
        let gpu = Duration::from_nanos(100_000 + n as u64);
        model.observe_detailed(
            "synthetic-crossover",
            BackendKind::Cpu,
            fp,
            runtime_cost_model::CostObservation::execution(n, cpu, true),
        );
        model.observe_detailed(
            "synthetic-crossover",
            BackendKind::Gpu,
            fp,
            runtime_cost_model::CostObservation::execution(n, gpu, true),
        );
    }
}

fn main() {
    let machine = machine();
    let model = OnlineCostModel::default();
    train(&model, &machine);

    let empty = telemetry(
        backend(0, 0, 0, 0, 0),
        backend(0, 0, 0, 0, 0),
    );

    println!("# adaptive-control benchmark");
    println!("machine_fingerprint={}", MachineFingerprint::from_machine(&machine).0);
    println!();
    println!("## learned crossover");
    println!("| work_items | selected | cpu_ns | gpu_ns |");
    println!("|---:|---|---:|---:|");

    for n in [1_000usize, 10_000, 22_500, 25_000, 50_000, 100_000] {
        let d = model.decide("synthetic-crossover", n, context(&machine, empty));
        let cpu = d.cpu.and_then(|v| v.predicted_nanos).unwrap_or(f64::NAN);
        let gpu = d.gpu.and_then(|v| v.predicted_nanos).unwrap_or(f64::NAN);
        println!("| {n} | {:?} | {:.1} | {:.1} |", d.backend, cpu, gpu);
    }

    const DECISION_ITERS: usize = 200_000;
    let start = Instant::now();
    let mut last = BackendKind::Serial;
    for i in 0..DECISION_ITERS {
        let n = 1_000 + (i % 100_000);
        last = black_box(model.decide(
            "synthetic-crossover",
            black_box(n),
            context(&machine, empty),
        ))
        .backend;
    }
    let decision_elapsed = start.elapsed();
    let decision_ns = decision_elapsed.as_nanos() as f64 / DECISION_ITERS as f64;

    let learned = model.decide(
        "synthetic-crossover",
        100_000,
        context(&machine, empty),
    );
    let plan = AdaptiveExecutionPlanner.plan(PlannerContext {
        work_items: 100_000,
        cost: learned,
        machine: &machine,
        telemetry: empty,
        gpu_range_eligible: true,
    });

    const PLAN_ITERS: usize = 200_000;
    let start = Instant::now();
    let mut last_chunk = 0usize;
    for _ in 0..PLAN_ITERS {
        let p = black_box(AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items: black_box(100_000),
            cost: learned,
            machine: &machine,
            telemetry: empty,
            gpu_range_eligible: true,
        }));
        last_chunk = p.chunk_size;
    }
    let plan_elapsed = start.elapsed();
    let plan_ns = plan_elapsed.as_nanos() as f64 / PLAN_ITERS as f64;

    let baseline = telemetry(
        backend(0, 100, 0, 100_000, 1_000_000),
        backend(0, 100, 0, 100_000, 600_000),
    );
    let stable = telemetry(
        backend(0, 102, 0, 102_000, 1_020_000),
        backend(0, 102, 0, 102_000, 612_000),
    );

    const KEEP_ITERS: usize = 1_000_000;
    let start = Instant::now();
    let mut keep_count = 0usize;
    for _ in 0..KEEP_ITERS {
        let mut session = RebalanceSession::new(plan.clone(), baseline);
        if matches!(
            black_box(session.consider(stable, 8, 1)),
            RebalanceAction::Keep
        ) {
            keep_count += 1;
        }
    }
    let keep_elapsed = start.elapsed();
    let keep_ns = keep_elapsed.as_nanos() as f64 / KEEP_ITERS as f64;

    let pressure_changed = telemetry(
        backend(6, 108, 0, 108_000, 1_080_000),
        backend(0, 108, 0, 108_000, 648_000),
    );
    const TRIGGER_ITERS: usize = 500_000;
    let start = Instant::now();
    let mut trigger_count = 0usize;
    for _ in 0..TRIGGER_ITERS {
        let mut session = RebalanceSession::new(plan.clone(), baseline);
        if matches!(
            black_box(session.consider(pressure_changed, 8, 1)),
            RebalanceAction::Replan(_)
        ) {
            trigger_count += 1;
        }
    }
    let trigger_elapsed = start.elapsed();
    let trigger_ns = trigger_elapsed.as_nanos() as f64 / TRIGGER_ITERS as f64;

    println!();
    println!("## control-plane overhead");
    println!("m12_decision_ns={decision_ns:.2}");
    println!("m13_plan_ns={plan_ns:.2}");
    println!("m14_keep_check_ns={keep_ns:.2}");
    println!("m14_trigger_check_ns={trigger_ns:.2}");
    println!("decision_last={last:?}");
    println!("plan_last_chunk={last_chunk}");
    println!("keep_count={keep_count}");
    println!("trigger_count={trigger_count}");

    assert_eq!(
        model.decide(
            "synthetic-crossover",
            1_000,
            context(&machine, empty)
        )
        .backend,
        BackendKind::Cpu
    );
    assert_eq!(
        model.decide(
            "synthetic-crossover",
            100_000,
            context(&machine, empty)
        )
        .backend,
        BackendKind::Gpu
    );
    assert_eq!(keep_count, KEEP_ITERS);
    assert_eq!(trigger_count, TRIGGER_ITERS);
}
