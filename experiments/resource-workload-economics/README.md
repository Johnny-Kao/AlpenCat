# Resource Lifecycle — actual workload CPU/Wall economics

**Experimental, not a product performance claim.** Runs an actual deterministic streamed integer-processing application. Every mode uses equivalent work and output. Each scenario performs thousands of batches of nontrivial integer computation with caller-owned chunk grain.

## Compared modes

1. `static`: no snapshots inside workload, chooses a grain once via the standard CPU parallelism API. This is a negative control and legitimate efficient baseline.
2. `per_chunk`: obtains full OS-backed `snapshot()` before every batch. This is a deliberately expensive but straightforward always-fresh-observation implementation, not the best alternative.
3. `periodic`: reads full `snapshot()` only every 128 batches, retaining the most recent policy. This is a realistic, efficient, no-cache-library comparison.
4. `cached`: performs initial `ResourceState::refresh()`, a no-OS-read `cached()` before each batch, and explicitly `invalidate()+refresh()` after each 128 batches. Invalidation points are **synthetic caller events**, not actual OS/cgroup capacity changes.

Modes 2–4 use the same process affinity and *visible* CPU quota to choose chunk grain. They calculate the same workload checksum as `static` even when grain differs. This is not a concurrent parallelism-control experiment and does not establish gains from better routing.

## Paired run

`cargo build --release --manifest-path experiments/resource-workload-economics/Cargo.toml`

`python3 experiments/resource-workload-economics/run_paired.py --binary experiments/resource-workload-economics/target/release/alpencat-workload-economics`

Linux `resource.getrusage(RUSAGE_CHILDREN)` obtains per-child **user+system CPU seconds**; Rust `Instant` inside the workload reports Wall Time excluding startup. Each of two workloads has 7 rounds with shuffled mode order and initial warmup. Paired comparisons use observations recorded on the same runner and round. Output raw CSV and JSON artifacts.

Positive evidence criterion (predeclared):
- For each baseline and each metric, cached must improve by >2% in at least 6/7 paired rounds (same runner). We report all median gains and per-round values, even when negative.
- `robust_net_workload_gain` requires **both** Wall Time and CPU Time to improve against **both** sensible periodic observation and tuned static.
- Winning only versus naive per-chunk OS reads is **not** sufficient to support broad application CPU/Wall gain.
- Linux Ubuntu 22.04 and 24.04 are independent environment checks, but two ephemeral runner types do not establish all-hardware stability.
- A failed economic condition is a **NO_GO outcome**, not an excuse to rewrite a favorable benchmark until green. CI fails for invalid output, crashes, or tooling defects, not negative economic outcomes.

## Constraints

These workloads isolate resource-observation overhead, not workload-dependent optimization policy. Static and periodic baselines can be faster. Because the cgroup state is not deliberately changed, a cache cannot by itself generate savings from improved scheduling/adaptation. Controlled real resource transitions and genuinely parallel applications would be separate evidence gates **only if** this experiment demonstrates a case worth pursuing.

No main branch, published release, external compute accounts, paid runners, or confidential product plans are involved.
