//! Backend-neutral internal primitives for the experimental runtime framework.

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

/// Default number of normalized resource-pressure bands used by control-plane
/// components that do not provide their own granularity.
pub const DEFAULT_PRESSURE_BANDS: u8 = 4;

/// Quantize runtime-owned in-flight work into a stable pressure band.
///
/// Keeping this calculation in runtime-core prevents policy-cache invalidation
/// and rebalancing from silently drifting to different pressure semantics.
pub const fn pressure_band(in_flight: usize, slots: usize, bands: u8) -> u8 {
    let bands = if bands < 2 { 2 } else { bands };
    if slots == 0 || in_flight >= slots {
        return bands;
    }
    let band = (in_flight.saturating_mul(bands as usize)) / slots;
    let max_active_band = bands.saturating_sub(1) as usize;
    if band > max_active_band {
        max_active_band as u8
    } else {
        band as u8
    }
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
