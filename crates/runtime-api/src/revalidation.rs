//! Bounded serial/CPU revalidation for stale execution boundaries.
//!
//! This module is intentionally cold-path code. It measures only a bounded
//! neighborhood around the previously published crossover and never
//! extrapolates a local failure into a global routing claim.

use runtime_core::{BackendKind, BoundaryProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedRevalidationConfig {
    pub max_points: usize,
    pub min_items: usize,
    pub max_items: usize,
}

impl BoundedRevalidationConfig {
    pub const fn new(max_points: usize, min_items: usize, max_items: usize) -> Self {
        Self {
            max_points,
            min_items,
            max_items,
        }
    }
}

impl Default for BoundedRevalidationConfig {
    fn default() -> Self {
        Self {
            max_points: 3,
            min_items: 1,
            max_items: usize::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteMeasurement {
    pub work_items: usize,
    pub serial_cost: u64,
    pub cpu_cost: u64,
}

impl RouteMeasurement {
    pub const fn preferred_backend(self) -> BackendKind {
        if self.serial_cost <= self.cpu_cost {
            BackendKind::Serial
        } else {
            BackendKind::Cpu
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBoundaryEvidence {
    pub proposed_boundary: Option<BoundaryProfile>,
    pub measurements: Vec<RouteMeasurement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevalidationStatus {
    NotStale,
    Published,
    NoLocalCrossover,
    InvalidatedDuringMeasurement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRevalidationOutcome {
    pub status: RevalidationStatus,
    pub evidence: LocalBoundaryEvidence,
    pub published_boundary: Option<runtime_core::BoundarySnapshot>,
}

fn measure_point<F>(work_items: usize, measure: &mut F) -> RouteMeasurement
where
    F: FnMut(usize, BackendKind) -> u64,
{
    RouteMeasurement {
        work_items,
        serial_cost: measure(work_items, BackendKind::Serial),
        cpu_cost: measure(work_items, BackendKind::Cpu),
    }
}

/// Search only a bounded neighborhood around the old serial/CPU boundary.
///
/// If no local crossover is observed, the proposed boundary is absent. The
/// caller must leave the old boundary stale rather than extrapolating that the
/// CPU route disappeared globally.
pub fn bounded_revalidate_serial_cpu<F>(
    boundary: BoundaryProfile,
    config: BoundedRevalidationConfig,
    mut measure: F,
) -> LocalBoundaryEvidence
where
    F: FnMut(usize, BackendKind) -> u64,
{
    let min_items = config.min_items.max(1);
    let max_items = config.max_items.max(min_items);
    let max_points = config.max_points.max(1);
    let start = boundary.serial_max_items.clamp(min_items, max_items);

    let mut measurements = Vec::with_capacity(max_points);
    let first = measure_point(start, &mut measure);
    measurements.push(first);

    let mut serial_point = None;
    let mut cpu_point = None;

    match first.preferred_backend() {
        BackendKind::Serial => serial_point = Some(start),
        BackendKind::Cpu => cpu_point = Some(start),
        BackendKind::Gpu => unreachable!(),
    }

    let mut current = start;
    while measurements.len() < max_points && (serial_point.is_none() || cpu_point.is_none()) {
        let next = if serial_point.is_some() {
            current.saturating_mul(2).min(max_items)
        } else {
            (current / 2).max(min_items)
        };

        if next == current {
            break;
        }

        current = next;
        let measurement = measure_point(current, &mut measure);
        measurements.push(measurement);

        match measurement.preferred_backend() {
            BackendKind::Serial => serial_point = Some(current),
            BackendKind::Cpu => cpu_point = Some(current),
            BackendKind::Gpu => unreachable!(),
        }
    }

    let proposed_boundary = match (serial_point, cpu_point) {
        (Some(serial), Some(cpu)) if serial < cpu => Some(BoundaryProfile::new(
            serial,
            boundary.cpu_max_items.max(serial.saturating_add(1)),
        )),
        _ => None,
    };

    LocalBoundaryEvidence {
        proposed_boundary,
        measurements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_a_nearby_crossover_without_global_search() {
        let result = bounded_revalidate_serial_cpu(
            BoundaryProfile::new(1_024, 262_144),
            BoundedRevalidationConfig::new(3, 256, 8_192),
            |n, backend| match backend {
                BackendKind::Serial => n as u64,
                BackendKind::Cpu => 1_500,
                BackendKind::Gpu => unreachable!(),
            },
        );

        assert_eq!(
            result.proposed_boundary,
            Some(BoundaryProfile::new(1_024, 262_144))
        );
        assert_eq!(result.measurements.len(), 2);
    }

    #[test]
    fn local_failure_does_not_infer_global_serial_fallback() {
        let result = bounded_revalidate_serial_cpu(
            BoundaryProfile::new(1_024, 262_144),
            BoundedRevalidationConfig::new(3, 256, 8_192),
            |_n, backend| match backend {
                BackendKind::Serial => 100,
                BackendKind::Cpu => 200,
                BackendKind::Gpu => unreachable!(),
            },
        );

        assert_eq!(result.proposed_boundary, None);
        assert_eq!(result.measurements.len(), 3);
    }
}
