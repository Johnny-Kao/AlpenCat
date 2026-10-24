//! Independent application owns the workload policy: AlpenCat only provides inputs.
use alpencat_resource::{Reading, Freshness, ResourceState};

fn choose_chunk(state: &ResourceState) -> usize {
    let view = state.cached();
    if view.fresh().is_none() {
        // No unsafe assumption based on expired or invalidated readings.
        return 128;
    }
    let allowed = match &view.fresh().unwrap().allowed_cpus {
        Reading::Known(cpus) => cpus.len().max(1),
        _ => 1,
    };
    let quota = match &view.fresh().unwrap().effective_cpu_capacity {
        Reading::Known(capacity) if capacity.is_finite() && *capacity > 0.0 =>
            (capacity.floor() as usize).max(1),
        _ => allowed,
    };
    128 * allowed.min(quota).min(8)
}

fn workload(chunk: usize) -> u64 {
    let values: Vec<u64> = (0..10_000_u64).collect();
    values.chunks(chunk).map(|group| {
        group.iter().fold(0_u64, |acc, n| acc.wrapping_add(n.wrapping_mul(7)))
    }).fold(0, u64::wrapping_add)
}

fn main() {
    let state = ResourceState::new();
    assert_eq!(state.cached().freshness, Freshness::Empty);
    let initial = state.refresh().expect("consistent initial resource observation");
    assert!(initial.fresh().is_some());
    let chunk = choose_chunk(&state);
    let expected = workload(128);
    assert_eq!(workload(chunk), expected);

    // The application, not AlpenCat, notices an external resource event.
    state.invalidate();
    assert_eq!(state.cached().freshness, Freshness::Stale);
    assert!(state.cached().fresh().is_none());
    assert_eq!(workload(choose_chunk(&state)), expected); // safe fallback

    state.refresh().expect("consistent observation after invalidation");
    assert_eq!(state.cached().freshness, Freshness::Fresh);
    assert_eq!(workload(choose_chunk(&state)), expected);
    println!("LIFECYCLE_CONSUMER=PASS generation={} chunk={}", state.cached().generation, chunk);
}
