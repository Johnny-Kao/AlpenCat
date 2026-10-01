//! Adaptive execution-plan synthesis.
//!
//! M13 turns M12 backend economics plus live machine state into a concrete,
//! backend-neutral execution plan. Ordinary callers do not provide tuning
//! thresholds.

use runtime_core::BackendKind;
use runtime_cost_model::{BackendCostEstimate, CostModelDecision};
use runtime_machine::MachineProfile;
use runtime_telemetry::RuntimeTelemetrySnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidencyHint {
    None,
    PreferDevice,
    KeepResident,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendMix {
    pub serial_weight: u16,
    pub cpu_weight: u16,
    pub gpu_weight: u16,
}

impl BackendMix {
    pub const TOTAL_WEIGHT: u16 = 1_000;

    pub const fn single(backend: BackendKind) -> Self {
        match backend {
            BackendKind::Serial => Self {
                serial_weight: Self::TOTAL_WEIGHT,
                cpu_weight: 0,
                gpu_weight: 0,
            },
            BackendKind::Cpu => Self {
                serial_weight: 0,
                cpu_weight: Self::TOTAL_WEIGHT,
                gpu_weight: 0,
            },
            BackendKind::Gpu => Self {
                serial_weight: 0,
                cpu_weight: 0,
                gpu_weight: Self::TOTAL_WEIGHT,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub primary_backend: BackendKind,
    /// Advisory backend proportions for an executor that can overlap multiple
    /// backend lanes. The v0.1 synchronous executor currently uses
    /// `primary_backend` for one WorkUnit at a time and does not claim that
    /// these weights are concurrently enforced.
    pub backend_mix: BackendMix,
    pub cpu_parallelism: usize,
    pub chunk_size: usize,
    /// Concurrency ceiling. The v0.1 executor uses it to cap planned CPU
    /// parallelism and GPU slot capacity. Because WorkUnits are currently
    /// dispatched synchronously, it does not yet represent multiple
    /// simultaneously in-flight WorkUnits within one submission.
    pub max_in_flight: usize,
    /// Planner-side working-memory budget. The v0.1 range executor uses it to
    /// derive policy but does not reserve or enforce process memory.
    pub memory_budget_bytes: Option<u64>,
    pub gpu_device: Option<String>,
    /// Advisory data-residency preference. Current wgpu range dispatches do not
    /// yet promise persistent device residency across WorkUnits.
    pub residency_hint: ResidencyHint,
    pub confidence_milli: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PlannerContext<'a> {
    pub work_items: usize,
    pub cost: CostModelDecision,
    pub machine: &'a MachineProfile,
    pub telemetry: RuntimeTelemetrySnapshot,
    pub gpu_range_eligible: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AdaptiveExecutionPlanner;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannerPolicy {
    pub min_cost_estimates_for_mix: usize,
    pub mix_min_confidence: f64,
    pub backend_prune_ratio: f64,
    pub target_chunks_per_lane: usize,
    pub memory_available_divisor: u64,
    pub memory_total_divisor: u64,
    pub medium_memory_pressure: f64,
    pub high_memory_pressure: f64,
    pub medium_pressure_divisor: usize,
    pub high_pressure_divisor: usize,
    pub residency_min_confidence: f64,
    pub keep_resident_min_items: usize,
}

impl Default for PlannerPolicy {
    fn default() -> Self {
        Self {
            min_cost_estimates_for_mix: 2,
            mix_min_confidence: 0.25,
            backend_prune_ratio: 2.0,
            target_chunks_per_lane: 4,
            memory_available_divisor: 4,
            memory_total_divisor: 8,
            medium_memory_pressure: 0.70,
            high_memory_pressure: 0.85,
            medium_pressure_divisor: 2,
            high_pressure_divisor: 4,
            residency_min_confidence: 0.75,
            keep_resident_min_items: 1_000_000,
        }
    }
}

impl AdaptiveExecutionPlanner {
    pub fn plan(self, context: PlannerContext<'_>) -> ExecutionPlan {
        self.plan_with_policy(context, PlannerPolicy::default())
    }

    pub fn plan_with_policy(
        self,
        context: PlannerContext<'_>,
        policy: PlannerPolicy,
    ) -> ExecutionPlan {
        let work_items = context.work_items.max(1);
        let cpu_parallelism = cpu_parallelism(context.machine, context.telemetry);
        let memory_budget_bytes = memory_budget(context.machine, policy);
        let backend_mix = backend_mix(context.cost, context.gpu_range_eligible, policy);
        let chunk_size = chunk_size(
            work_items,
            cpu_parallelism,
            backend_mix,
            memory_pressure(context.machine),
            policy,
        );
        let max_in_flight = max_in_flight(
            cpu_parallelism,
            backend_mix,
            context.telemetry,
            memory_pressure(context.machine),
            policy,
        );
        let gpu_device = if backend_mix.gpu_weight > 0 {
            context.machine.gpus.first().map(|gpu| gpu.name.clone())
        } else {
            None
        };
        let residency_hint = residency_hint(
            work_items,
            context.cost,
            backend_mix,
            context.machine,
            memory_pressure(context.machine),
            policy,
        );

        ExecutionPlan {
            primary_backend: context.cost.backend,
            backend_mix,
            cpu_parallelism,
            chunk_size,
            max_in_flight,
            memory_budget_bytes,
            gpu_device,
            residency_hint,
            confidence_milli: (context.cost.confidence.clamp(0.0, 1.0) * 1_000.0).round() as u16,
        }
    }
}

fn cpu_parallelism(machine: &MachineProfile, telemetry: RuntimeTelemetrySnapshot) -> usize {
    let logical = machine.host.logical_cpus.max(1);
    let free = logical.saturating_sub(telemetry.cpu.in_flight).max(1);
    free.min(logical)
}

fn memory_budget(machine: &MachineProfile, policy: PlannerPolicy) -> Option<u64> {
    let total = machine.host.memory_total_bytes?;
    let available = machine.host.memory_available_bytes.unwrap_or(total);

    // Keep a conservative automatic working budget. The planner is not the
    // only process on the machine and must leave headroom for the host.
    let by_available = available / policy.memory_available_divisor.max(1);
    let by_total = total / policy.memory_total_divisor.max(1);
    Some(by_available.min(by_total).max(1))
}

fn memory_pressure(machine: &MachineProfile) -> f64 {
    let Some(total) = machine.host.memory_total_bytes else {
        return 0.0;
    };
    let Some(available) = machine.host.memory_available_bytes else {
        return 0.0;
    };
    if total == 0 {
        return 0.0;
    }
    (1.0 - available as f64 / total as f64).clamp(0.0, 1.0)
}

fn backend_mix(
    cost: CostModelDecision,
    gpu_range_eligible: bool,
    policy: PlannerPolicy,
) -> BackendMix {
    let mut estimates = [
        weighted_estimate(cost.serial),
        weighted_estimate(cost.cpu),
        if gpu_range_eligible {
            weighted_estimate(cost.gpu)
        } else {
            None
        },
    ];

    let measured = estimates.iter().flatten().count();
    if measured < policy.min_cost_estimates_for_mix
        || cost.confidence < policy.mix_min_confidence
    {
        return BackendMix::single(cost.backend);
    }

    let best = estimates
        .iter()
        .flatten()
        .map(|(_, nanos)| *nanos)
        .fold(f64::INFINITY, f64::min);

    // Backends predicted at more than 2x the best cost are not fed work.
    for estimate in &mut estimates {
        if let Some((_, nanos)) = estimate {
            if *nanos > best * policy.backend_prune_ratio.max(1.0) {
                *estimate = None;
            }
        }
    }

    let raw: Vec<(BackendKind, f64)> = estimates
        .into_iter()
        .flatten()
        .map(|(backend, nanos)| (backend, best / nanos.max(1.0)))
        .collect();
    let sum = raw.iter().map(|(_, weight)| *weight).sum::<f64>();

    if sum <= f64::EPSILON {
        return BackendMix::single(cost.backend);
    }

    let mut serial = 0u16;
    let mut cpu = 0u16;
    let mut gpu = 0u16;
    let mut assigned = 0u16;

    for (index, (backend, weight)) in raw.iter().enumerate() {
        let share = if index + 1 == raw.len() {
            BackendMix::TOTAL_WEIGHT.saturating_sub(assigned)
        } else {
            ((weight / sum) * f64::from(BackendMix::TOTAL_WEIGHT))
                .round()
                .clamp(0.0, f64::from(BackendMix::TOTAL_WEIGHT)) as u16
        };
        assigned = assigned.saturating_add(share);
        match backend {
            BackendKind::Serial => serial = share,
            BackendKind::Cpu => cpu = share,
            BackendKind::Gpu => gpu = share,
        }
    }

    BackendMix {
        serial_weight: serial,
        cpu_weight: cpu,
        gpu_weight: gpu,
    }
}

fn weighted_estimate(estimate: Option<BackendCostEstimate>) -> Option<(BackendKind, f64)> {
    let estimate = estimate?;
    Some((estimate.backend, estimate.predicted_nanos?))
}

fn chunk_size(
    work_items: usize,
    cpu_parallelism: usize,
    mix: BackendMix,
    memory_pressure: f64,
    policy: PlannerPolicy,
) -> usize {
    let active_lanes = cpu_parallelism
        .saturating_mul(usize::from(mix.cpu_weight > 0))
        .saturating_add(usize::from(mix.gpu_weight > 0))
        .saturating_add(usize::from(mix.serial_weight > 0))
        .max(1);

    // Target several chunks per active lane so M14 can later rebalance without
    // creating tiny scheduler-dominated units.
    let target_chunks = active_lanes
        .saturating_mul(policy.target_chunks_per_lane.max(1))
        .max(1);
    let mut chunk = work_items.div_ceil(target_chunks).max(1);

    if memory_pressure >= policy.high_memory_pressure {
        chunk = chunk
            .div_ceil(policy.high_pressure_divisor.max(1))
            .max(1);
    } else if memory_pressure >= policy.medium_memory_pressure {
        chunk = chunk
            .div_ceil(policy.medium_pressure_divisor.max(1))
            .max(1);
    }

    chunk.min(work_items)
}

fn max_in_flight(
    cpu_parallelism: usize,
    mix: BackendMix,
    telemetry: RuntimeTelemetrySnapshot,
    memory_pressure: f64,
    policy: PlannerPolicy,
) -> usize {
    let mut capacity = if mix.cpu_weight > 0 {
        cpu_parallelism
    } else {
        0
    };
    if mix.gpu_weight > 0 {
        capacity = capacity.saturating_add(1);
    }
    if mix.serial_weight > 0 {
        capacity = capacity.saturating_add(1);
    }
    capacity = capacity.max(1);

    let already_in_flight = telemetry
        .cpu
        .in_flight
        .saturating_add(telemetry.gpu.in_flight)
        .saturating_add(telemetry.serial.in_flight);
    let free = capacity.saturating_sub(already_in_flight).max(1);

    if memory_pressure >= policy.high_memory_pressure {
        1
    } else if memory_pressure >= policy.medium_memory_pressure {
        free.div_ceil(policy.medium_pressure_divisor.max(1)).max(1)
    } else {
        free
    }
}

fn residency_hint(
    work_items: usize,
    cost: CostModelDecision,
    mix: BackendMix,
    machine: &MachineProfile,
    memory_pressure: f64,
    policy: PlannerPolicy,
) -> ResidencyHint {
    if mix.gpu_weight == 0
        || machine.gpus.is_empty()
        || memory_pressure >= policy.high_memory_pressure
    {
        return ResidencyHint::None;
    }

    if cost.backend == BackendKind::Gpu
        && cost.confidence >= policy.residency_min_confidence
        && work_items >= policy.keep_resident_min_items
    {
        ResidencyHint::KeepResident
    } else {
        ResidencyHint::PreferDevice
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_cost_model::{BackendCostEstimate, CostEstimateSource};
    use runtime_machine::{GpuDeviceProfile, HostProfile};
    use runtime_telemetry::BackendTelemetrySnapshot;

    fn backend(in_flight: usize) -> BackendTelemetrySnapshot {
        BackendTelemetrySnapshot {
            in_flight,
            completed: 0,
            failed: 0,
            work_items: 0,
            elapsed_nanos: 0,
            last_work_items: 0,
            last_elapsed_nanos: 0,
        }
    }

    fn telemetry(cpu: usize, gpu: usize) -> RuntimeTelemetrySnapshot {
        RuntimeTelemetrySnapshot {
            serial: backend(0),
            cpu: backend(cpu),
            gpu: backend(gpu),
        }
    }

    fn machine(gpu: bool, available_gib: u64) -> MachineProfile {
        MachineProfile {
            host: HostProfile {
                os: "linux",
                architecture: "x86_64",
                logical_cpus: 8,
                memory_total_bytes: Some(16 << 30),
                memory_available_bytes: Some(available_gib << 30),
            },
            gpus: if gpu {
                vec![GpuDeviceProfile {
                    name: "GPU".into(),
                    backend: "Vulkan".into(),
                    device_type: "DiscreteGpu".into(),
                    vendor_id: Some(0x10de),
                    device_id: Some(1),
                    dedicated_memory_bytes: None,
                }]
            } else {
                Vec::new()
            },
        }
    }

    fn estimate(backend: BackendKind, nanos: f64) -> BackendCostEstimate {
        BackendCostEstimate {
            backend,
            predicted_nanos: Some(nanos),
            confidence: 1.0,
            source: CostEstimateSource::TaskHistory,
        }
    }

    #[test]
    fn cold_plan_is_single_backend_and_needs_no_tuning() {
        let machine = machine(false, 12);
        let plan = AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items: 10_000,
            cost: CostModelDecision {
                backend: BackendKind::Cpu,
                confidence: 0.0,
                serial: None,
                cpu: None,
                gpu: None,
            },
            machine: &machine,
            telemetry: telemetry(0, 0),
            gpu_range_eligible: false,
        });
        assert_eq!(plan.backend_mix, BackendMix::single(BackendKind::Cpu));
        assert_eq!(plan.cpu_parallelism, 8);
        assert!(plan.chunk_size > 0);
        assert!(plan.max_in_flight > 0);
    }

    #[test]
    fn learned_cpu_gpu_costs_produce_mix() {
        let machine = machine(true, 12);
        let plan = AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items: 1_000_000,
            cost: CostModelDecision {
                backend: BackendKind::Gpu,
                confidence: 1.0,
                serial: Some(estimate(BackendKind::Serial, 10_000.0)),
                cpu: Some(estimate(BackendKind::Cpu, 2_000.0)),
                gpu: Some(estimate(BackendKind::Gpu, 1_000.0)),
            },
            machine: &machine,
            telemetry: telemetry(0, 0),
            gpu_range_eligible: true,
        });
        assert_eq!(
            plan.backend_mix.cpu_weight + plan.backend_mix.gpu_weight,
            BackendMix::TOTAL_WEIGHT
        );
        assert_eq!(plan.backend_mix.serial_weight, 0);
        assert!(plan.backend_mix.gpu_weight > plan.backend_mix.cpu_weight);
    }

    #[test]
    fn high_memory_pressure_reduces_concurrency() {
        let relaxed_machine = machine(false, 12);
        let pressured_machine = machine(false, 1);
        let cost = CostModelDecision {
            backend: BackendKind::Cpu,
            confidence: 0.0,
            serial: None,
            cpu: None,
            gpu: None,
        };
        let relaxed = AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items: 1_000_000,
            cost,
            machine: &relaxed_machine,
            telemetry: telemetry(0, 0),
            gpu_range_eligible: false,
        });
        let pressured = AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items: 1_000_000,
            cost,
            machine: &pressured_machine,
            telemetry: telemetry(0, 0),
            gpu_range_eligible: false,
        });
        assert!(pressured.chunk_size < relaxed.chunk_size);
        assert!(pressured.max_in_flight < relaxed.max_in_flight);
        assert!(pressured.memory_budget_bytes < relaxed.memory_budget_bytes);
    }

    #[test]
    fn confident_large_gpu_plan_prefers_residency() {
        let machine = machine(true, 12);
        let plan = AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items: 2_000_000,
            cost: CostModelDecision {
                backend: BackendKind::Gpu,
                confidence: 1.0,
                serial: Some(estimate(BackendKind::Serial, 8_000.0)),
                cpu: Some(estimate(BackendKind::Cpu, 3_000.0)),
                gpu: Some(estimate(BackendKind::Gpu, 1_000.0)),
            },
            machine: &machine,
            telemetry: telemetry(0, 0),
            gpu_range_eligible: true,
        });
        assert_eq!(plan.residency_hint, ResidencyHint::KeepResident);
        assert_eq!(plan.gpu_device.as_deref(), Some("GPU"));
    }
}
