//! Explicit, bounded resource-state lifecycle: no implicit OS reads or workers.
//! A cached snapshot is a historical *observation*, never a resource entitlement.
use super::{snapshot, ResourceSnapshot};
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Empty,
    Fresh,
    /// Caller invalidated the observation; retain it only as historical evidence.
    Stale,
    /// The last successful observation is older than the caller's maximum age.
    Expired,
}

#[derive(Debug, Clone)]
pub struct CachedSnapshot {
    pub snapshot: Option<Arc<ResourceSnapshot>>,
    pub freshness: Freshness,
    /// Monotonically increases on successful publications only.
    pub generation: u64,
    /// Elapsed time since the observation started; not a guarantee of validity.
    pub age: Option<Duration>,
}

impl CachedSnapshot {
    /// A stale, expired, or absent reading must not drive freshness-sensitive policy.
    pub fn fresh(&self) -> Option<&ResourceSnapshot> {
        if self.freshness == Freshness::Fresh {
            self.snapshot.as_deref()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshError {
    /// An invalidation or a newer refresh superseded this in-flight observation.
    Superseded,
    /// The process changed cgroup membership during the OS read window.
    Inconsistent,
}

#[derive(Debug)]
struct Inner {
    snapshot: Option<Arc<ResourceSnapshot>>,
    observed_at: Option<Instant>,
    generation: u64,
    epoch: u64,
    published_epoch: u64,
}

/// No filesystem reads on construction, cache inspection, or invalidation.
/// Only explicit `refresh()` invokes synchronous OS observation.
/// Readers take a short Mutex for Arc cloning; OS reads occur outside the lock.
#[derive(Debug)]
pub struct ResourceState {
    inner: Mutex<Inner>,
}

impl Default for ResourceState {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                snapshot: None,
                observed_at: None,
                generation: 0,
                epoch: 0,
                published_epoch: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // All critical sections only update simple fields and clone Arc, so
        // the internal invariant survives user panics outside the lock.
        self.inner.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn cached(&self) -> CachedSnapshot {
        self.view(None)
    }

    pub fn cached_with_max_age(&self, max_age: Duration) -> CachedSnapshot {
        self.view(Some(max_age))
    }

    fn view(&self, max_age: Option<Duration>) -> CachedSnapshot {
        let inner = self.lock();
        let age = inner.observed_at.map(|instant| instant.elapsed());
        let freshness = match (&inner.snapshot, age) {
            (None, _) => Freshness::Empty,
            (Some(_), _) if inner.epoch != inner.published_epoch => Freshness::Stale,
            (Some(_), Some(age)) if max_age.is_some_and(|limit| age > limit) => Freshness::Expired,
            (Some(_), _) => Freshness::Fresh,
        };
        CachedSnapshot {
            snapshot: inner.snapshot.clone(),
            freshness,
            generation: inner.generation,
            age,
        }
    }

    /// Mark all previously published readings stale, with no I/O.
    /// Does not delete historical evidence or falsely mark it as fresh.
    pub fn invalidate(&self) {
        let mut inner = self.lock();
        inner.epoch = inner.epoch.wrapping_add(1);
    }

    /// Publish an observation only if invalidation did not occur while it was
    /// collected, and no later-started observation has already been published.
    /// On error the last snapshot is preserved (stale after invalidation).
    pub fn refresh(&self) -> Result<CachedSnapshot, RefreshError> {
        self.refresh_with(snapshot)
    }

    fn refresh_with<F>(&self, observer: F) -> Result<CachedSnapshot, RefreshError>
    where
        F: FnOnce() -> ResourceSnapshot,
    {
        let epoch = self.lock().epoch;
        let started = Instant::now();
        let next = observer();
        if next.membership_changed_during_read {
            return Err(RefreshError::Inconsistent);
        }

        let mut inner = self.lock();
        if inner.epoch != epoch
            || inner.observed_at.is_some_and(|last| last > started)
        {
            return Err(RefreshError::Superseded);
        }

        let age = started.elapsed();
        let next = Arc::new(next);
        inner.generation = inner.generation.wrapping_add(1);
        inner.observed_at = Some(started);
        inner.published_epoch = epoch;
        inner.snapshot = Some(Arc::clone(&next));
        Ok(CachedSnapshot {
            snapshot: Some(next),
            freshness: Freshness::Fresh,
            generation: inner.generation,
            age: Some(age),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fixture() -> ResourceSnapshot {
        let mut value = snapshot();
        value.membership_changed_during_read = false;
        value
    }

    #[test]
    fn lifecycle_is_explicit_and_preserves_history() {
        let state = ResourceState::new();
        assert_eq!(state.cached().freshness, Freshness::Empty);
        assert_eq!(state.cached().generation, 0);
        let first = state.refresh_with(fixture).unwrap();
        assert_eq!(first.generation, 1);
        assert!(first.fresh().is_some());

        state.invalidate();
        let stale = state.cached();
        assert_eq!(stale.freshness, Freshness::Stale);
        assert_eq!(stale.generation, 1);
        assert!(stale.snapshot.is_some());
        assert!(stale.fresh().is_none());

        let second = state.refresh_with(fixture).unwrap();
        assert_eq!(second.generation, 2);
        assert_eq!(state.cached().freshness, Freshness::Fresh);
        assert_eq!(state.cached_with_max_age(Duration::ZERO).freshness, Freshness::Expired);
    }

    #[test]
    fn cached_and_invalidate_never_invoke_observer() {
        let state = ResourceState::new();
        let count = AtomicUsize::new(0);
        state.refresh_with(|| {
            count.fetch_add(1, Ordering::Relaxed);
            fixture()
        }).unwrap();
        for _ in 0..10_000 {
            assert!(state.cached().snapshot.is_some());
        }
        state.invalidate();
        assert_eq!(state.cached().freshness, Freshness::Stale);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn inconsistent_refresh_does_not_publish() {
        let state = ResourceState::new();
        state.refresh_with(fixture).unwrap();
        let prior = state.cached().generation;
        assert_eq!(state.refresh_with(|| {
            let mut v = fixture();
            v.membership_changed_during_read = true;
            v
        }).unwrap_err(), RefreshError::Inconsistent);
        assert_eq!(state.cached().generation, prior);
    }

    #[test]
    fn invalidation_during_refresh_rejects_publication() {
        use std::sync::Barrier;
        use std::thread;
        let state = Arc::new(ResourceState::new());
        state.refresh_with(fixture).unwrap();
        let entered = Arc::new(Barrier::new(2));
        let released = Arc::new(Barrier::new(2));
        let s = Arc::clone(&state);
        let e = Arc::clone(&entered);
        let r = Arc::clone(&released);
        let handle = thread::spawn(move || {
            s.refresh_with(|| {
                e.wait();
                r.wait();
                fixture()
            })
        });
        entered.wait();
        state.invalidate();
        released.wait();
        assert_eq!(handle.join().unwrap().unwrap_err(), RefreshError::Superseded);
        assert_eq!(state.cached().freshness, Freshness::Stale);
        assert_eq!(state.cached().generation, 1);
    }

    #[test]
    fn later_refresh_wins_out_of_order_completion() {
        use std::sync::Barrier;
        use std::thread;
        let state = Arc::new(ResourceState::new());
        let entered = Arc::new(Barrier::new(2));
        let released = Arc::new(Barrier::new(2));
        let s = Arc::clone(&state);
        let e = Arc::clone(&entered);
        let r = Arc::clone(&released);
        let slow = thread::spawn(move || s.refresh_with(|| {
            e.wait();
            r.wait();
            fixture()
        }));
        entered.wait();
        state.refresh_with(fixture).unwrap();
        released.wait();
        assert_eq!(slow.join().unwrap().unwrap_err(), RefreshError::Superseded);
        assert_eq!(state.cached().generation, 1);
    }

    #[test]
    fn concurrent_cache_and_invalidation_are_consistent() {
        use std::thread;
        let state = Arc::new(ResourceState::new());
        state.refresh_with(fixture).unwrap();
        let mut handles = Vec::new();
        for _ in 0..6 {
            let state = Arc::clone(&state);
            handles.push(thread::spawn(move || {
                for _ in 0..2_000 {
                    let current = state.cached();
                    assert_eq!(current.generation, 1);
                    if current.freshness == Freshness::Fresh {
                        assert!(current.fresh().is_some());
                    }
                }
            }));
        }
        state.invalidate();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(state.cached().freshness, Freshness::Stale);
    }
}
