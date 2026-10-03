//! Backend selection policy boundary.
//!
//! Calibration remains explicit and environment/task specific. The selector
//! returns a backend directly; there is no wrapper state at this layer.

use runtime_core::BackendKind;

pub const SERIAL_MAX_ITEMS: usize = 1_024;
pub const CPU_MAX_ITEMS: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalibrationProfile {
    pub serial_max_items: usize,
    pub cpu_max_items: usize,
}

impl Default for CalibrationProfile {
    fn default() -> Self {
        Self {
            serial_max_items: SERIAL_MAX_ITEMS,
            cpu_max_items: CPU_MAX_ITEMS,
        }
    }
}

pub const fn select(
    work_items: usize,
    gpu_eligible: bool,
    profile: CalibrationProfile,
) -> BackendKind {
    if work_items <= profile.serial_max_items {
        BackendKind::Serial
    } else if work_items <= profile.cpu_max_items || !gpu_eligible {
        BackendKind::Cpu
    } else {
        BackendKind::Gpu
    }
}

/// Minimal state for localized adaptivity around a CPU/GPU crossover.
///
/// The direct path is intentionally tiny:
///
/// - `work_items <= cpu_safe_max` => CPU;
/// - `work_items >= gpu_safe_min` => GPU;
/// - otherwise => ask the slower control plane.
///
/// A controller may widen the uncertainty band after an audit/probe discovers
/// that a previously safe direct route has become wrong. Recalibration can
/// later publish a tighter boundary again.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalizedBoundary {
    pub cpu_safe_max: usize,
    pub gpu_safe_min: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalizedDecision {
    Direct(BackendKind),
    Recalibrate,
}

impl LocalizedBoundary {
    pub const fn new(cpu_safe_max: usize, gpu_safe_min: usize) -> Self {
        Self {
            cpu_safe_max,
            gpu_safe_min,
        }
    }

    pub const fn is_valid(self) -> bool {
        self.cpu_safe_max < self.gpu_safe_min
    }

    pub const fn contains(self, work_items: usize) -> bool {
        !self.is_valid()
            || (work_items > self.cpu_safe_max && work_items < self.gpu_safe_min)
    }

    pub const fn decide(self, work_items: usize, gpu_eligible: bool) -> LocalizedDecision {
        if !gpu_eligible {
            return LocalizedDecision::Direct(BackendKind::Cpu);
        }
        if !self.is_valid() {
            return LocalizedDecision::Recalibrate;
        }
        if work_items <= self.cpu_safe_max {
            LocalizedDecision::Direct(BackendKind::Cpu)
        } else if work_items >= self.gpu_safe_min {
            LocalizedDecision::Direct(BackendKind::Gpu)
        } else {
            LocalizedDecision::Recalibrate
        }
    }

    /// Expand the uncertainty band only when an audited direct decision is
    /// contradicted by the measured winner.
    ///
    /// This function performs no sampling itself. The caller decides when an
    /// audit/probe is worth paying for; ordinary calls remain two comparisons.
    pub fn observe_audit(&mut self, work_items: usize, observed_best: BackendKind) -> bool {
        if !self.is_valid() {
            return false;
        }

        match self.decide(work_items, true) {
            LocalizedDecision::Direct(BackendKind::Cpu) if observed_best == BackendKind::Gpu => {
                self.cpu_safe_max = work_items.saturating_sub(1);
                true
            }
            LocalizedDecision::Direct(BackendKind::Gpu) if observed_best == BackendKind::Cpu => {
                self.gpu_safe_min = work_items.saturating_add(1);
                true
            }
            _ => false,
        }
    }

    /// Publish a newly calibrated safe region. The caller is responsible for
    /// deriving these bounds from measurements; the hot path never fits a
    /// model or mutates state.
    pub fn publish(&mut self, cpu_safe_max: usize, gpu_safe_min: usize) {
        self.cpu_safe_max = cpu_safe_max;
        self.gpu_safe_min = gpu_safe_min;
    }
}

/// Coarse runtime-pressure epoch.
///
/// These bits are deliberately descriptive rather than predictive. A change
/// invalidates confidence in a published boundary; it does not choose a
/// backend by itself.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResourceEpoch(u8);

impl ResourceEpoch {
    const CPU_BUSY: u8 = 1 << 0;
    const MEMORY_PRESSURE: u8 = 1 << 1;
    const ACCELERATOR_BUSY: u8 = 1 << 2;

    pub const fn new(cpu_busy: bool, memory_pressure: bool, accelerator_busy: bool) -> Self {
        Self(
            (if cpu_busy { Self::CPU_BUSY } else { 0 })
                | (if memory_pressure {
                    Self::MEMORY_PRESSURE
                } else {
                    0
                })
                | (if accelerator_busy {
                    Self::ACCELERATOR_BUSY
                } else {
                    0
                }),
        )
    }

    pub const fn cpu_busy(self) -> bool {
        self.0 & Self::CPU_BUSY != 0
    }

    pub const fn memory_pressure(self) -> bool {
        self.0 & Self::MEMORY_PRESSURE != 0
    }

    pub const fn accelerator_busy(self) -> bool {
        self.0 & Self::ACCELERATOR_BUSY != 0
    }
}

/// Expected cost/benefit of one localized recalibration.
///
/// The controller recalibrates only if the expected remaining use count can
/// amortize the estimated calibration cost. This replaces an arbitrary
/// "wait N calls" debounce timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecalibrationEconomics {
    pub estimated_cost_ns: u64,
    pub estimated_regret_per_use_ns: u64,
}

impl RecalibrationEconomics {
    pub const fn break_even_uses(self) -> Option<u64> {
        if self.estimated_regret_per_use_ns == 0 {
            return None;
        }

        Some(
            self.estimated_cost_ns
                .saturating_add(self.estimated_regret_per_use_ns - 1)
                / self.estimated_regret_per_use_ns,
        )
    }

    pub const fn pays_back_within(self, expected_remaining_uses: u64) -> bool {
        match self.break_even_uses() {
            Some(required) => expected_remaining_uses >= required,
            None => false,
        }
    }
}

/// Lazy control-plane state for one localized crossover boundary.
///
/// FastRoute remains unchanged. This state only decides whether an already
/// stale boundary is worth recalibrating when a relevant call reaches the
/// crossover region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LazyLocalizedState {
    pub boundary: LocalizedBoundary,
    pub epoch: ResourceEpoch,
    stale: bool,
}

impl LazyLocalizedState {
    /// Reuse the existing M12 uncertainty margin for passive timing drift.
    pub const TIMING_DRIFT_PERCENT: u64 = 10;

    pub const fn new(boundary: LocalizedBoundary, epoch: ResourceEpoch) -> Self {
        Self {
            boundary,
            epoch,
            stale: false,
        }
    }

    pub const fn is_stale(self) -> bool {
        self.stale
    }

    /// Mark the boundary stale only when the coarse resource epoch changes.
    pub fn observe_epoch(&mut self, epoch: ResourceEpoch) -> bool {
        if epoch == self.epoch {
            return false;
        }
        self.epoch = epoch;
        self.stale = true;
        true
    }

    /// Passive fallback for same-epoch drift.
    ///
    /// Only the backend that already ran is observed. No alternate backend is
    /// probed. A >10% slowdown relative to the supplied reference marks the
    /// boundary stale.
    pub fn observe_selected_timing(&mut self, reference_ns: u64, observed_ns: u64) -> bool {
        if reference_ns == 0 {
            return false;
        }

        let lhs = observed_ns as u128 * 100;
        let rhs = reference_ns as u128 * (100 + Self::TIMING_DRIFT_PERCENT as u128);
        if lhs > rhs {
            self.stale = true;
            true
        } else {
            false
        }
    }

    /// Decide whether this call should pay the recalibration cost.
    ///
    /// A stale bit alone is not enough. Recalibration is lazy:
    /// - GPU must actually be eligible;
    /// - the call must land in the localized uncertainty region;
    /// - expected future reuse must be large enough to amortize calibration.
    pub const fn should_recalibrate(
        self,
        work_items: usize,
        gpu_eligible: bool,
        expected_remaining_uses: u64,
        economics: RecalibrationEconomics,
    ) -> bool {
        self.stale
            && gpu_eligible
            && self.boundary.contains(work_items)
            && economics.pays_back_within(expected_remaining_uses)
    }

    /// Publish a newly calibrated boundary and return to the direct path.
    pub fn publish(&mut self, boundary: LocalizedBoundary, epoch: ResourceEpoch) {
        self.boundary = boundary;
        self.epoch = epoch;
        self.stale = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_matches_bootstrap_boundaries() {
        let profile = CalibrationProfile::default();

        assert_eq!(select(SERIAL_MAX_ITEMS, true, profile), BackendKind::Serial);
        assert_eq!(
            select(SERIAL_MAX_ITEMS + 1, true, profile),
            BackendKind::Cpu
        );
        assert_eq!(select(CPU_MAX_ITEMS + 1, true, profile), BackendKind::Gpu);
    }

    #[test]
    fn calibrated_profile_changes_boundaries() {
        let profile = CalibrationProfile {
            serial_max_items: 64,
            cpu_max_items: 4096,
        };

        assert_eq!(select(64, true, profile), BackendKind::Serial);
        assert_eq!(select(65, true, profile), BackendKind::Cpu);
        assert_eq!(select(4097, true, profile), BackendKind::Gpu);
    }

    #[test]
    fn gpu_ineligible_work_never_selects_gpu() {
        let profile = CalibrationProfile {
            serial_max_items: 64,
            cpu_max_items: 4096,
        };

        assert_eq!(select(1_000_000, false, profile), BackendKind::Cpu);
    }

    #[test]
    fn localized_boundary_is_direct_only_outside_uncertainty_band() {
        let boundary = LocalizedBoundary::new(32_768, 65_536);

        assert_eq!(
            boundary.decide(16_384, true),
            LocalizedDecision::Direct(BackendKind::Cpu)
        );
        assert_eq!(
            boundary.decide(49_152, true),
            LocalizedDecision::Recalibrate
        );
        assert_eq!(
            boundary.decide(131_072, true),
            LocalizedDecision::Direct(BackendKind::Gpu)
        );
    }

    #[test]
    fn localized_boundary_expands_only_after_observed_direct_miss() {
        let mut boundary = LocalizedBoundary::new(32_768, 65_536);

        assert!(!boundary.observe_audit(16_384, BackendKind::Cpu));
        assert_eq!(boundary.cpu_safe_max, 32_768);

        assert!(boundary.observe_audit(16_384, BackendKind::Gpu));
        assert_eq!(boundary.cpu_safe_max, 16_383);
        assert_eq!(
            boundary.decide(16_384, true),
            LocalizedDecision::Recalibrate
        );

        assert!(boundary.observe_audit(131_072, BackendKind::Cpu));
        assert_eq!(boundary.gpu_safe_min, 131_073);
        assert_eq!(
            boundary.decide(131_072, true),
            LocalizedDecision::Recalibrate
        );
    }

    #[test]
    fn localized_boundary_can_be_tightened_after_recalibration() {
        let mut boundary = LocalizedBoundary::new(16_383, 131_073);
        boundary.publish(32_768, 65_536);

        assert_eq!(
            boundary.decide(16_384, true),
            LocalizedDecision::Direct(BackendKind::Cpu)
        );
        assert_eq!(
            boundary.decide(131_072, true),
            LocalizedDecision::Direct(BackendKind::Gpu)
        );
    }

    #[test]
    fn invalid_localized_boundary_fails_to_recalibration() {
        let boundary = LocalizedBoundary::new(65_536, 32_768);
        assert_eq!(
            boundary.decide(49_152, true),
            LocalizedDecision::Recalibrate
        );
    }

    #[test]
    fn localized_boundary_state_is_two_machine_words() {
        assert_eq!(
            std::mem::size_of::<LocalizedBoundary>(),
            2 * std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn resource_epoch_only_marks_stale_on_change() {
        let idle = ResourceEpoch::new(false, false, false);
        let cpu_busy = ResourceEpoch::new(true, false, false);
        let mut state = LazyLocalizedState::new(
            LocalizedBoundary::new(32_768, 65_536),
            idle,
        );

        assert!(!state.observe_epoch(idle));
        assert!(!state.is_stale());
        assert!(state.observe_epoch(cpu_busy));
        assert!(state.is_stale());
        assert!(state.epoch.cpu_busy());
    }

    #[test]
    fn selected_timing_uses_ten_percent_passive_fallback() {
        let idle = ResourceEpoch::default();
        let mut state = LazyLocalizedState::new(
            LocalizedBoundary::new(32_768, 65_536),
            idle,
        );

        assert!(!state.observe_selected_timing(1_000, 1_100));
        assert!(!state.is_stale());
        assert!(state.observe_selected_timing(1_000, 1_101));
        assert!(state.is_stale());
    }

    #[test]
    fn recalibration_economics_uses_ceiling_break_even() {
        let economics = RecalibrationEconomics {
            estimated_cost_ns: 1_001,
            estimated_regret_per_use_ns: 100,
        };

        assert_eq!(economics.break_even_uses(), Some(11));
        assert!(!economics.pays_back_within(10));
        assert!(economics.pays_back_within(11));
    }

    #[test]
    fn zero_expected_regret_never_pays_for_recalibration() {
        let economics = RecalibrationEconomics {
            estimated_cost_ns: 10_000,
            estimated_regret_per_use_ns: 0,
        };

        assert_eq!(economics.break_even_uses(), None);
        assert!(!economics.pays_back_within(u64::MAX));
    }

    #[test]
    fn stale_boundary_recalibrates_only_near_crossover_and_when_amortized() {
        let mut state = LazyLocalizedState::new(
            LocalizedBoundary::new(32_768, 65_536),
            ResourceEpoch::default(),
        );
        state.observe_epoch(ResourceEpoch::new(true, false, false));

        let economics = RecalibrationEconomics {
            estimated_cost_ns: 2_000,
            estimated_regret_per_use_ns: 100,
        };

        assert!(!state.should_recalibrate(16_384, true, 100, economics));
        assert!(!state.should_recalibrate(49_152, true, 19, economics));
        assert!(state.should_recalibrate(49_152, true, 20, economics));
        assert!(!state.should_recalibrate(49_152, false, 100, economics));
    }

    #[test]
    fn publishing_new_boundary_clears_stale_state() {
        let mut state = LazyLocalizedState::new(
            LocalizedBoundary::new(32_768, 65_536),
            ResourceEpoch::default(),
        );
        let busy = ResourceEpoch::new(true, true, false);
        state.observe_epoch(busy);
        assert!(state.is_stale());

        state.publish(LocalizedBoundary::new(65_536, 131_072), busy);

        assert!(!state.is_stale());
        assert_eq!(state.boundary, LocalizedBoundary::new(65_536, 131_072));
        assert_eq!(state.epoch, busy);
    }
}
