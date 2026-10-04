use std::hint::black_box;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use runtime_core::{BoundaryProfile, BoundarySnapshot, PublishedBoundary, ResourceEpoch};
use runtime_selector::select;

const ITERS: u64 = 50_000_000;
const REPS: usize = 9;

#[inline(always)]
fn baseline_route(n: usize, boundary: BoundaryProfile) -> bool {
    matches!(select(n, false, boundary), runtime_core::BackendKind::Cpu)
}

#[inline(always)]
fn production_route(
    n: usize,
    boundary: &PublishedBoundary,
    epoch: &ResourceEpoch,
) -> bool {
    let snapshot = boundary.snapshot();
    black_box(epoch.is_stale(snapshot.resource_epoch));
    matches!(select(n, false, snapshot.profile), runtime_core::BackendKind::Cpu)
}

#[inline(always)]
fn stale_far_route(
    n: usize,
    boundary: &PublishedBoundary,
    epoch: &ResourceEpoch,
) -> bool {
    let snapshot = boundary.snapshot();
    if epoch.is_stale(snapshot.resource_epoch) {
        let distance = n.abs_diff(snapshot.profile.serial_max_items);
        black_box(distance <= snapshot.profile.serial_max_items / 4);
    }
    matches!(select(n, false, snapshot.profile), runtime_core::BackendKind::Cpu)
}

#[inline(always)]
fn stale_near_route(
    n: usize,
    boundary: &PublishedBoundary,
    epoch: &ResourceEpoch,
    demand: &AtomicU32,
) -> bool {
    let snapshot = boundary.snapshot();
    if epoch.is_stale(snapshot.resource_epoch) {
        let distance = n.abs_diff(snapshot.profile.serial_max_items);
        if distance <= snapshot.profile.serial_max_items / 4 {
            demand.fetch_add(1, Ordering::Relaxed);
        }
    }
    matches!(select(n, false, snapshot.profile), runtime_core::BackendKind::Cpu)
}

fn measure<F: FnMut(u64) -> bool>(mut f: F) -> f64 {
    let start = Instant::now();
    let mut sink = false;
    for i in 0..ITERS {
        sink ^= black_box(f(i));
    }
    black_box(sink);
    start.elapsed().as_nanos() as f64 / ITERS as f64
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.total_cmp(b));
    xs[xs.len() / 2]
}

fn main() {
    let profile = BoundaryProfile::new(16_384, usize::MAX);
    let near_n = black_box(16_000usize);
    let far_n = black_box(2_048usize);
    let epoch = ResourceEpoch::new();
    let boundary = PublishedBoundary::new(profile, epoch.current());
    let demand = AtomicU32::new(0);

    let mut baseline = Vec::with_capacity(REPS);
    let mut epoch_match = Vec::with_capacity(REPS);
    let mut stale_far = Vec::with_capacity(REPS);
    let mut stale_near = Vec::with_capacity(REPS);

    for _ in 0..REPS {
        baseline.push(measure(|i| {
            baseline_route(black_box(near_n + ((i as usize) & 1)), profile)
        }));
        epoch_match.push(measure(|i| {
            production_route(
                black_box(near_n + ((i as usize) & 1)),
                &boundary,
                &epoch,
            )
        }));

        epoch.invalidate();
        stale_far.push(measure(|i| {
            stale_far_route(
                black_box(far_n + ((i as usize) & 1)),
                &boundary,
                &epoch,
            )
        }));
        stale_near.push(measure(|i| {
            stale_near_route(
                black_box(near_n + ((i as usize) & 1)),
                &boundary,
                &epoch,
                &demand,
            )
        }));
        boundary.publish(BoundarySnapshot::new(profile, epoch.current()));
    }

    let b = median(baseline);
    let e = median(epoch_match);
    let f = median(stale_far);
    let n = median(stale_near);

    println!("case,ns_per_call,delta_ns,overhead_pct");
    for (name, value) in [
        ("baseline", b),
        ("epoch_match", e),
        ("stale_far", f),
        ("stale_near", n),
    ] {
        let delta = value - b;
        let pct = if b > 0.0 { 100.0 * delta / b } else { 0.0 };
        println!("{},{:.6},{:.6},{:.3}", name, value, delta, pct);
    }
    println!("demand_counter={}", demand.load(Ordering::Relaxed));
}
