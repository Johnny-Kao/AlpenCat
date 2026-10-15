# v0.5 One-Process Lazy/Eager/Static CPU Transition Batch

Date: 2026-10-15. Branch `research/v05-server-integrated-batch`.

## Executed evidence

- [Run #38748692303](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38748692303), tested SHA `3d10077767840acd4df3f68371dd6136c1abc574`: **4/4 jobs successful** (Ubuntu 22.04/24.04 × two independent allocations).
- Single live Rust process per job; Python supervisor externally changes actual allowed CPU affinity **4 → 2 → 1 → 2 → 4** without root, restarting or changing machine-wide resources.
- Three long-lived independent SDK runtimes with the same workload: `Static` (published initial boundary, never recalibrated); `Eager` (explicit revalidation on every transition); `Signal Lazy` (only when effective schedulable CPU count differs from previous phase). Also records fixed Serial and fixed CPU execution as route references; asserts output vector equivalence.
- Each phase makes repeated calls across four sizes. Wall elapsed inside execute and elapsed calibration are captured separately. All runs completed and produced archived JSON artifacts.

## Measured results — summed wall-clock times across 5 resource phases

| Runner | Static | Eager | Signal Lazy | Eager vs Static | Signal Lazy vs Static |
|---|---:|---:|---:|---:|---:|
| Ubuntu 22.04 #1 | 448.371 ms | 462.792 ms | 468.469 ms | +3.22% | +4.48% |
| Ubuntu 22.04 #2 | 378.860 ms | 382.337 ms | 387.352 ms | +0.92% | +2.24% |
| Ubuntu 24.04 #1 | 394.624 ms | 406.846 ms | 418.997 ms | +3.10% | +6.18% |
| Ubuntu 24.04 #2 | 346.457 ms | 358.659 ms | 366.847 ms | +3.52% | +5.89% |

**In these four test allocations, Static won all four.** Every phase changed effective CPU count, so `Signal Lazy` revalidated every phase and did not demonstrate economically gated skipping. It is a *change-triggered baseline*, **not** the hypothesized economic Lazy gate. Crucially, no signal provides enough evidence that changed CPU count by itself implies positive optimization value.

## Interpretation and limits

1. The benchmark uses an artificial CPU kernel and routes are always visited in the same order within a phase; host noise, thermal effects, runner allocation, and order bias mean these numbers are **not** publishable production performance estimates.
2. Summed wall-clock excludes some control and Python supervisor bookkeeping; includes synchronous SDK calibration only. No process CPU-time, process peak RSS or calibrated decision cost is attributed separately to each strategy in this batch. Therefore **CPU-hours saved remains unknown**.
3. `Static` is an existing Auto runtime with unchanging boundary, **not** forced Serial. `Eager` and `Signal Lazy` are explicitly invoked by the test harness; SDK does not automatically infer cgroup/CPU changes.
4. This run strongly undermines the assumption that *every available-CPU change* should trigger revalidation. It **does not prove** that a selective expected-gain-gated Lazy policy can never win, but no evidence justifies implementing such a policy yet.
5. No Intel/AMD-specific or cross-L3 hardware claim. No Runtime Core changes.

## Decision

**Method/SDK integration PASS. Optimization GO: NO. Research ITERATE toward a limited economically gated design, only if a workload demonstrates genuine recoverable stale-route regret that exceeds probing and switching costs.** If no such workload appears under available hardware, STOP this optimization track rather than endlessly varying thresholds.

Future measurement requires counterbalanced policy order, full process CPU-time and RSS, and explicit idle/no-op resource events to distinguish `Lazy` from unconditional revalidation. Do not start another broad CI matrix without a materially new hypothesis.
