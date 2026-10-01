//! Cached execution-policy layer for ultra-hot / tiny work.
//!
//! This is a control-plane cache. Adapters should cache the returned policy
//! at their own signature/task boundary and keep the dataplane free of
//! repeated runtime arbitration.

use std::collections::HashMap;
use std::sync::Mutex;

use runtime_broker::{BrokerCapacity, BrokerRequest, ResourceBroker};
use runtime_core::BackendKind;
use runtime_telemetry::RuntimeTelemetrySnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyCacheStatus {
    Hit,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedExecutionPolicy {
    pub backend: BackendKind,
    pub status: PolicyCacheStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ResourceFingerprint {
    cpu_slots: usize,
    gpu_slots: usize,
    cpu_pressure_band: u8,
    gpu_pressure_band: u8,
    gpu_range_eligible: bool,
    cpu_failed: u64,
    gpu_failed: u64,
}

impl ResourceFingerprint {
    fn new(
        telemetry: RuntimeTelemetrySnapshot,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Self {
        Self {
            cpu_slots: capacity.cpu_slots,
            gpu_slots: capacity.gpu_slots,
            cpu_pressure_band: pressure_band(telemetry.cpu.in_flight, capacity.cpu_slots),
            gpu_pressure_band: pressure_band(telemetry.gpu.in_flight, capacity.gpu_slots),
            gpu_range_eligible: request.gpu_range_eligible,
            cpu_failed: telemetry.cpu.failed,
            gpu_failed: telemetry.gpu.failed,
        }
    }
}

fn pressure_band(in_flight: usize, slots: usize) -> u8 {
    if slots == 0 {
        return 4;
    }
    if in_flight >= slots {
        return 4;
    }
    ((in_flight.saturating_mul(4)) / slots).min(3) as u8
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PolicyKey {
    task_id: String,
    work_class: u8,
}

impl PolicyKey {
    fn new(task_id: &str, work_items: usize) -> Self {
        Self {
            task_id: task_id.to_owned(),
            work_class: work_class(work_items),
        }
    }
}

fn work_class(work_items: usize) -> u8 {
    if work_items == 0 {
        0
    } else {
        (usize::BITS - work_items.leading_zeros()) as u8
    }
}

#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    backend: BackendKind,
    fingerprint: ResourceFingerprint,
    last_used: u64,
}

#[derive(Debug, Default)]
struct CacheState {
    entries: HashMap<PolicyKey, CacheEntry>,
    clock: u64,
}

#[derive(Debug)]
pub struct ExecutionPolicyCache {
    state: Mutex<CacheState>,
    max_entries: usize,
}

impl Default for ExecutionPolicyCache {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl ExecutionPolicyCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            state: Mutex::new(CacheState::default()),
            max_entries: max_entries.max(1),
        }
    }

    pub fn resolve(
        &self,
        task_id: &str,
        work_items: usize,
        telemetry: RuntimeTelemetrySnapshot,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Option<CachedExecutionPolicy> {
        self.resolve_with(task_id, work_items, telemetry, capacity, request, || {
            ResourceBroker.select_backend(telemetry, capacity, request)
        })
    }

    pub fn resolve_with<F>(
        &self,
        task_id: &str,
        work_items: usize,
        telemetry: RuntimeTelemetrySnapshot,
        capacity: BrokerCapacity,
        request: BrokerRequest,
        planner: F,
    ) -> Option<CachedExecutionPolicy>
    where
        F: FnOnce() -> Option<BackendKind>,
    {
        let key = PolicyKey::new(task_id, work_items);
        let fingerprint = ResourceFingerprint::new(telemetry, capacity, request);

        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.clock = state.clock.wrapping_add(1);
            let now = state.clock;

            if let Some(entry) = state.entries.get_mut(&key) {
                if entry.fingerprint == fingerprint {
                    entry.last_used = now;
                    return Some(CachedExecutionPolicy {
                        backend: entry.backend,
                        status: PolicyCacheStatus::Hit,
                    });
                }
            }
        }

        // Planning can involve another synchronized subsystem (for example
        // the online cost model). Do not hold the cache mutex while doing it.
        let backend = planner()?;

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.clock = state.clock.wrapping_add(1);
        let now = state.clock;

        // Another thread may have populated the same policy while this thread
        // was planning. Prefer the now-valid cached answer.
        if let Some(entry) = state.entries.get_mut(&key) {
            if entry.fingerprint == fingerprint {
                entry.last_used = now;
                return Some(CachedExecutionPolicy {
                    backend: entry.backend,
                    status: PolicyCacheStatus::Hit,
                });
            }
        }

        if state.entries.len() >= self.max_entries && !state.entries.contains_key(&key) {
            if let Some(oldest) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            {
                state.entries.remove(&oldest);
            }
        }
        state.entries.insert(
            key,
            CacheEntry {
                backend,
                fingerprint,
                last_used: now,
            },
        );

        Some(CachedExecutionPolicy {
            backend,
            status: PolicyCacheStatus::Planned,
        })
    }

    pub fn invalidate_task(&self, task_id: &str) -> usize {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = state.entries.len();
        state.entries.retain(|key, _| key.task_id != task_id);
        before - state.entries.len()
    }

    pub fn clear(&self) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_telemetry::BackendTelemetrySnapshot;

    fn backend(in_flight: usize, failed: u64) -> BackendTelemetrySnapshot {
        BackendTelemetrySnapshot {
            in_flight,
            completed: 0,
            failed,
            work_items: 0,
            elapsed_nanos: 0,
            last_work_items: 0,
            last_elapsed_nanos: 0,
        }
    }

    fn telemetry(
        cpu_in_flight: usize,
        gpu_in_flight: usize,
        gpu_failed: u64,
    ) -> RuntimeTelemetrySnapshot {
        RuntimeTelemetrySnapshot {
            serial: backend(0, 0),
            cpu: backend(cpu_in_flight, 0),
            gpu: backend(gpu_in_flight, gpu_failed),
        }
    }

    fn request() -> BrokerRequest {
        BrokerRequest {
            gpu_range_eligible: true,
        }
    }

    #[test]
    fn identical_state_hits_cache() {
        let cache = ExecutionPolicyCache::new(8);
        let cap = BrokerCapacity::new(4, 1);
        let first = cache
            .resolve("x", 100, telemetry(0, 0, 0), cap, request())
            .unwrap();
        let second = cache
            .resolve("x", 100, telemetry(0, 0, 0), cap, request())
            .unwrap();
        assert_eq!(first.status, PolicyCacheStatus::Planned);
        assert_eq!(second.status, PolicyCacheStatus::Hit);
        assert_eq!(first.backend, second.backend);
    }

    #[test]
    fn material_pressure_change_replans() {
        let cache = ExecutionPolicyCache::new(8);
        let cap = BrokerCapacity::new(4, 1);
        let first = cache
            .resolve("x", 100, telemetry(0, 0, 0), cap, request())
            .unwrap();
        let second = cache
            .resolve("x", 100, telemetry(3, 0, 0), cap, request())
            .unwrap();
        assert_eq!(first.backend, BackendKind::Cpu);
        assert_eq!(second.backend, BackendKind::Gpu);
        assert_eq!(second.status, PolicyCacheStatus::Planned);
    }

    #[test]
    fn small_pressure_change_within_band_hits() {
        let cache = ExecutionPolicyCache::new(8);
        let cap = BrokerCapacity::new(8, 1);
        let _ = cache
            .resolve("x", 100, telemetry(2, 0, 0), cap, request())
            .unwrap();
        let second = cache
            .resolve("x", 100, telemetry(3, 0, 0), cap, request())
            .unwrap();
        assert_eq!(second.status, PolicyCacheStatus::Hit);
    }

    #[test]
    fn backend_failure_invalidates_fingerprint() {
        let cache = ExecutionPolicyCache::new(8);
        let cap = BrokerCapacity::new(4, 1);
        let _ = cache
            .resolve("x", 100, telemetry(3, 0, 0), cap, request())
            .unwrap();
        let second = cache
            .resolve("x", 100, telemetry(3, 0, 1), cap, request())
            .unwrap();
        assert_eq!(second.status, PolicyCacheStatus::Planned);
    }

    #[test]
    fn task_invalidation_removes_all_work_classes() {
        let cache = ExecutionPolicyCache::new(8);
        let cap = BrokerCapacity::new(4, 1);
        let _ = cache.resolve("x", 10, telemetry(0, 0, 0), cap, request());
        let _ = cache.resolve("x", 10_000, telemetry(0, 0, 0), cap, request());
        assert_eq!(cache.invalidate_task("x"), 2);
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_is_bounded() {
        let cache = ExecutionPolicyCache::new(2);
        let cap = BrokerCapacity::new(4, 1);
        for id in ["a", "b", "c"] {
            let _ = cache.resolve(id, 100, telemetry(0, 0, 0), cap, request());
        }
        assert_eq!(cache.len(), 2);
    }
}
