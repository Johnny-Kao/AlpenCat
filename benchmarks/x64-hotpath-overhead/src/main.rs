use std::hint::black_box;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

const ITERS: u64 = 50_000_000;
const REPS: usize = 9;

#[inline(always)]
fn baseline_route(n: usize, boundary: usize) -> bool {
    n >= boundary
}

#[inline(always)]
fn epoch_route(n: usize, boundary: usize, global_epoch: &AtomicU64, local_epoch: u64) -> bool {
    let stale = global_epoch.load(Ordering::Relaxed) != local_epoch;
    black_box(stale);
    n >= boundary
}

#[inline(always)]
fn stale_far_route(
    n: usize,
    boundary: usize,
    global_epoch: &AtomicU64,
    local_epoch: u64,
) -> bool {
    let stale = global_epoch.load(Ordering::Relaxed) != local_epoch;
    if stale {
        let distance = n.abs_diff(boundary);
        black_box(distance <= boundary / 4);
    }
    n >= boundary
}

#[inline(always)]
fn stale_near_route(
    n: usize,
    boundary: usize,
    global_epoch: &AtomicU64,
    local_epoch: u64,
    demand: &AtomicU32,
) -> bool {
    let stale = global_epoch.load(Ordering::Relaxed) != local_epoch;
    if stale {
        let distance = n.abs_diff(boundary);
        if distance <= boundary / 4 {
            demand.fetch_add(1, Ordering::Relaxed);
        }
    }
    n >= boundary
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
    let boundary = black_box(16_384usize);
    let near_n = black_box(16_000usize);
    let far_n = black_box(2_048usize);
    let epoch = AtomicU64::new(7);
    let demand = AtomicU32::new(0);

    let mut baseline = Vec::with_capacity(REPS);
    let mut epoch_match = Vec::with_capacity(REPS);
    let mut stale_far = Vec::with_capacity(REPS);
    let mut stale_near = Vec::with_capacity(REPS);

    for _ in 0..REPS {
        baseline.push(measure(|i| baseline_route(black_box(near_n + ((i as usize) & 1)), boundary)));
        epoch_match.push(measure(|i| epoch_route(black_box(near_n + ((i as usize) & 1)), boundary, &epoch, 7)));
        stale_far.push(measure(|i| stale_far_route(black_box(far_n + ((i as usize) & 1)), boundary, &epoch, 6)));
        stale_near.push(measure(|i| stale_near_route(black_box(near_n + ((i as usize) & 1)), boundary, &epoch, 6, &demand)));
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
