//! Continuous, throttled rebalancing trigger logic.
//!
//! M14 deliberately keeps the hot path cheap. It compares compact telemetry
//! snapshots and only asks the higher-level runtime to re-run M12+M13 when a
//! material event is observed.

use runtime_core::BackendKind;
use runtime_execution_planner::ExecutionPlan;
use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebalanceReason {
    BackendFailure,
    MaterialPressureChange,
    SustainedThroughputDegradation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebalanceAction {
    Keep,
    Replan(RebalanceReason),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BackendWindow {
    completed: u64,
    failed: u64,
    work_items: u64,
    elapsed_nanos: u64,
    in_flight: usize,
}

impl From<BackendTelemetrySnapshot> for BackendWindow {
    fn from(value: BackendTelemetrySnapshot) -> Self {
        Self {
            completed: value.completed,
            failed: value.failed,
            work_items: value.work_items,
            elapsed_nanos: value.elapsed_nanos,
            in_flight: value.in_flight,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TelemetryWindow {
    serial: BackendWindow,
    cpu: BackendWindow,
    gpu: BackendWindow,
}

impl From<RuntimeTelemetrySnapshot> for TelemetryWindow {
    fn from(value: RuntimeTelemetrySnapshot) -> Self {
        Self {
            serial: value.serial.into(),
            cpu: value.cpu.into(),
            gpu: value.gpu.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebalancePolicy {
    /// Minimum newly completed backend executions before non-failure signals
    /// are allowed to trigger another replan.
    pub min_completed_delta: u64,
    /// Pressure is quantized into this many bands.
    pub pressure_bands: u8,
    /// Throughput must degrade by this percentage across complete windows.
    pub degradation_percent: u16,
}

impl Default for RebalancePolicy {
    fn default() -> Self {
        Self {
            min_completed_delta: 8,
            pressure_bands: 4,
            degradation_percent: 25,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RebalanceSession {
    plan: ExecutionPlan,
    baseline: TelemetryWindow,
    last_evaluated: TelemetryWindow,
    policy: RebalancePolicy,
}

impl RebalanceSession {
    pub fn new(plan: ExecutionPlan, telemetry: RuntimeTelemetrySnapshot) -> Self {
        Self::with_policy(plan, telemetry, RebalancePolicy::default())
    }

    pub fn with_policy(
        plan: ExecutionPlan,
        telemetry: RuntimeTelemetrySnapshot,
        policy: RebalancePolicy,
    ) -> Self {
        Self {
            plan,
            baseline: telemetry.into(),
            last_evaluated: telemetry.into(),
            policy,
        }
    }

    pub fn plan(&self) -> &ExecutionPlan {
        &self.plan
    }

    pub fn replace_plan(&mut self, plan: ExecutionPlan, telemetry: RuntimeTelemetrySnapshot) {
        self.plan = plan;
        self.baseline = telemetry.into();
        self.last_evaluated = telemetry.into();
    }

    pub fn consider(
        &mut self,
        telemetry: RuntimeTelemetrySnapshot,
        cpu_slots: usize,
        gpu_slots: usize,
    ) -> RebalanceAction {
        let current = TelemetryWindow::from(telemetry);

        if failure_changed(self.last_evaluated, current) {
            self.last_evaluated = current;
            return RebalanceAction::Replan(RebalanceReason::BackendFailure);
        }

        let completed_delta = completed_delta(self.last_evaluated, current);
        if completed_delta < self.policy.min_completed_delta {
            return RebalanceAction::Keep;
        }

        if material_pressure_change(
            self.last_evaluated,
            current,
            cpu_slots,
            gpu_slots,
            self.policy.pressure_bands,
        ) {
            self.last_evaluated = current;
            return RebalanceAction::Replan(RebalanceReason::MaterialPressureChange);
        }

        if sustained_degradation(
            &self.plan,
            self.baseline,
            current,
            self.policy.degradation_percent,
        ) {
            self.last_evaluated = current;
            return RebalanceAction::Replan(RebalanceReason::SustainedThroughputDegradation);
        }

        // Advance the evaluation window so long tasks do not repeatedly compare
        // against stale pressure while still keeping the broader baseline for
        // throughput degradation.
        self.last_evaluated = current;
        RebalanceAction::Keep
    }
}

fn completed_delta(previous: TelemetryWindow, current: TelemetryWindow) -> u64 {
    current
        .serial
        .completed
        .saturating_sub(previous.serial.completed)
        .saturating_add(current.cpu.completed.saturating_sub(previous.cpu.completed))
        .saturating_add(current.gpu.completed.saturating_sub(previous.gpu.completed))
}

fn failure_changed(previous: TelemetryWindow, current: TelemetryWindow) -> bool {
    current.serial.failed > previous.serial.failed
        || current.cpu.failed > previous.cpu.failed
        || current.gpu.failed > previous.gpu.failed
}

fn pressure_band(in_flight: usize, slots: usize, bands: u8) -> u8 {
    if slots == 0 {
        return bands;
    }
    if in_flight >= slots {
        return bands;
    }
    ((in_flight.saturating_mul(usize::from(bands))) / slots)
        .min(usize::from(bands.saturating_sub(1))) as u8
}

fn material_pressure_change(
    previous: TelemetryWindow,
    current: TelemetryWindow,
    cpu_slots: usize,
    gpu_slots: usize,
    bands: u8,
) -> bool {
    let bands = bands.max(2);
    pressure_band(previous.cpu.in_flight, cpu_slots, bands)
        != pressure_band(current.cpu.in_flight, cpu_slots, bands)
        || pressure_band(previous.gpu.in_flight, gpu_slots, bands)
            != pressure_band(current.gpu.in_flight, gpu_slots, bands)
}

fn sustained_degradation(
    plan: &ExecutionPlan,
    baseline: TelemetryWindow,
    current: TelemetryWindow,
    degradation_percent: u16,
) -> bool {
    let backend = plan.primary_backend;
    let before = backend_window(baseline, backend);
    let now = backend_window(current, backend);

    let delta_items = now.work_items.saturating_sub(before.work_items);
    let delta_nanos = now.elapsed_nanos.saturating_sub(before.elapsed_nanos);
    let delta_completed = now.completed.saturating_sub(before.completed);

    // Require multiple completed samples so one slow chunk cannot dominate.
    if delta_completed < 4 || delta_items == 0 || delta_nanos == 0 {
        return false;
    }

    let recent_nanos_per_item = delta_nanos as f64 / delta_items as f64;
    let historical = historical_nanos_per_item(before);
    let Some(historical) = historical else {
        return false;
    };

    let threshold = historical * (1.0 + f64::from(degradation_percent) / 100.0);
    recent_nanos_per_item > threshold
}

fn historical_nanos_per_item(window: BackendWindow) -> Option<f64> {
    if window.work_items == 0 || window.elapsed_nanos == 0 {
        return None;
    }
    Some(window.elapsed_nanos as f64 / window.work_items as f64)
}

fn backend_window(window: TelemetryWindow, backend: BackendKind) -> BackendWindow {
    match backend {
        BackendKind::Serial => window.serial,
        BackendKind::Cpu => window.cpu,
        BackendKind::Gpu => window.gpu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_execution_planner::{BackendMix, ResidencyHint};

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

    fn snapshot(
        cpu: BackendTelemetrySnapshot,
        gpu: BackendTelemetrySnapshot,
    ) -> RuntimeTelemetrySnapshot {
        RuntimeTelemetrySnapshot {
            serial: backend(0, 0, 0, 0, 0),
            cpu,
            gpu,
        }
    }

    fn plan(primary: BackendKind) -> ExecutionPlan {
        ExecutionPlan {
            primary_backend: primary,
            backend_mix: BackendMix::single(primary),
            cpu_parallelism: 4,
            chunk_size: 1024,
            max_in_flight: 4,
            memory_budget_bytes: Some(1 << 30),
            gpu_device: None,
            residency_hint: ResidencyHint::None,
            confidence_milli: 800,
        }
    }

    #[test]
    fn tiny_progress_does_not_replan() {
        let start = snapshot(backend(0, 10, 0, 10_000, 100_000), backend(0, 0, 0, 0, 0));
        let mut session = RebalanceSession::new(plan(BackendKind::Cpu), start);
        let current = snapshot(backend(1, 12, 0, 12_000, 120_000), backend(0, 0, 0, 0, 0));
        assert_eq!(session.consider(current, 8, 1), RebalanceAction::Keep);
    }

    #[test]
    fn backend_failure_replans_immediately() {
        let start = snapshot(
            backend(0, 10, 0, 10_000, 100_000),
            backend(0, 4, 0, 4_000, 40_000),
        );
        let mut session = RebalanceSession::new(plan(BackendKind::Gpu), start);
        let current = snapshot(
            backend(0, 10, 0, 10_000, 100_000),
            backend(0, 4, 1, 4_100, 41_000),
        );
        assert_eq!(
            session.consider(current, 8, 1),
            RebalanceAction::Replan(RebalanceReason::BackendFailure)
        );
    }

    #[test]
    fn material_pressure_change_waits_for_window() {
        let start = snapshot(backend(0, 10, 0, 10_000, 100_000), backend(0, 0, 0, 0, 0));
        let mut session = RebalanceSession::new(plan(BackendKind::Cpu), start);
        let current = snapshot(backend(6, 18, 0, 18_000, 180_000), backend(0, 0, 0, 0, 0));
        assert_eq!(
            session.consider(current, 8, 1),
            RebalanceAction::Replan(RebalanceReason::MaterialPressureChange)
        );
    }

    #[test]
    fn pressure_recovery_can_reopen_planning() {
        let start = snapshot(backend(6, 10, 0, 10_000, 100_000), backend(0, 0, 0, 0, 0));
        let mut session = RebalanceSession::new(plan(BackendKind::Cpu), start);
        let current = snapshot(backend(0, 18, 0, 18_000, 180_000), backend(0, 0, 0, 0, 0));
        assert_eq!(
            session.consider(current, 8, 1),
            RebalanceAction::Replan(RebalanceReason::MaterialPressureChange)
        );
    }

    #[test]
    fn sustained_degradation_uses_accumulated_window() {
        let start = snapshot(
            backend(0, 100, 0, 100_000, 1_000_000),
            backend(0, 0, 0, 0, 0),
        );
        let mut session = RebalanceSession::new(plan(BackendKind::Cpu), start);
        let current = snapshot(
            backend(0, 108, 0, 108_000, 1_160_000),
            backend(0, 0, 0, 0, 0),
        );
        assert_eq!(
            session.consider(current, 8, 1),
            RebalanceAction::Replan(RebalanceReason::SustainedThroughputDegradation)
        );
    }

    #[test]
    fn stable_window_keeps_plan() {
        let start = snapshot(
            backend(0, 100, 0, 100_000, 1_000_000),
            backend(0, 0, 0, 0, 0),
        );
        let mut session = RebalanceSession::new(plan(BackendKind::Cpu), start);
        let current = snapshot(
            backend(0, 108, 0, 108_000, 1_080_000),
            backend(0, 0, 0, 0, 0),
        );
        assert_eq!(session.consider(current, 8, 1), RebalanceAction::Keep);
    }
}
