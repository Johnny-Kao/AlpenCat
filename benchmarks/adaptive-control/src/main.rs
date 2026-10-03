use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use runtime_core::BackendKind;
use runtime_broker::{BrokerCapacity, BrokerRequest};
use runtime_cost_model::{CostModelContext, MachineFingerprint, OnlineCostModel};
use runtime_execution_planner::{AdaptiveExecutionPlanner, PlannerContext};
use runtime_machine::{GpuDeviceProfile, HostProfile, MachineProfile};
use runtime_policy_cache::{ExecutionPolicyCache, PolicyCacheStatus};
use runtime_rebalancer::{RebalanceAction, RebalanceSession};
use runtime_selector::{
    CalibrationProfile, LazyLocalizedState, LocalizedBoundary, RecalibrationEconomics,
    RecentUseRate, ResourceEpoch,
};
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

fn benchmark_machine() -> MachineProfile {
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

fn control_backend(
    model: &OnlineCostModel,
    machine: &MachineProfile,
    telemetry: RuntimeTelemetrySnapshot,
    work_items: usize,
) -> BackendKind {
    let learned = model.decide(
        "synthetic-crossover",
        work_items,
        context(machine, telemetry),
    );
    AdaptiveExecutionPlanner
        .plan(PlannerContext {
            work_items,
            cost: learned,
            machine,
            telemetry,
            gpu_range_eligible: true,
        })
        .primary_backend
}

fn backend_to_u8(backend: BackendKind) -> u8 {
    match backend {
        BackendKind::Serial => 0,
        BackendKind::Cpu => 1,
        BackendKind::Gpu => 2,
    }
}

fn main() {
    let machine = benchmark_machine();
    let model = OnlineCostModel::default();
    train(&model, &machine);

    println!("# lazy-localized controller microbench");
    const LOCALIZED_ITERS: usize = 5_000_000;

    let boundary = LocalizedBoundary::new(32_768, 65_536);
    let idle_epoch = ResourceEpoch::new(false, false, false);
    let busy_epoch = ResourceEpoch::new(true, true, false);
    let economics = RecalibrationEconomics {
        estimated_cost_ns: 20_000,
        estimated_regret_per_use_ns: 1_000,
    };

    let start = Instant::now();
    let mut epoch_state = LazyLocalizedState::new(boundary, idle_epoch);
    let mut epoch_hits = 0usize;
    for _ in 0..LOCALIZED_ITERS {
        if black_box(epoch_state.observe_epoch(black_box(idle_epoch))) {
            epoch_hits += 1;
        }
    }
    let observe_epoch_same_ns = start.elapsed().as_nanos() as f64 / LOCALIZED_ITERS as f64;

    let start = Instant::now();
    let mut timing_state = LazyLocalizedState::new(boundary, idle_epoch);
    let mut timing_hits = 0usize;
    for i in 0..LOCALIZED_ITERS {
        let observed = if i & 1 == 0 { 1_050 } else { 1_080 };
        if black_box(timing_state.observe_selected_timing(1_000, black_box(observed))) {
            timing_hits += 1;
        }
    }
    let selected_timing_ns = start.elapsed().as_nanos() as f64 / LOCALIZED_ITERS as f64;

    let mut rate = RecentUseRate::new();
    rate.observe(0);
    rate.observe(1_000);
    rate.observe(2_000);
    let mut gate_state = LazyLocalizedState::new(boundary, idle_epoch);
    assert!(gate_state.observe_epoch(busy_epoch));

    let start = Instant::now();
    let mut gate_true = 0usize;
    for _ in 0..LOCALIZED_ITERS {
        if black_box(gate_state.should_recalibrate_at_rate(
            black_box(49_152),
            true,
            rate,
            economics,
            100_000,
        )) {
            gate_true += 1;
        }
    }
    let economics_gate_ns = start.elapsed().as_nanos() as f64 / LOCALIZED_ITERS as f64;

    let start = Instant::now();
    let mut rate_bench = RecentUseRate::new();
    for i in 0..LOCALIZED_ITERS {
        rate_bench.observe(black_box((i as u64 + 1) * 1_000));
    }
    let recent_use_update_ns = start.elapsed().as_nanos() as f64 / LOCALIZED_ITERS as f64;

    println!("lazy_observe_epoch_same_ns={observe_epoch_same_ns:.4}");
    println!("lazy_selected_timing_ns={selected_timing_ns:.4}");
    println!("lazy_economics_gate_ns={economics_gate_ns:.4}");
    println!("lazy_recent_use_update_ns={recent_use_update_ns:.4}");
    black_box((epoch_hits, timing_hits, gate_true, rate_bench));

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

    let policy_cache = ExecutionPolicyCache::default();
    let cache_capacity = BrokerCapacity::new(8, 1);
    let cache_request = BrokerRequest {
        gpu_range_eligible: true,
    };
    let seeded = policy_cache
        .resolve_with(
            "synthetic-crossover",
            100_000,
            empty,
            cache_capacity,
            cache_request,
            || Some(BackendKind::Gpu),
        )
        .expect("seed policy cache");
    assert_eq!(seeded.status, PolicyCacheStatus::Planned);

    const CACHE_HIT_ITERS: usize = 1_000_000;
    let start = Instant::now();
    let mut cache_hits = 0usize;
    for _ in 0..CACHE_HIT_ITERS {
        let policy = black_box(
            policy_cache
                .resolve_with(
                    black_box("synthetic-crossover"),
                    black_box(100_000),
                    empty,
                    cache_capacity,
                    cache_request,
                    || Some(BackendKind::Serial),
                )
                .expect("cached policy"),
        );
        if policy.status == PolicyCacheStatus::Hit {
            cache_hits += 1;
        }
    }
    let cache_hit_elapsed = start.elapsed();
    let policy_cache_hit_ns =
        cache_hit_elapsed.as_nanos() as f64 / CACHE_HIT_ITERS as f64;

    let shared_cache = Arc::new(policy_cache);
    println!();
    println!("## policy-cache contention");
    for threads in [1usize, 2, 4, 8] {
        const ITERS_PER_THREAD: usize = 200_000;
        let start = Instant::now();
        let mut workers = Vec::with_capacity(threads);
        for _ in 0..threads {
            let cache = Arc::clone(&shared_cache);
            workers.push(thread::spawn(move || {
                let mut hits = 0usize;
                for _ in 0..ITERS_PER_THREAD {
                    let policy = cache
                        .resolve_with(
                            "synthetic-crossover",
                            100_000,
                            empty,
                            cache_capacity,
                            cache_request,
                            || Some(BackendKind::Serial),
                        )
                        .expect("cached policy");
                    if policy.status == PolicyCacheStatus::Hit {
                        hits += 1;
                    }
                }
                hits
            }));
        }
        let mut hits = 0usize;
        for worker in workers {
            hits += worker.join().expect("policy-cache worker");
        }
        let elapsed = start.elapsed();
        let total = threads * ITERS_PER_THREAD;
        let ns_per_hit = elapsed.as_nanos() as f64 / total as f64;
        println!(
            "m11_5_policy_cache_shared_threads_{threads}_ns={ns_per_hit:.2}"
        );
        assert_eq!(hits, total);
    }

    println!();
    println!("## control-plane overhead");
    println!("m12_decision_ns={decision_ns:.2}");
    println!("m13_plan_ns={plan_ns:.2}");
    println!("m14_keep_check_ns={keep_ns:.2}");
    println!("m14_trigger_check_ns={trigger_ns:.2}");
    println!("m11_5_policy_cache_hit_ns={policy_cache_hit_ns:.2}");
    println!("decision_last={last:?}");
    println!("plan_last_chunk={last_chunk}");
    println!("keep_count={keep_count}");
    println!("trigger_count={trigger_count}");
    println!("policy_cache_hits={cache_hits}");

    println!();
    println!("## synchronous vs asynchronous control refresh");
    const CONTROL_ITERS: usize = 2_000_000;
    for period in [1usize, 16, 64, 256, 1024] {
        let start = Instant::now();
        let mut sync_policy = BackendKind::Serial;
        let mut sync_updates = 0usize;
        for i in 0..CONTROL_ITERS {
            if i % period == 0 {
                let n = 1_000 + (i % 100_000);
                sync_policy = control_backend(&model, &machine, empty, n);
                sync_updates += 1;
            }
            black_box(sync_policy);
        }
        let sync_ns = start.elapsed().as_nanos() as f64 / CONTROL_ITERS as f64;

        let request_seq = Arc::new(AtomicUsize::new(0));
        let request_work = Arc::new(AtomicUsize::new(1_000));
        let published = Arc::new(AtomicU8::new(backend_to_u8(BackendKind::Cpu)));
        let completed = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let bg_request_seq = Arc::clone(&request_seq);
        let bg_request_work = Arc::clone(&request_work);
        let bg_published = Arc::clone(&published);
        let bg_completed = Arc::clone(&completed);
        let bg_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let bg_machine = benchmark_machine();
            let bg_model = OnlineCostModel::default();
            train(&bg_model, &bg_machine);
            let bg_empty = telemetry(
                backend(0, 0, 0, 0, 0),
                backend(0, 0, 0, 0, 0),
            );
            let mut seen = 0usize;
            while !bg_stop.load(Ordering::Acquire) {
                let seq = bg_request_seq.load(Ordering::Acquire);
                if seq == seen {
                    std::hint::spin_loop();
                    continue;
                }
                let n = bg_request_work.load(Ordering::Relaxed);
                let next = control_backend(&bg_model, &bg_machine, bg_empty, n);
                bg_published.store(backend_to_u8(next), Ordering::Release);
                seen = seq;
                bg_completed.store(seen, Ordering::Release);
            }
        });

        let start = Instant::now();
        let mut async_requests = 0usize;
        let mut async_reads = 0u64;
        for i in 0..CONTROL_ITERS {
            if i % period == 0 {
                request_work.store(1_000 + (i % 100_000), Ordering::Relaxed);
                async_requests += 1;
                request_seq.store(async_requests, Ordering::Release);
            }
            async_reads += published.load(Ordering::Acquire) as u64;
        }
        let async_ns = start.elapsed().as_nanos() as f64 / CONTROL_ITERS as f64;
        let completed_at_loop_end = completed.load(Ordering::Acquire);
        stop.store(true, Ordering::Release);
        worker.join().expect("async control worker");

        println!(
            "control_refresh period={period} sync_ns={sync_ns:.2} "
        );
        println!(
            "control_refresh_async period={period} foreground_ns={async_ns:.2} "
        );
        println!(
            "control_refresh_counts period={period} sync_updates={sync_updates} "
        );
        println!(
            "control_refresh_async_counts period={period} requests={async_requests} "
        );
        println!(
            "control_refresh_async_completed period={period} completed={completed_at_loop_end} "
        );
        black_box(async_reads);
    }

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
    assert_eq!(cache_hits, CACHE_HIT_ITERS);
}
