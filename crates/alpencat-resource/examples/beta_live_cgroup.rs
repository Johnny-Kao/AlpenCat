//! Linux release-candidate field smoke for resource validity under an owned cgroup.
//! Launched only by a disposable GitHub Actions transient systemd service.
#[cfg(target_os = "linux")]
fn main() {
    use alpencat_resource::{snapshot, Freshness, Reading, ResourceState};
    let capacity = |value: &alpencat_resource::ResourceSnapshot| match &value.effective_cpu_capacity
    {
        Reading::Known(capacity) => *capacity,
        ref other => panic!("resource quota unavailable despite real cpu.max: {other:?}"),
    };
    let direct = snapshot();
    assert!(direct.cgroup_resolved, "must discover live cgroup2");
    let q = capacity(&direct);
    assert!(
        (q - 0.5).abs() <= 0.001,
        "live half-CPU quota should be 0.5, got {q}"
    );
    let state = ResourceState::new();
    assert_eq!(state.cached().freshness, Freshness::Empty);
    let new = state.refresh().expect("consistent cgroup observation");
    assert_eq!(new.freshness, Freshness::Fresh);
    assert!((capacity(new.fresh().expect("fresh resource")) - 0.5).abs() <= 0.001);
    state.invalidate();
    assert_eq!(state.cached().freshness, Freshness::Stale);
    assert!(
        state.cached().fresh().is_none(),
        "stale snapshot must not be fresh"
    );
    let again = state
        .refresh()
        .expect("refresh after explicit invalidation");
    assert_eq!(again.freshness, Freshness::Fresh);
    assert_eq!(again.generation, 2);
    println!(
        "RESOURCE_BETA_LIVE_CGROUP_PASS quota={q} generation={}",
        again.generation
    );
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("RESOURCE_BETA_LIVE_CGROUP_UNSUPPORTED: linux only");
}
