# Server CPU Integrated Batch Gate — 2026-10-15

This isolated branch exists to avoid triggering the older, broad, unrelated economics screening on each research commit. It is based on `research/v0.5-adaptation-economics`. The batch is intentionally a **hardware evidence gate**, not a CPU optimization or a release PR.

## Design

- Linux sysfs L3/cache and NUMA topology, effective cpuset, cgroup CPU quota, PSI and NUMA distance inventory.
- Native `pthread` memory-access workload, checksummed and measured with wall + **process CPU time**.
- CPU affinity cases: same-L3 two-core, distinct-L3 two-core when available, and a one-CPU negative control. No privileged modification.
- Working-set footprints: 128 KiB, 2 MiB, and 32 MiB. Two reversed-order passes and 15 repeats per case.
- Four different Linux runner allocations in one GitHub Actions workflow (two OS images times two independent samples); reports kept as JSON artifacts.
- Cross-L3 absent: report `BLOCKED_NO_DISTINCT_L3`, not a negative speedup result.
- Cross-L3 present: compare sign consistency and magnitudes, **not** assume an effect or fit a policy.
- Only one workflow run after code + documentation is assembled; reserve re-runs for actual failures.

## Interpretation

The native example includes thread startup inside each trial and relies on Linux first-touch placement rather than explicitly controlling NUMA memory allocation. Therefore it supports instrumentation/feasibility, not a valid claim of steady-state locality gains, CPU-hour savings or Lazy policy improvements. Before attempting production routing, physical multi-L3 / multi-NUMA server evidence must support warmed persistent workers, controlled page placement, and Static/Periodic/Lazy net economic comparisons.

## Gates

1. CI compilation and output checksum: PASS/FAIL.
2. Runner exposes distinct allowed L3 domains: AVAILABLE/BLOCKED.
3. Stable directional locality performance across independent allocations: SUPPORTED/INCONCLUSIVE.
4. Opportunity sufficiently exceeds measurement and adaptation costs: GO/ITERATE/STOP, only once gate 3 is supported.

Do not overwrite `main`, v0.4, or frozen Generic holdout decisions.
