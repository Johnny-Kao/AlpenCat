//! Minimal backend-neutral primitives for AlpenCat.
//!
//! The active runtime is deliberately small: a published execution boundary,
//! a resource epoch used to invalidate confidence in that boundary, and a few
//! execution types shared by the routing and backend layers.

use std::sync::atomic::{AtomicU64, Ordering};

pub const DEFAULT_SERIAL_MAX_ITEMS: usize = 1_024;
pub const DEFAULT_CPU_MAX_ITEMS: usize = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkRange {
    pub begin: usize,
    pub end: usize,
}

impl WorkRange {
    pub const fn new(begin: usize, end: usize) -> Self {
        Self { begin, end }
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.begin)
    }

    pub const fn is_empty(self) -> bool {
        self.begin >= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Serial,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionBudget {
    pub max_parallelism: usize,
}

impl ExecutionBudget {
    pub const fn new(max_parallelism: usize) -> Self {
        Self {
            max_parallelism: if max_parallelism == 0 {
                1
            } else {
                max_parallelism
            },
        }
    }

    pub const fn serial() -> Self {
        Self { max_parallelism: 1 }
    }
}

impl Default for ExecutionBudget {
    fn default() -> Self {
        Self {
            max_parallelism: std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1),
        }
    }
}

/// Published crossover points used by the fast routing path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundaryProfile {
    pub serial_max_items: usize,
    pub cpu_max_items: usize,
}

impl BoundaryProfile {
    pub const fn new(serial_max_items: usize, cpu_max_items: usize) -> Self {
        Self {
            serial_max_items,
            cpu_max_items,
        }
    }
}

impl Default for BoundaryProfile {
    fn default() -> Self {
        Self {
            serial_max_items: DEFAULT_SERIAL_MAX_ITEMS,
            cpu_max_items: DEFAULT_CPU_MAX_ITEMS,
        }
    }
}

/// A boundary together with the resource epoch at which it was published.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundarySnapshot {
    pub profile: BoundaryProfile,
    pub resource_epoch: u64,
}

impl BoundarySnapshot {
    pub const fn new(profile: BoundaryProfile, resource_epoch: u64) -> Self {
        Self {
            profile,
            resource_epoch,
        }
    }
}

/// Monotonic invalidation signal shared by thin platform adapters and FastRoute.
///
/// Platform adapters only advance the epoch. They do not make routing decisions.
#[derive(Debug)]
pub struct ResourceEpoch {
    value: AtomicU64,
}

impl ResourceEpoch {
    pub const fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    pub fn current(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn invalidate(&self) -> u64 {
        self.value.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }

    #[inline(always)]
    pub fn is_stale(&self, published_epoch: u64) -> bool {
        self.current() != published_epoch
    }
}

impl Default for ResourceEpoch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_epoch_marks_published_boundary_stale() {
        let epoch = ResourceEpoch::new();
        let published = epoch.current();
        assert!(!epoch.is_stale(published));

        assert_eq!(epoch.invalidate(), 1);
        assert!(epoch.is_stale(published));
    }

    #[test]
    fn default_boundary_is_ordered() {
        let boundary = BoundaryProfile::default();
        assert!(boundary.serial_max_items < boundary.cpu_max_items);
    }
}
