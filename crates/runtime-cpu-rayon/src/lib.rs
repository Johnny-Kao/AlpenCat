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
    SerialResourceLimited,
    Parallel,
    ParallelBudgetLimited,
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
        let effective_parallelism = self.effective_parallelism(budget);

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
            kind: if effective_parallelism < budget.max_parallelism {
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
    fn same_budget_reuses_pool_when_parallelism_exists() {
        let adapter = CpuAdapter::default();
        let range = WorkRange::new(0, 4096);
        let budget = ExecutionBudget::new(2);
        let effective = adapter.effective_parallelism(budget);
        let expected_pools = usize::from(effective > 1);

        adapter.map(range, budget, |index| index);
        assert_eq!(adapter.cached_pool_count(), expected_pools);

        adapter.map(range, budget, |index| index);
        assert_eq!(adapter.cached_pool_count(), expected_pools);
    }

    #[test]
    fn distinct_effective_parallelism_gets_distinct_cached_pools() {
        let adapter = CpuAdapter::default();
        let range = WorkRange::new(0, 4096);
        let budgets = [ExecutionBudget::new(2), ExecutionBudget::new(3)];

        for budget in budgets {
            adapter.map(range, budget, |index| index);
        }

        let mut effective = budgets
            .into_iter()
            .map(|budget| adapter.effective_parallelism(budget))
            .filter(|value| *value > 1)
            .collect::<Vec<_>>();
        effective.sort_unstable();
        effective.dedup();

        assert_eq!(adapter.cached_pool_count(), effective.len());
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
}
