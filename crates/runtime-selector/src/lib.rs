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
            LocalizedDecision::Direct(BackendKind::Cpu)
                if observed_best == BackendKind::Gpu =>
            {
                self.cpu_safe_max = work_items.saturating_sub(1);
                true
            }
            LocalizedDecision::Direct(BackendKind::Gpu)
                if observed_best == BackendKind::Cpu =>
            {
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
}
