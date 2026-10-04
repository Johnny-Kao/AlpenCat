//! AlpenCat's hot routing boundary.
//!
//! This crate intentionally does not observe telemetry or predict performance.
//! It chooses a route from a published boundary and explicit backend eligibility.

use runtime_core::{BackendKind, BoundaryProfile};

pub use runtime_core::{
    DEFAULT_CPU_MAX_ITEMS as CPU_MAX_ITEMS, DEFAULT_SERIAL_MAX_ITEMS as SERIAL_MAX_ITEMS,
};

#[inline(always)]
pub const fn select(
    work_items: usize,
    gpu_eligible: bool,
    boundary: BoundaryProfile,
) -> BackendKind {
    if work_items <= boundary.serial_max_items {
        BackendKind::Serial
    } else if work_items <= boundary.cpu_max_items || !gpu_eligible {
        BackendKind::Cpu
    } else {
        BackendKind::Gpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_boundary_routes_serial_cpu_gpu() {
        let boundary = BoundaryProfile::default();

        assert_eq!(
            select(SERIAL_MAX_ITEMS, true, boundary),
            BackendKind::Serial
        );
        assert_eq!(
            select(SERIAL_MAX_ITEMS + 1, true, boundary),
            BackendKind::Cpu
        );
        assert_eq!(select(CPU_MAX_ITEMS + 1, true, boundary), BackendKind::Gpu);
    }

    #[test]
    fn gpu_ineligible_work_stays_on_cpu() {
        let boundary = BoundaryProfile::new(64, 4096);
        assert_eq!(select(1_000_000, false, boundary), BackendKind::Cpu);
    }
}
