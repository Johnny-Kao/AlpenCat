//! Rayon-backed CPU adapter.
//!
//! The adapter owns bounded Rayon pools and reuses them by execution budget.
//! It does not decide whether CPU should be selected.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use runtime_core::{ExecutionBudget, WorkRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuExecutionKind {
    SerialBudget,
    SerialNested,
    SerialExternalParallelism,
    SerialResourceLimited,
    Parallel,
    ParallelBudgetLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuExecutionOptions {
    pub budget: ExecutionBudget,
    pub external_parallelism: bool,
}

impl CpuExecutionOptions {
    pub const fn new(budget: ExecutionBudget) -> Self {
        Self {
            budget,
            external_parallelism: false,
        }
    }

    pub const fn with_external_parallelism(mut self, active: bool) -> Self {
        self.external_parallelism = active;
        self
    }
}

#[derive(Debug)]
pub struct CpuExecution<T> {
    pub kind: CpuExecutionKind,
    pub values: Vec<T>,
}

#[derive(Debug, Default)]
pub struct CpuAdapter {
    pools: Mutex<HashMap<usize, Arc<ThreadPool>>>,
}

impl CpuAdapter {
    pub const fn supports(&self, _range: WorkRange) -> bool {
        true
    }

    pub fn in_rayon_parallel_context(&self) -> bool {
        rayon::current_thread_index().is_some()
    }

    fn pool_for(&self, max_parallelism: usize) -> Option<Arc<ThreadPool>> {
        let mut pools = self
            .pools
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(pool) = pools.get(&max_parallelism) {
            return Some(Arc::clone(pool));
        }

        let pool = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(max_parallelism)
                .build()
                .ok()?,
        );

        pools.insert(max_parallelism, Arc::clone(&pool));
        Some(pool)
    }

    pub fn cached_pool_count(&self) -> usize {
        self.pools
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn effective_parallelism(&self, budget: ExecutionBudget) -> usize {
        let available = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1);
        budget.max_parallelism.min(available).max(1)
    }

    pub fn map<T, F>(
        &self,
        range: WorkRange,
        budget: ExecutionBudget,
        operation: F,
    ) -> CpuExecution<T>
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        self.map_with_options(range, CpuExecutionOptions::new(budget), operation)
    }

    pub fn map_with_options<T, F>(
        &self,
        range: WorkRange,
        options: CpuExecutionOptions,
        operation: F,
    ) -> CpuExecution<T>
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        if options.external_parallelism {
            return CpuExecution {
                kind: CpuExecutionKind::SerialExternalParallelism,
                values: (range.begin..range.end).map(operation).collect(),
            };
        }

        let effective_parallelism = self.effective_parallelism(options.budget);

        if effective_parallelism <= 1 {
            return CpuExecution {
                kind: CpuExecutionKind::SerialBudget,
                values: (range.begin..range.end).map(operation).collect(),
            };
        }

        if self.in_rayon_parallel_context() {
            return CpuExecution {
                kind: CpuExecutionKind::SerialNested,
                values: (range.begin..range.end).map(operation).collect(),
            };
        }

        let Some(pool) = self.pool_for(effective_parallelism) else {
            return CpuExecution {
                kind: CpuExecutionKind::SerialResourceLimited,
                values: (range.begin..range.end).map(operation).collect(),
            };
        };

        let values = pool.install(|| {
            (range.begin..range.end)
                .into_par_iter()
                .map(operation)
                .collect()
        });

        CpuExecution {
            kind: if effective_parallelism < options.budget.max_parallelism {
                CpuExecutionKind::ParallelBudgetLimited
            } else {
                CpuExecutionKind::Parallel
            },
            values,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_budget_reuses_pool() {
        let adapter = CpuAdapter::default();
        let range = WorkRange::new(0, 4096);
        let budget = ExecutionBudget::new(2);

        adapter.map(range, budget, |index| index);
        assert_eq!(adapter.cached_pool_count(), 1);

        adapter.map(range, budget, |index| index);
        assert_eq!(adapter.cached_pool_count(), 1);
    }

    #[test]
    fn different_budgets_get_distinct_cached_pools() {
        let adapter = CpuAdapter::default();
        let range = WorkRange::new(0, 4096);

        adapter.map(range, ExecutionBudget::new(2), |index| index);
        adapter.map(range, ExecutionBudget::new(3), |index| index);

        assert_eq!(adapter.cached_pool_count(), 2);
    }

    #[test]
    fn serial_budget_does_not_create_pool() {
        let adapter = CpuAdapter::default();
        let range = WorkRange::new(0, 4096);

        adapter.map(range, ExecutionBudget::serial(), |index| index);

        assert_eq!(adapter.cached_pool_count(), 0);
    }

    #[test]
    fn absurd_budget_is_capped_to_host_parallelism() {
        let adapter = CpuAdapter::default();
        let effective = adapter.effective_parallelism(ExecutionBudget::new(usize::MAX));
        let available = std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1);

        assert_eq!(effective, available);
    }

    #[test]
    fn external_parallelism_uses_serial_wrapper_path_without_pool_creation() {
        let adapter = CpuAdapter::default();
        let execution = adapter.map_with_options(
            WorkRange::new(0, 4096),
            CpuExecutionOptions::new(ExecutionBudget::new(4))
                .with_external_parallelism(true),
            |index| index,
        );

        assert_eq!(
            execution.kind,
            CpuExecutionKind::SerialExternalParallelism
        );
        assert_eq!(adapter.cached_pool_count(), 0);
    }
}
