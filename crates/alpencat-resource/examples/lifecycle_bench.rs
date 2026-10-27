//! Bounded, paired Linux experimental measurements, not a universal SLA.
use alpencat_resource::{snapshot, Freshness, ResourceState};
use std::{hint::black_box, time::Instant};

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

fn main() {
    const BATCHES: usize = 7;
    const SNAPSHOTS: usize = 300;
    const CACHED: usize = 30_000;
    let state = ResourceState::new();
    state.refresh().expect("consistent initial snapshot");
    let mut full = Vec::with_capacity(BATCHES);
    let mut cached = Vec::with_capacity(BATCHES);
    for _ in 0..BATCHES {
        // Each pair runs on the same runner; repeat to reduce scheduling noise.
        let started = Instant::now();
        for _ in 0..SNAPSHOTS {
            black_box(snapshot());
        }
        full.push(started.elapsed().as_nanos() as f64 / SNAPSHOTS as f64);

        let started = Instant::now();
        for _ in 0..CACHED {
            let view = black_box(state.cached());
            assert_eq!(view.freshness, Freshness::Fresh);
            black_box(view.fresh());
        }
        cached.push(started.elapsed().as_nanos() as f64 / CACHED as f64);
    }

    let full_median = median(&mut full);
    let cached_median = median(&mut cached);
    let ratio = full_median / cached_median;
    state.invalidate();
    assert_eq!(state.cached().freshness, Freshness::Stale);
    let refreshed = state.refresh().expect("refresh after invalidation");
    assert_eq!(refreshed.freshness, Freshness::Fresh);
    println!("LIFECYCLE_METRICS full_snapshot_median_ns={full_median:.1} cached_median_ns={cached_median:.1} speedup_x={ratio:.2} batches={BATCHES} snapshots_per_batch={SNAPSHOTS} cached_per_batch={CACHED} max_history=1 background_workers=0");
    // A stable reduction in *observation overhead* is the hypothesis, not faster workload execution.
    assert!(
        ratio >= 10.0,
        "minimum observation-cost gain of 10x not met: {ratio:.2}x"
    );
}
