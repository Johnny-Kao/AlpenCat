//! Online, task-aware execution cost model.
//!
//! M12 v2 learns a small parametric model from real executions:
//!
//! ```text
//! total = setup + per_item * work + transfer
//! ```
//!
//! No hardware marketing table or user threshold is required. When the model
//! has insufficient evidence it falls back to existing safe bootstrap routing.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use runtime_core::BackendKind;
use runtime_machine::MachineProfile;
use runtime_selector::{select, CalibrationProfile};
use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};

const LOCAL_BUCKETS: usize = usize::BITS as usize;
const UNCERTAINTY_MARGIN: f64 = 0.10;
const WARMUP_SAMPLES: u64 = 4;
const WARMUP_ALPHA: f64 = 0.5;
const STEADY_ALPHA: f64 = 0.2;
const LOCAL_BLEND_FULL_SAMPLES: f64 = 4.0;
const LOCAL_BLEND_MAX: f64 = 0.75;
const MIN_LINEAR_FIT_SAMPLES: u64 = 4;
const FULL_CONFIDENCE_SAMPLES: f64 = 8.0;
const NO_FIT_SIZE_DIVERSITY_CONFIDENCE: f64 = 0.6;
const FAILURE_RATE_PENALTY: f64 = 4.0;
const DEFAULT_MODEL_ENTRIES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostEstimateSource {
    TaskHistory,
    RuntimeTelemetry,
    Bootstrap,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackendCostEstimate {
    pub backend: BackendKind,
    pub predicted_nanos: Option<f64>,
    pub confidence: f64,
    pub source: CostEstimateSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MachineFingerprint(pub u64);

impl MachineFingerprint {
    pub fn from_machine(machine: &MachineProfile) -> Self {
        let mut hash = 0xcbf29ce484222325u64;
        hash_bytes(&mut hash, machine.host.os.as_bytes());
        hash_bytes(&mut hash, machine.host.architecture.as_bytes());
        hash_u64(&mut hash, machine.host.logical_cpus as u64);
        hash_u64(&mut hash, machine.host.memory_total_bytes.unwrap_or(0));
        for gpu in &machine.gpus {
            hash_bytes(&mut hash, gpu.name.as_bytes());
            hash_bytes(&mut hash, gpu.backend.as_bytes());
            hash_bytes(&mut hash, gpu.device_type.as_bytes());
            hash_u64(&mut hash, u64::from(gpu.vendor_id.unwrap_or(0)));
            hash_u64(&mut hash, u64::from(gpu.device_id.unwrap_or(0)));
            hash_u64(&mut hash, gpu.dedicated_memory_bytes.unwrap_or(0));
        }
        Self(hash)
    }
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn hash_u64(hash: &mut u64, value: u64) {
    hash_bytes(hash, &value.to_le_bytes());
}

#[derive(Debug, Clone, Copy)]
pub struct CostModelContext<'a> {
    pub cpu_eligible: bool,
    pub gpu_eligible: bool,
    pub machine: &'a MachineProfile,
    pub telemetry: RuntimeTelemetrySnapshot,
    pub bootstrap: CalibrationProfile,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CostModelDecision {
    pub backend: BackendKind,
    pub confidence: f64,
    pub serial: Option<BackendCostEstimate>,
    pub cpu: Option<BackendCostEstimate>,
    pub gpu: Option<BackendCostEstimate>,
}

#[derive(Debug, Clone, Copy)]
struct EstimateContext<'a> {
    task_id: &'a str,
    backend: BackendKind,
    work_items: usize,
    machine: &'a MachineProfile,
    fingerprint: MachineFingerprint,
    telemetry: BackendTelemetrySnapshot,
    eligible: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CostObservation {
    pub work_items: usize,
    pub elapsed: Duration,
    pub success: bool,
    /// Optional bytes moved between host and accelerator for this execution.
    pub transfer_bytes: u64,
    /// Optional measured transfer-only duration. Zero means unavailable.
    pub transfer_elapsed: Duration,
}

impl CostObservation {
    pub const fn execution(work_items: usize, elapsed: Duration, success: bool) -> Self {
        Self {
            work_items,
            elapsed,
            success,
            transfer_bytes: 0,
            transfer_elapsed: Duration::ZERO,
        }
    }

    pub const fn with_transfer(mut self, transfer_bytes: u64, transfer_elapsed: Duration) -> Self {
        self.transfer_bytes = transfer_bytes;
        self.transfer_elapsed = transfer_elapsed;
        self
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LocalBucket {
    samples: u64,
    ema_work: f64,
    ema_compute_nanos: f64,
}

impl LocalBucket {
    fn observe(&mut self, work: f64, compute_nanos: f64) {
        self.samples = self.samples.saturating_add(1);
        let alpha = if self.samples <= WARMUP_SAMPLES {
            WARMUP_ALPHA
        } else {
            STEADY_ALPHA
        };
        if self.samples == 1 {
            self.ema_work = work;
            self.ema_compute_nanos = compute_nanos;
        } else {
            self.ema_work = alpha * work + (1.0 - alpha) * self.ema_work;
            self.ema_compute_nanos = alpha * compute_nanos + (1.0 - alpha) * self.ema_compute_nanos;
        }
    }

    fn blend_weight(self) -> f64 {
        (self.samples as f64 / LOCAL_BLEND_FULL_SAMPLES).min(LOCAL_BLEND_MAX)
    }
}

fn work_size_class(work_items: usize) -> usize {
    let value = work_items.max(1);
    let class = (usize::BITS - 1 - value.leading_zeros()) as usize;
    class.min(LOCAL_BUCKETS - 1)
}

#[derive(Debug, Clone, Copy)]
struct LinearStats {
    samples: u64,
    success_samples: u64,
    failure_samples: u64,
    sum_x: f64,
    sum_y: f64,
    sum_xx: f64,
    sum_xy: f64,
    min_x: f64,
    max_x: f64,
    ema_nanos_per_item: f64,
    transfer_bytes: u64,
    transfer_nanos: u64,
    local: [LocalBucket; LOCAL_BUCKETS],
}

impl Default for LinearStats {
    fn default() -> Self {
        Self {
            samples: 0,
            success_samples: 0,
            failure_samples: 0,
            sum_x: 0.0,
            sum_y: 0.0,
            sum_xx: 0.0,
            sum_xy: 0.0,
            min_x: 0.0,
            max_x: 0.0,
            ema_nanos_per_item: 0.0,
            transfer_bytes: 0,
            transfer_nanos: 0,
            local: [LocalBucket::default(); LOCAL_BUCKETS],
        }
    }
}

impl LinearStats {
    fn observe(&mut self, observation: CostObservation) {
        self.samples = self.samples.saturating_add(1);
        if !observation.success {
            self.failure_samples = self.failure_samples.saturating_add(1);
            return;
        }

        self.success_samples = self.success_samples.saturating_add(1);
        let x = observation.work_items.max(1) as f64;
        let total_nanos = observation.elapsed.as_nanos().min(u128::from(u64::MAX)) as f64;
        let transfer_nanos = observation
            .transfer_elapsed
            .as_nanos()
            .min(u128::from(u64::MAX)) as f64;
        let compute_nanos = (total_nanos - transfer_nanos).max(0.0);
        self.local[work_size_class(observation.work_items)].observe(x, compute_nanos);

        self.sum_x += x;
        self.sum_y += compute_nanos;
        self.sum_xx += x * x;
        self.sum_xy += x * compute_nanos;

        if self.success_samples == 1 {
            self.min_x = x;
            self.max_x = x;
            self.ema_nanos_per_item = compute_nanos / x;
        } else {
            self.min_x = self.min_x.min(x);
            self.max_x = self.max_x.max(x);
            let alpha = if self.success_samples <= WARMUP_SAMPLES {
                WARMUP_ALPHA
            } else {
                STEADY_ALPHA
            };
            let per_item = compute_nanos / x;
            self.ema_nanos_per_item = alpha * per_item + (1.0 - alpha) * self.ema_nanos_per_item;
        }

        if observation.transfer_bytes > 0 && transfer_nanos > 0.0 {
            self.transfer_bytes = self
                .transfer_bytes
                .saturating_add(observation.transfer_bytes);
            self.transfer_nanos = self.transfer_nanos.saturating_add(transfer_nanos as u64);
        }
    }

    fn fit(self) -> Option<(f64, f64)> {
        if self.success_samples < MIN_LINEAR_FIT_SAMPLES || self.max_x <= self.min_x {
            return None;
        }

        let n = self.success_samples as f64;
        let denominator = n * self.sum_xx - self.sum_x * self.sum_x;
        if denominator.abs() <= f64::EPSILON {
            return None;
        }

        let slope = ((n * self.sum_xy - self.sum_x * self.sum_y) / denominator).max(0.0);
        let intercept = ((self.sum_y - slope * self.sum_x) / n).max(0.0);
        Some((intercept, slope))
    }

    fn transfer_nanos_for(self, transfer_bytes: u64) -> f64 {
        if transfer_bytes == 0 || self.transfer_bytes == 0 || self.transfer_nanos == 0 {
            return 0.0;
        }
        transfer_bytes as f64 * self.transfer_nanos as f64 / self.transfer_bytes as f64
    }

    fn predicted_nanos(self, work_items: usize, transfer_bytes: u64) -> Option<f64> {
        if self.success_samples == 0 {
            return None;
        }

        let work = work_items.max(1) as f64;
        let (setup, per_item) = self.fit().unwrap_or((0.0, self.ema_nanos_per_item));
        let compute = setup + per_item * work;
        let transfer = self.transfer_nanos_for(transfer_bytes);
        let local = self.local[work_size_class(work_items)];
        // Recompute the bucket residual against the current fit. Paired EMAs
        // preserve its slope within a bucket and do not include transfer twice.
        let residual = if local.samples > 0 {
            local.blend_weight() * (local.ema_compute_nanos - (setup + per_item * local.ema_work))
        } else {
            0.0
        };
        let corrected = (compute + residual).max(0.0) + transfer;
        let failure_rate = self.failure_samples as f64 / self.samples.max(1) as f64;
        Some(corrected * (1.0 + failure_rate * FAILURE_RATE_PENALTY))
    }

    fn confidence(self) -> f64 {
        if self.samples == 0 {
            return 0.0;
        }

        let sample_confidence =
            (self.success_samples as f64 / FULL_CONFIDENCE_SAMPLES).min(1.0);
        let size_diversity = if self.fit().is_some() {
            1.0
        } else {
            NO_FIT_SIZE_DIVERSITY_CONFIDENCE
        };
        let reliability = 1.0 - self.failure_samples as f64 / self.samples.max(1) as f64;
        (sample_confidence * size_diversity * reliability).clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModelKey {
    task_id: String,
    backend: BackendKind,
    machine: MachineFingerprint,
}

impl ModelKey {
    fn new(task_id: &str, backend: BackendKind, machine: MachineFingerprint) -> Self {
        Self {
            task_id: task_id.to_owned(),
            backend,
            machine,
        }
    }
}

#[derive(Debug, Default)]
struct CostModelState {
    entries: HashMap<ModelKey, LinearStats>,
    active_machine: Option<MachineFingerprint>,
}

#[derive(Debug)]
pub struct OnlineCostModel {
    state: Mutex<CostModelState>,
    max_entries: usize,
}

impl Default for OnlineCostModel {
    fn default() -> Self {
        Self::new(DEFAULT_MODEL_ENTRIES)
    }
}

impl OnlineCostModel {
    pub fn new(max_entries: usize) -> Self {
        Self {
            state: Mutex::new(CostModelState::default()),
            max_entries: max_entries.max(3),
        }
    }

    pub fn observe(
        &self,
        task_id: &str,
        backend: BackendKind,
        work_items: usize,
        elapsed: Duration,
        success: bool,
    ) {
        self.observe_detailed(
            task_id,
            backend,
            MachineFingerprint(0),
            CostObservation::execution(work_items, elapsed, success),
        );
    }

    pub fn observe_detailed(
        &self,
        task_id: &str,
        backend: BackendKind,
        machine: MachineFingerprint,
        observation: CostObservation,
    ) {
        let key = ModelKey::new(task_id, backend, machine);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if state.entries.len() >= self.max_entries && !state.entries.contains_key(&key) {
            if let Some(key) = state.entries.keys().next().cloned() {
                state.entries.remove(&key);
            }
        }

        state.entries.entry(key).or_default().observe(observation);
    }

    pub fn decide(
        &self,
        task_id: &str,
        work_items: usize,
        context: CostModelContext<'_>,
    ) -> CostModelDecision {
        self.decide_with_preference(task_id, work_items, context, None)
    }

    pub fn decide_with_preference(
        &self,
        task_id: &str,
        work_items: usize,
        context: CostModelContext<'_>,
        preferred_backend: Option<BackendKind>,
    ) -> CostModelDecision {
        let CostModelContext {
            cpu_eligible,
            gpu_eligible,
            machine,
            telemetry,
            bootstrap,
        } = context;
        let fingerprint = MachineFingerprint::from_machine(machine);
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.active_machine = Some(fingerprint);
        }

        let serial = self.estimate_backend(EstimateContext {
            task_id,
            backend: BackendKind::Serial,
            work_items,
            machine,
            fingerprint,
            telemetry: telemetry.serial,
            eligible: true,
        });
        let cpu = self.estimate_backend(EstimateContext {
            task_id,
            backend: BackendKind::Cpu,
            work_items,
            machine,
            fingerprint,
            telemetry: telemetry.cpu,
            eligible: cpu_eligible && machine.host.logical_cpus > 1,
        });
        let gpu = self.estimate_backend(EstimateContext {
            task_id,
            backend: BackendKind::Gpu,
            work_items,
            machine,
            fingerprint,
            telemetry: telemetry.gpu,
            eligible: gpu_eligible && !machine.gpus.is_empty(),
        });

        let candidates = [serial, cpu, gpu];
        let measured_count = candidates
            .iter()
            .flatten()
            .filter(|estimate| estimate.predicted_nanos.is_some())
            .count();
        let learned = if measured_count >= 2 {
            let mut measured = candidates
                .iter()
                .flatten()
                .filter(|estimate| estimate.predicted_nanos.is_some())
                .copied()
                .collect::<Vec<_>>();
            measured.sort_by(|left, right| {
                left.predicted_nanos
                    .partial_cmp(&right.predicted_nanos)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let best = measured.first().copied();
            best.map(|best| {
                let best_nanos = best.predicted_nanos.unwrap_or(f64::INFINITY);
                let runner_up_nanos = measured
                    .get(1)
                    .and_then(|estimate| estimate.predicted_nanos)
                    .unwrap_or(f64::INFINITY);
                let relative_gap = if best_nanos.is_finite() && best_nanos > 0.0 {
                    ((runner_up_nanos - best_nanos) / best_nanos).max(0.0)
                } else {
                    1.0
                };
                let uncertainty_confidence = (relative_gap / UNCERTAINTY_MARGIN).clamp(0.0, 1.0);

                let selected = preferred_backend
                    .and_then(|preferred| {
                        measured
                            .iter()
                            .find(|estimate| estimate.backend == preferred)
                            .copied()
                    })
                    .filter(|preferred| {
                        preferred
                            .predicted_nanos
                            .is_some_and(|nanos| nanos <= best_nanos * (1.0 + UNCERTAINTY_MARGIN))
                    })
                    .unwrap_or(best);

                (selected, selected.confidence * uncertainty_confidence)
            })
        } else {
            None
        };

        let (backend, confidence) = if let Some((best, confidence)) = learned {
            (best.backend, confidence)
        } else {
            (
                bootstrap_backend(
                    work_items,
                    cpu_eligible && machine.host.logical_cpus > 1,
                    gpu_eligible && !machine.gpus.is_empty(),
                    bootstrap,
                ),
                0.0,
            )
        };

        CostModelDecision {
            backend,
            confidence,
            serial,
            cpu,
            gpu,
        }
    }

    fn estimate_backend(&self, context: EstimateContext<'_>) -> Option<BackendCostEstimate> {
        if !context.eligible {
            return None;
        }

        let history = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state
                .entries
                .get(&ModelKey::new(
                    context.task_id,
                    context.backend,
                    context.fingerprint,
                ))
                .or_else(|| {
                    state.entries.get(&ModelKey::new(
                        context.task_id,
                        context.backend,
                        MachineFingerprint(0),
                    ))
                })
                .copied()
        };

        if let Some(stats) = history {
            if let Some(base) = stats.predicted_nanos(context.work_items, 0) {
                return Some(BackendCostEstimate {
                    backend: context.backend,
                    predicted_nanos: Some(apply_pressure(
                        base,
                        context.backend,
                        context.machine,
                        context.telemetry,
                    )),
                    confidence: stats.confidence(),
                    source: CostEstimateSource::TaskHistory,
                });
            }
        }

        if let Some(nanos_per_item) = context.telemetry.average_nanos_per_item() {
            let base = nanos_per_item * context.work_items.max(1) as f64;
            return Some(BackendCostEstimate {
                backend: context.backend,
                predicted_nanos: Some(apply_pressure(
                    base,
                    context.backend,
                    context.machine,
                    context.telemetry,
                )),
                confidence: telemetry_confidence(context.telemetry),
                source: CostEstimateSource::RuntimeTelemetry,
            });
        }

        Some(BackendCostEstimate {
            backend: context.backend,
            predicted_nanos: None,
            confidence: 0.0,
            source: CostEstimateSource::Bootstrap,
        })
    }

    pub fn clear_task(&self, task_id: &str) -> usize {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = state.entries.len();
        state.entries.retain(|key, _| key.task_id != task_id);
        before - state.entries.len()
    }

    pub fn active_machine_fingerprint(&self) -> Option<MachineFingerprint> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_machine
    }

    pub fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn bootstrap_backend(
    work_items: usize,
    cpu_eligible: bool,
    gpu_eligible: bool,
    bootstrap: CalibrationProfile,
) -> BackendKind {
    match select(work_items, gpu_eligible, bootstrap) {
        BackendKind::Gpu if gpu_eligible => BackendKind::Gpu,
        BackendKind::Cpu if cpu_eligible => BackendKind::Cpu,
        BackendKind::Gpu | BackendKind::Cpu if cpu_eligible => BackendKind::Cpu,
        _ => BackendKind::Serial,
    }
}

fn telemetry_confidence(snapshot: BackendTelemetrySnapshot) -> f64 {
    let successes = snapshot.completed as f64;
    let total = snapshot.completed.saturating_add(snapshot.failed) as f64;
    if total == 0.0 {
        return 0.0;
    }
    ((successes / FULL_CONFIDENCE_SAMPLES).min(1.0) * (successes / total)).clamp(0.0, 1.0)
}

fn apply_pressure(
    base_nanos: f64,
    backend: BackendKind,
    machine: &MachineProfile,
    telemetry: BackendTelemetrySnapshot,
) -> f64 {
    let slots = match backend {
        BackendKind::Serial => 1,
        BackendKind::Cpu => machine.host.logical_cpus.max(1),
        BackendKind::Gpu => 1,
    };
    let pressure = telemetry.in_flight as f64 / slots as f64;
    let failure_rate = telemetry.failed as f64
        / telemetry.completed.saturating_add(telemetry.failed).max(1) as f64;
    base_nanos * (1.0 + pressure + failure_rate * FAILURE_RATE_PENALTY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_machine::{GpuDeviceProfile, HostProfile};

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

    fn machine(with_gpu: bool) -> MachineProfile {
        MachineProfile {
            host: HostProfile {
                os: "linux",
                architecture: "x86_64",
                logical_cpus: 8,
                memory_total_bytes: Some(16 * 1024 * 1024 * 1024),
                memory_available_bytes: Some(12 * 1024 * 1024 * 1024),
            },
            gpus: if with_gpu {
                vec![GpuDeviceProfile {
                    name: "test gpu".into(),
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

    #[test]
    fn bootstrap_needs_no_user_tuning() {
        let model = OnlineCostModel::default();
        let machine = machine(true);
        let decision = model.decide(
            "x",
            100,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
        );
        assert_eq!(decision.confidence, 0.0);
    }

    #[test]
    fn regression_learns_setup_and_per_item_cost() {
        let mut stats = LinearStats::default();
        for n in [100usize, 1_000, 10_000, 100_000] {
            let nanos = 50_000 + 3 * n as u64;
            stats.observe(CostObservation::execution(
                n,
                Duration::from_nanos(nanos),
                true,
            ));
        }
        let (setup, per_item) = stats.fit().expect("fit");
        assert!((setup - 50_000.0).abs() < 1.0);
        assert!((per_item - 3.0).abs() < 0.01);
    }

    #[test]
    fn transfer_bandwidth_contributes_to_prediction() {
        let mut stats = LinearStats::default();
        for n in [100usize, 1_000, 10_000, 100_000] {
            stats.observe(
                CostObservation::execution(n, Duration::from_micros(100), true)
                    .with_transfer(1_000_000, Duration::from_micros(50)),
            );
        }
        let without_transfer = stats.predicted_nanos(1_000, 0).unwrap();
        let with_transfer = stats.predicted_nanos(1_000, 1_000_000).unwrap();
        assert!((without_transfer - 50_000.0).abs() < 1.0);
        assert!((with_transfer - 100_000.0).abs() < 1.0);
    }

    #[test]
    fn task_history_changes_backend_choice() {
        let model = OnlineCostModel::default();
        for n in [512usize, 4096, 16_384, 65_536] {
            model.observe("x", BackendKind::Cpu, n, Duration::from_micros(400), true);
            model.observe("x", BackendKind::Gpu, n, Duration::from_micros(100), true);
        }
        let machine = machine(true);
        let decision = model.decide(
            "x",
            4096,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
        );
        assert_eq!(decision.backend, BackendKind::Gpu);
    }

    #[test]
    fn learned_parametric_model_finds_cpu_gpu_crossover() {
        let model = OnlineCostModel::default();
        for n in [100usize, 1_000, 10_000, 100_000] {
            model.observe(
                "crossover",
                BackendKind::Cpu,
                n,
                Duration::from_nanos(10_000 + 5 * n as u64),
                true,
            );
            model.observe(
                "crossover",
                BackendKind::Gpu,
                n,
                Duration::from_nanos(100_000 + n as u64),
                true,
            );
        }

        let machine = machine(true);
        let small = model.decide(
            "crossover",
            1_000,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
        );
        let large = model.decide(
            "crossover",
            100_000,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
        );

        assert_eq!(small.backend, BackendKind::Cpu);
        assert_eq!(large.backend, BackendKind::Gpu);
    }

    #[test]
    fn local_residual_preserves_linear_predictions_inside_observed_bucket() {
        let mut stats = LinearStats::default();
        for n in [100usize, 1_000, 10_000, 12_000, 100_000] {
            stats.observe(CostObservation::execution(
                n,
                Duration::from_nanos(50_000 + 3 * n as u64),
                true,
            ));
        }
        for n in [8_192usize, 11_000, 16_383] {
            let expected = 50_000.0 + 3.0 * n as f64;
            assert!((stats.predicted_nanos(n, 0).unwrap() - expected).abs() < 1.0);
        }
    }

    #[test]
    fn local_bucket_residual_corrects_nearby_global_fit() {
        let mut stats = LinearStats::default();
        for n in [16_384usize, 32_768, 65_536, 262_144, 1_048_576] {
            let global = 50_000 + 4 * n as u64;
            stats.observe(CostObservation::execution(
                n,
                Duration::from_nanos(global),
                true,
            ));
        }
        for _ in 0..4 {
            stats.observe(CostObservation::execution(
                65_536,
                Duration::from_nanos(250_000),
                true,
            ));
        }

        let predicted = stats.predicted_nanos(65_536, 0).unwrap();
        assert!(predicted < 350_000.0);
        assert!(predicted > 200_000.0);
    }

    #[test]
    fn apple_m5_h128_near_crossover_prefers_stable_cpu() {
        let model = OnlineCostModel::default();
        let samples = [
            (16_384usize, 189_790u64, 455_000u64),
            (32_768, 339_830, 534_830),
            (65_536, 665_120, 750_250),
            (262_144, 2_775_880, 1_350_960),
            (1_048_576, 9_398_080, 4_288_120),
        ];

        for (work_items, cpu_nanos, gpu_nanos) in samples {
            model.observe(
                "m5-h128",
                BackendKind::Cpu,
                work_items,
                Duration::from_nanos(cpu_nanos),
                true,
            );
            model.observe(
                "m5-h128",
                BackendKind::Gpu,
                work_items,
                Duration::from_nanos(gpu_nanos),
                true,
            );
        }

        let machine = machine(true);
        let decision = model.decide_with_preference(
            "m5-h128",
            65_536,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
            Some(BackendKind::Cpu),
        );

        assert_eq!(decision.backend, BackendKind::Cpu);
        assert!(decision.confidence < 0.5);
    }

    #[test]
    fn preference_holds_backend_inside_uncertainty_band() {
        let model = OnlineCostModel::default();
        for n in [100usize, 1_000, 10_000, 100_000] {
            model.observe(
                "hysteresis",
                BackendKind::Cpu,
                n,
                Duration::from_nanos(10_000 + 10 * n as u64),
                true,
            );
            model.observe(
                "hysteresis",
                BackendKind::Gpu,
                n,
                Duration::from_nanos(15_000 + 9 * n as u64),
                true,
            );
        }
        let machine = machine(true);
        let decision = model.decide_with_preference(
            "hysteresis",
            10_000,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
            Some(BackendKind::Cpu),
        );
        assert_eq!(decision.backend, BackendKind::Cpu);
        assert!(decision.confidence < 0.5);
    }

    #[test]
    fn preference_does_not_block_materially_faster_backend() {
        let model = OnlineCostModel::default();
        for n in [100usize, 1_000, 10_000, 100_000] {
            model.observe(
                "switch",
                BackendKind::Cpu,
                n,
                Duration::from_nanos(50_000 + 10 * n as u64),
                true,
            );
            model.observe(
                "switch",
                BackendKind::Gpu,
                n,
                Duration::from_nanos(10_000 + 2 * n as u64),
                true,
            );
        }
        let machine = machine(true);
        let decision = model.decide_with_preference(
            "switch",
            100_000,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
            Some(BackendKind::Cpu),
        );
        assert_eq!(decision.backend, BackendKind::Gpu);
    }

    #[test]
    fn failure_penalty_can_move_work_off_backend() {
        let model = OnlineCostModel::default();
        for _ in 0..8 {
            model.observe(
                "x",
                BackendKind::Cpu,
                4096,
                Duration::from_micros(110),
                true,
            );
            model.observe(
                "x",
                BackendKind::Gpu,
                4096,
                Duration::from_micros(100),
                false,
            );
        }
        let machine = machine(true);
        let decision = model.decide(
            "x",
            4096,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
        );
        assert_eq!(decision.backend, BackendKind::Cpu);
    }

    #[test]
    fn machine_fingerprint_changes_with_device_identity() {
        let left = machine(true);
        let mut right = machine(true);
        right.gpus[0].device_id = Some(2);
        assert_ne!(
            MachineFingerprint::from_machine(&left),
            MachineFingerprint::from_machine(&right)
        );
    }

    #[test]
    fn gpu_is_not_considered_when_machine_has_none() {
        let model = OnlineCostModel::default();
        let machine = machine(false);
        let decision = model.decide(
            "x",
            1_000_000,
            CostModelContext {
                cpu_eligible: true,
                gpu_eligible: true,
                machine: &machine,
                telemetry: telemetry(),
                bootstrap: CalibrationProfile::default(),
            },
        );
        assert!(decision.gpu.is_none());
        assert_ne!(decision.backend, BackendKind::Gpu);
    }
}
