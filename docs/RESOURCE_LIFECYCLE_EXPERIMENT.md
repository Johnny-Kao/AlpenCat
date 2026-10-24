# Resource Lifecycle — experimental API contract

This is an engineering experiment on `research/resource-lifecycle-beta`. It is **not** part of the released `v0.5.0-alpha` tag or a Beta announcement.

## Intended use

```rust
use alpencat_resource::{ResourceState, Freshness};
use std::time::Duration;

let state = ResourceState::new(); // no I/O
assert_eq!(state.cached().freshness, Freshness::Empty);
state.refresh().expect("consistent observation"); // reads OS
let view = state.cached_with_max_age(Duration::from_secs(2)); // no filesystem I/O
if let Some(snapshot) = view.fresh() {
    println!("{:?}", snapshot.allowed_cpus);
}
state.invalidate(); // caller knows resources may have changed
assert_eq!(state.cached().freshness, Freshness::Stale);
state.refresh().expect("consistent observation");
```

For an independent application exercising a caller-owned chunk-size policy, see `experiments/resource-lifecycle-consumer/`. The caller retains complete responsibility for safe execution and semantic equivalence. AlpenCat does not choose/execute workloads.

## Semantic contract

- `cached()`: returns an `Arc<ResourceSnapshot>` and status without OS file reads. It is Fresh only **relative to application-supplied invalidations** and the last successful refresh. Fresh cannot certify live host resources remained unchanged.
- `cached_with_max_age(max_age)`: additionally returns Expired once the observation age exceeds the caller's bound (based on monotonic `Instant`). Use this if the application has no reliable resource-change signals.
- `invalidate()`: never performs I/O; immediately marks cached data stale while preserving last-known observation.
- `refresh()`: explicitly performs a synchronous resource read outside the cache mutex. It refuses to publish if another invalidation occurred or a later-started refresh already published. On detected cgroup-membership race it marks history stale and returns Inconsistent.
- A successful refresh publishes one immutable shared snapshot. At most one snapshot is held *by ResourceState*. Callers retaining old `Arc` handles own their memory.
- Unknown or Unsupported fields remain Unknown/Unsupported inside the snapshot. They are never implicitly inferred as zero.
- `generation` changes only on successful publication; it measures refresh publications, **not physical resource changes**. Changes in generation must not be interpreted as capacity improvement.
- The Snapshot is still non-atomic across kernel interfaces. Membership re-read only detects some races. Hidden cgroup ancestors, invalid mount contexts and platform restrictions remain limitations of v0.5's observation adapter.
- The Mutex can briefly block on other cache readers, but no OS reads take place while it is held. No worker thread or polling timer is created.

## Bounded performance experiment

The CI job uses repeated **paired** complete `snapshot()` and cached `ResourceState::cached()` lookups on the same GitHub runner, 7 batches of 300 full snapshots and 30,000 cached lookups each. It measures median nanoseconds per lookup, reports the ratio, and enforces **at least 10x lower observation-cost** for cached reads on that measured Linux runner. This is not a claim of application throughput gain, cloud invoice savings or performance across all machines.

Tests also verify: empty → fresh → stale → refreshed, max-age expiry, zero observer calls on cached lookup, concurrent readers, invalidation during in-flight refresh, out-of-order publication rejection, inconsistent membership fails closed, and an independent Consumer with equivalent output under changed application-owned chunk-size policy.

## Remaining before Beta release

CI success demonstrates lifecycle correctness in the tested environments, not automatic resource-change discovery. Before any Beta release, audit the existing Linux scope semantics for tricky cgroup namespaces and document whether the polling-free refresh semantics suit pilot applications. This experiment does **not** modify existing releases or public release tags.
