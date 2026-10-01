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
}
