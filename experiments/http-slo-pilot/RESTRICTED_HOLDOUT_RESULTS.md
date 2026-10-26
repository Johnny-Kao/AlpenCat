# Prospective 0.5-CPU mixed-traffic holdout — Results (2026-10-26)

**Engineering integrity: PASS (both Linux runners). Predeclared conditional product GO: FALSE.**

This holdout was preregistered in [RESTRICTED_HOLDOUT_PLAN.md](RESTRICTED_HOLDOUT_PLAN.md) before execution.

- [GitHub Actions #40344702688](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40344702688): Ubuntu 22.04 and 24.04 both SUCCESS at exact tested commit `babe7b591aefccb8f512bdf362e921de5709d01b`.
- Raw per-phase/per-round HTTP p95/p99, CPU, throughput/wall, `cpu.stat` throttling deltas, and response SHA digests are available in two workflow artifacts.
- An OSS `tiny_http` reference server received **real TCP requests** with 16 concurrent clients. A dedicated transient Cgroup v2 service experienced actual verified `cpu.max` transitions **2 → 0.5 → 2 equivalent CPUs**. During the middle stage, `nr_throttled` increased. Responses matched across all policy arms.
- This run uses independently varied *request cost mixtures* from the earlier exploratory HTTP batch study and **five randomized paired rounds per case/runner**. It is **not** a different external production service.
- The primary comparison measures genuine end-to-end HTTP **p95 in the 0.5 CPU middle phase**, not sums of three phase p95 or synthetic request latencies.

## Predeclared pass rules

For `alpencat_guard` (preselected 4→2→4 active workers) versus `fixed8` on the 0.5-CPU phase: HTTP p95 improvement **>5% in at least 4/5 paired rounds**, process CPU-Time regression ≤5% at paired median, and HTTP batch Wall-Time regression ≤5% at paired median. Require parity (median absolute difference within 5% for p95 and CPU) versus `direct_guard`, which uses the **same concurrency rule with direct OS snapshot reads**, to avoid attributing policy gains to the SDK observation method. Both request mix cases must pass on both Ubuntu versions.

## Measured comparisons: AlpenCat guard relative to fixed 8 workers

Positive percent means lower HTTP p95, CPU Time, or HTTP batch Wall Time.

| Runner / heldout case | HTTP p95 improvement | p95 >5% rounds | CPU-Time gain | HTTP Wall-Time gain | Predeclared case GO |
|---|---:|---:|---:|---:|---|
| Ubuntu 22.04 / mixed | **+32.32%** | **5/5** | -0.89% | +0.73% | **PASS** |
| Ubuntu 22.04 / weighted | **+35.08%** | **5/5** | -1.44% | -0.52% | **PASS** |
| Ubuntu 24.04 / mixed | **+7.01%** | **4/5** | -0.90% | +0.30% | **PASS** |
| Ubuntu 24.04 / weighted | +4.63% | **1/5** | -3.17% | -1.31% | **FAIL** |

The **identical direct-OS policy matched AlpenCat's p95 and process-CPU behavior within the 5% median parity budget in all four cases**. This parity is evidence against a unique application-CPU speedup attributable to the ResourceState implementation alone.

## Actual restricted-stage p95 medians (milliseconds)

| Runner / case | Fixed 8 | Fixed 4 | Fixed 2 | Direct guard | AlpenCat guard |
|---|---:|---:|---:|---:|---:|
| Ubuntu 22.04 / mixed | 146.57 | 100.93 | 99.75 | 99.71 | **99.54** |
| Ubuntu 22.04 / weighted | 152.02 | 100.58 | 98.67 | **98.47** | 98.59 |
| Ubuntu 24.04 / mixed | 103.74 | 98.90 | **95.98** | 96.26 | 96.43 |
| Ubuntu 24.04 / weighted | 101.41 | 98.90 | **96.00** | 95.86 | 98.98 |

Note that **fixed 2 workers are nearly as good or better** than the guard on p95 in these specific constrained phases. The original 2→0.5→2 experiment also included normal-capacity phases and their cost; the predeclared holdout evaluates the restricted phase only. Any deployment must verify the whole traffic lifecycle, not just p95 under throttling.

## Absolute research SLO checks

The holdout declared restricted p95 targets of **160ms (mixed)** and **220ms (weighted)**. On Ubuntu 22.04 mixed, fixed8 met the 160ms target 3/5 times while AlpenCat guard met it 5/5. All arms met the larger weighted threshold in all 5 rounds on both platforms; those thresholds are **not sufficiently discriminating** in most scenarios.

## Engineering / release decision

- **PASS:** real Cgroup CPU quota 2→0.5→2, nonzero throttling, independent HTTP server and actual HTTP response timing, data correctness, reproducible four-case evaluation, all CPU/Wall cost accounting.
- **Three out of four cases show a bounded p95/CPU/throughput tradeoff relative to fixed8; fourth case fails the preregistered p95 threshold.** Therefore the overall conditional **GO is FALSE** and we do not ship an automatic policy or claim universal stable acceleration.
- Direct-OS implementation with the same policy performs similarly. Fixed2 can equal/exceed guard under 0.5 CPU, so p95 gains vs fixed8 alone **do not prove a need for auto-adaptation**.
- The useful narrow insight is **avoiding excessive concurrency under some real, sustained CPU quotas reduces HTTP tail latency at modest CPU/Wall cost**, but the best setting is workload/platform/SLO-dependent.
- Next product-oriented step, if justified by a real integration: expose resource evidence + fresh/stale semantics, and let the application opt into measured, SLO-specific policy. Avoid per-event exhaustive search, do not modify `main` or create a release from this experiment.

