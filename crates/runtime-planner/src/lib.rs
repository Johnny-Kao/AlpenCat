//! Backend-neutral work decomposition.
//!
//! This crate converts logical work ranges into independently schedulable units.
//! Plans remain O(1) in metadata size: WorkUnits are generated lazily rather than
//! materialized for the entire logical range.

use std::collections::VecDeque;

use runtime_core::WorkRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkPolicy {
    pub target_items: usize,
    pub min_items: usize,
    pub max_items: usize,
}

impl ChunkPolicy {
    pub const fn new(target_items: usize, min_items: usize, max_items: usize) -> Self {
        let min_items = if min_items == 0 { 1 } else { min_items };
        let max_items = if max_items < min_items {
            min_items
        } else {
            max_items
        };
        let target_items = if target_items < min_items {
            min_items
        } else if target_items > max_items {
            max_items
        } else {
            target_items
        };

        Self {
            target_items,
            min_items,
            max_items,
        }
    }

    pub const fn fixed(items: usize) -> Self {
        let items = if items == 0 { 1 } else { items };
        Self {
            target_items: items,
            min_items: items,
            max_items: items,
        }
    }
}

impl Default for ChunkPolicy {
    fn default() -> Self {
        Self::new(16_384, 1_024, 262_144)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkUnit {
    pub id: usize,
    pub range: WorkRange,
}

impl WorkUnit {
    pub const fn len(self) -> usize {
        self.range.len()
    }

    pub const fn is_empty(self) -> bool {
        self.range.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkPlan {
    pub source: WorkRange,
    pub chunk_items: usize,
    unit_count: usize,
    base_items: usize,
    remainder: usize,
}

impl WorkPlan {
    pub const fn total_items(&self) -> usize {
        self.source.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.unit_count == 0
    }

    pub const fn unit_count(&self) -> usize {
        self.unit_count
    }

    pub fn unit(&self, id: usize) -> Option<WorkUnit> {
        if id >= self.unit_count {
            return None;
        }

        let extra_before = id.min(self.remainder);
        let begin_offset = id
            .saturating_mul(self.base_items)
            .saturating_add(extra_before);
        let len = self.base_items + usize::from(id < self.remainder);
        let begin = self.source.begin.saturating_add(begin_offset);
        let end = begin.saturating_add(len).min(self.source.end);

        Some(WorkUnit {
            id,
            range: WorkRange::new(begin, end),
        })
    }

    pub fn iter(&self) -> WorkPlanIter {
        WorkPlanIter {
            plan: *self,
            next_id: 0,
        }
    }
}

pub struct WorkPlanIter {
    plan: WorkPlan,
    next_id: usize,
}

impl Iterator for WorkPlanIter {
    type Item = WorkUnit;

    fn next(&mut self) -> Option<Self::Item> {
        let unit = self.plan.unit(self.next_id)?;
        self.next_id += 1;
        Some(unit)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.plan.unit_count.saturating_sub(self.next_id);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for WorkPlanIter {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkQueue {
    plan: WorkPlan,
    next_id: usize,
    requeued_front: VecDeque<WorkUnit>,
    requeued_back: VecDeque<WorkUnit>,
}

impl WorkQueue {
    pub fn from_plan(plan: &WorkPlan) -> Self {
        Self {
            plan: *plan,
            next_id: 0,
            requeued_front: VecDeque::new(),
            requeued_back: VecDeque::new(),
        }
    }

    pub fn claim_next(&mut self) -> Option<WorkUnit> {
        if let Some(unit) = self.requeued_front.pop_front() {
            return Some(unit);
        }

        if let Some(unit) = self.plan.unit(self.next_id) {
            self.next_id += 1;
            return Some(unit);
        }

        self.requeued_back.pop_front()
    }

    pub fn requeue_front(&mut self, unit: WorkUnit) {
        self.requeued_front.push_front(unit);
    }

    pub fn requeue_back(&mut self, unit: WorkUnit) {
        self.requeued_back.push_back(unit);
    }

    pub fn pending_count(&self) -> usize {
        self.requeued_front
            .len()
            .saturating_add(self.plan.unit_count.saturating_sub(self.next_id))
            .saturating_add(self.requeued_back.len())
    }

    pub fn is_empty(&self) -> bool {
        self.pending_count() == 0
    }
}

#[derive(Debug, Default)]
pub struct ChunkPlanner;

impl ChunkPlanner {
    pub fn plan(&self, range: WorkRange, policy: ChunkPolicy) -> WorkPlan {
        if range.is_empty() {
            return WorkPlan {
                source: range,
                chunk_items: policy.target_items,
                unit_count: 0,
                base_items: 0,
                remainder: 0,
            };
        }

        let total = range.len();
        let unit_count = choose_unit_count(total, policy);
        let base_items = total / unit_count;
        let remainder = total % unit_count;
        let chunk_items = base_items + usize::from(remainder > 0);

        WorkPlan {
            source: range,
            chunk_items,
            unit_count,
            base_items,
            remainder,
        }
    }
}

fn choose_unit_count(total: usize, policy: ChunkPolicy) -> usize {
    if total <= policy.min_items {
        return 1;
    }

    let target = policy
        .target_items
        .clamp(policy.min_items, policy.max_items);
    let mut count = total.div_ceil(target).max(1);
    let min_count_for_max = total.div_ceil(policy.max_items).max(1);
    count = count.max(min_count_for_max);

    while count > 1 && total / count < policy.min_items {
        count -= 1;
    }

    count.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_range_produces_no_units() {
        let plan = ChunkPlanner.plan(WorkRange::new(5, 5), ChunkPolicy::default());
        assert!(plan.is_empty());
        assert_eq!(plan.unit_count(), 0);
        assert_eq!(plan.total_items(), 0);
    }

    #[test]
    fn plan_is_contiguous_complete_and_non_overlapping() {
        let source = WorkRange::new(10, 10_010);
        let plan = ChunkPlanner.plan(source, ChunkPolicy::fixed(1024));
        let units: Vec<_> = plan.iter().collect();

        assert_eq!(plan.total_items(), source.len());
        assert_eq!(units.first().unwrap().range.begin, source.begin);
        assert_eq!(units.last().unwrap().range.end, source.end);

        for pair in units.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.begin);
            assert_eq!(pair[0].id + 1, pair[1].id);
        }
    }

    #[test]
    fn small_work_is_not_split_below_minimum() {
        let policy = ChunkPolicy::new(4096, 1024, 8192);
        let plan = ChunkPlanner.plan(WorkRange::new(0, 700), policy);

        assert_eq!(plan.unit_count(), 1);
        assert_eq!(plan.unit(0).unwrap().len(), 700);
    }

    #[test]
    fn policy_normalizes_invalid_bounds() {
        let policy = ChunkPolicy::new(0, 0, 0);
        assert_eq!(policy.min_items, 1);
        assert_eq!(policy.max_items, 1);
        assert_eq!(policy.target_items, 1);
    }

    #[test]
    fn unit_ids_are_stable_and_reassignable() {
        let plan = ChunkPlanner.plan(WorkRange::new(0, 4096), ChunkPolicy::fixed(1024));
        let ids: Vec<_> = plan.iter().map(|unit| unit.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
    }

    #[test]
    fn balanced_plan_avoids_tiny_tail_chunks() {
        let policy = ChunkPolicy::new(4096, 1024, 8192);
        let plan = ChunkPlanner.plan(WorkRange::new(0, 10_000), policy);
        let units: Vec<_> = plan.iter().collect();

        assert_eq!(plan.total_items(), 10_000);
        assert!(units.len() >= 2);
        assert!(units.iter().all(|unit| unit.len() >= policy.min_items));
        assert!(units.iter().all(|unit| unit.len() <= policy.max_items));

        let min = units.iter().map(|unit| unit.len()).min().unwrap();
        let max = units.iter().map(|unit| unit.len()).max().unwrap();
        assert!(max - min <= 1);
    }

    #[test]
    fn work_queue_supports_claim_and_requeue() {
        let plan = ChunkPlanner.plan(WorkRange::new(0, 4096), ChunkPolicy::fixed(1024));
        let mut queue = WorkQueue::from_plan(&plan);

        let first = queue.claim_next().unwrap();
        let second = queue.claim_next().unwrap();
        assert_eq!(first.id, 0);
        assert_eq!(second.id, 1);
        assert_eq!(queue.pending_count(), 2);

        queue.requeue_front(second);
        assert_eq!(queue.claim_next().unwrap().id, 1);

        queue.requeue_back(first);
        assert_eq!(queue.pending_count(), 3);
        assert_eq!(queue.claim_next().unwrap().id, 2);
        assert_eq!(queue.claim_next().unwrap().id, 3);
        assert_eq!(queue.claim_next().unwrap().id, 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn huge_plan_has_constant_metadata_and_lazy_queue() {
        let plan = ChunkPlanner.plan(WorkRange::new(0, 1_000_000_000_000), ChunkPolicy::fixed(1));

        assert_eq!(plan.unit_count(), 1_000_000_000_000);
        assert_eq!(plan.unit(0).unwrap().range, WorkRange::new(0, 1));
        assert_eq!(
            plan.unit(999_999_999_999).unwrap().range,
            WorkRange::new(999_999_999_999, 1_000_000_000_000)
        );

        let mut queue = WorkQueue::from_plan(&plan);
        assert_eq!(queue.pending_count(), 1_000_000_000_000);
        assert_eq!(queue.claim_next().unwrap().id, 0);
        assert_eq!(queue.pending_count(), 999_999_999_999);
    }

    #[test]
    fn near_usize_max_range_stays_contiguous() {
        let start = usize::MAX - 10_000;
        let source = WorkRange::new(start, usize::MAX);
        let plan = ChunkPlanner.plan(source, ChunkPolicy::fixed(1024));

        assert_eq!(plan.iter().map(WorkUnit::len).sum::<usize>(), 10_000);
        assert_eq!(plan.unit(0).unwrap().range.begin, start);
        assert_eq!(
            plan.unit(plan.unit_count() - 1).unwrap().range.end,
            usize::MAX
        );
    }
}
