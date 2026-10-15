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

## Completed integrated execution (one batch)

Run [#38746182786](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38746182786), tested SHA `229b6332da78402b86ffb1560ae191b1c5b80425`. **4/4 success:** Ubuntu 22.04 ×2, Ubuntu 24.04 ×2. Each runner ran both counterbalanced passes, three footprints, effective two-CPU same-L3 case and a one-CPU negative control, with 15 native pthread samples each. All checksum and syntax gates passed; raw manifest, measurements, comparisons and decision artifacts were uploaded.

Across all four allocations, `CROSS_L3_UNAVAILABLE` / `BLOCKED_NO_DISTINCT_L3`. Therefore no proof of cross-CCD, Intel Mesh, or NUMA locality gains; **scientific optimization GO is not supported by this environment**.

Representative 32-MiB footprint second-pass wall / process CPU medians:
- Ubuntu 22.04 allocation 1: one CPU 1197.6 / 1186.4 µs; two same-L3 CPUs 611.4 / 1159.5 µs.
- Ubuntu 22.04 allocation 2: one CPU 537.7 / 534.8 µs; two same-L3 CPUs 292.1 / 523.9 µs.
- Ubuntu 24.04 allocation 1: one CPU 1372.2 / 1367.8 µs; two same-L3 CPUs 690.0 / 1306.5 µs.
- Ubuntu 24.04 allocation 2: one CPU 1080.6 / 1067.9 µs; two same-L3 CPUs 343.6 / 648.1 µs.

These measurements show that two permitted CPUs can improve elapsed latency for this narrow pthread memory read while process CPU time need not improve comparably. This **does not** demonstrate a Lazy SDK improvement, server CPU-hour savings, or a locality advantage. The benchmark still creates threads inside measured windows and leaves memory page placement uncontrolled.

**Final decision:** Harness/instrumentation GO; multi-L3 physical-hardware gate BLOCKED; platform optimizer GO/STOP UNDETERMINED. Do not tune a policy from this evidence. Next valid hardware run should be a single comparable full-batch campaign with multiple accessible L3/cache domains, worker reuse, explicit NUMA-page policy and Static/Eager/Lazy accounting; no repeated general GitHub-hosted matrix will resolve that missing hardware capability.
