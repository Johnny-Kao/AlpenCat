# Convergence Gate 1 — measured result (2026-10-27)

**Actual CPU time scientific GO: NO-GO on the preregistered two-workload matrix.**  
**Experiment-integrity CI: PASS, both Linux systems.**

- [Verified GitHub Actions #40524645362](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40524645362), exact tested SHA `32f904560a6843ff26c4eee6627479b958db73a9`.
- Ubuntu 22.04 and Ubuntu 24.04 jobs both succeeded; each has raw per-trial CSV/JSON artifacts with CPU and Wall Time, backend availability and actual cgroup throttling.
- Five trials per Linux OS; within each, a **single** live process experienced real `cpu.max` changes **2 → 1.5 → 0.5 → 2 CPU**. At 0.5 CPU, nonzero `nr_throttled` was observed.
- Both families use `runtime-api::Runtime::submit_map` exactly as implemented in main, not a substitute executor. Serial and CPU outputs matched at each sample; 7 alternating paired measurements after warmups for each size and phase.
- Economic upper bound uses an optimistic hindsight oracle and a generous per-workload-size best fixed API route, ignoring all observation/revalidation costs. Its computed savings must be understood as an *upper bound for measured API routes*, subject to benchmark noise, not a product gain.

## Aggregate optimistic gross CPU-time savings (median over five trials)

| Family | Ubuntu 22.04 | Ubuntu 24.04 | Preset GO bar |
|---|---:|---:|---:|
| CPU-heavy 512-round integer transform | **0.000%** | **0.065%** | >1% for ≥4/5 trials |
| Irregular memory reads | **0.201%** | **0.050%** | >1% for ≥4/5 trials |

Corresponding optimistic Wall-Time regret (not CPU-hour savings):

| Family | Ubuntu 22.04 | Ubuntu 24.04 |
|---|---:|---:|
| CPU-heavy | 0.382% | 0.349% |
| Memory reads | 2.367% | 0.997% |

**No robust CPU-route crossover** for the predefined sizes was detected in either family on either OS. CPU-time savings fall short of the predeclared original-convergence economic threshold.

## Engineering finding, not to over-generalize

At 2 CPU effective quota, the existing `runtime-cpu-rayon` worker path reported `Cpu`. **At both 1.5 and 0.5 CPU effective quota, requesting `ExecutionMode::Cpu` fell back to `Serial` in every trial on both runners.** This follows current execution-budget/available-parallelism behavior; lower-quota phases therefore **did not experimentally compare two genuinely different running routes**. The results cannot prove an independent two-thread CPU executor would lack an opportunity under fractional quota. The existing public API already performs fallback, so the measured API's recoverable regret is small in this matrix.

This does not prove every workload or architecture has no potential. It does establish a proper STOP for *unbounded generic compute-adaptation work on these two preselected families and actual existing routes*. Research and product-readiness decisions must not confuse a successful Linux CI with economic GO.

## Decision and follow-through

1. Gate 1 **NO-GO** for measured generic CPU-compute-savings thesis; stop adding new auto-policy variants under the present campaign.
2. Do **not** implement Gate 2 revalidation-loop policy for these workload families, because the optimistic (free) CPU oracle already fails the >1% gate.
3. Continue the independently valuable **Resource Intelligence SDK** release-hardening path, particularly correct Linux Cgroup semantics, ResourceState validity, strict formatting, external consumer and release packaging. Completion of that product should not be blocked by speculative universal compute acceleration.
4. Keep original runtime functionality separately supported as experimental; any future change to fractional-quota Rayon thread choice requires a new explicit hypothesis and is **not** silently treated as the currently tested original API.

No `main`, existing release tags or production defaults were modified.
