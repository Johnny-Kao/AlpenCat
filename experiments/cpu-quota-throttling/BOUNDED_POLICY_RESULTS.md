# Bounded Policy Experiment — Result (2026-10-25)

**Experiment correctness: PASS. Stable positive compute economics: NO-GO.**

Exact tested code commit: `acf76db58940c6c6f460b0a252eb84135b5fee28`.

[GitHub Actions #40165140461](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40165140461): Ubuntu 22.04 and Ubuntu 24.04 both completed successfully. Per-run JSON and CSV are attached as workflow artifacts.

## Method
Actual systemd Cgroup v2 CPU quota transitions 2 → 0.5 → 2 equivalent CPUs, confirmed by `cpu.max`, with nonzero throttling periods in the restricted phase. Two deterministic CPU-bound queued workloads, five paired random-order repetitions each, fixed1/2/4/8, quota-only direct/AlpenCat, bounded-policy direct/AlpenCat. All versions complete identical request indices and produce the same checksum.

Each phase has 12 windows of real requests. Bounded search uses eight exploratory windows (two per candidate), then four exploitation windows. No exploratory compute time is excluded from Wall/CPU cost. Hysteresis is 5% and a one-time rollback threshold is 25%. A fixed policy and direct-OS same-policy baseline are retained.

**Important metric distinction:** The p95 reported below is the **sum of the three phase-specific request-completion p95 values**, where each request starts at the enqueue instant of its own window. It is **not** HTTP p95, a percentile across all phases or a measurement comparable to the previous single-phase saturated-queue experiment's absolute p95 figures.

## Ubuntu 24.04 — long scenario medians

| Policy | Wall ms | CPU ms | Sum phase p95 ms |
|---|---:|---:|---:|
| Fixed 1 | 3718.4 | 2835.7 | 324.1 |
| Fixed 2 | 2830.9 | 2846.1 | 265.4 |
| Fixed 4 | 2755.3 | 2921.9 | 324.0 |
| Fixed 8 | **2702.4** | 2930.0 | 326.9 |
| Direct simple quota | 2795.5 | **2838.8** | 251.5 |
| AlpenCat simple quota | 2763.8 | 2839.4 | **251.4** |
| Direct bounded search | 2852.5 | 2898.9 | 329.5 |
| AlpenCat bounded search | 2889.7 | 2899.4 | 333.1 |

## Ubuntu 22.04 — long scenario medians

| Policy | Wall ms | CPU ms | Sum phase p95 ms |
|---|---:|---:|---:|
| Fixed 1 | 4205.5 | 3199.3 | 355.9 |
| Fixed 2 | 3150.8 | 3206.8 | 281.0 |
| Fixed 4 | 3073.7 | 3265.9 | 334.3 |
| Fixed 8 | **3071.6** | 3277.3 | 339.5 |
| Direct simple quota | 3177.7 | 3206.3 | 273.9 |
| AlpenCat simple quota | 3170.3 | **3204.9** | **274.0** |
| Direct bounded search | 3217.0 | 3249.7 | 340.5 |
| AlpenCat bounded search | 3210.4 | 3248.8 | 337.9 |

The **short scenario** also failed the preset distinct-gain gate in both environments. Its per-run and median results are in the CI artifacts, not excluded from the verdict.

## Interpretation

1. The bounded candidate selection **cannot be recommended** as a default. The exploration overhead and instability overwhelm its theoretical ability to choose a better number of workers in these workloads.
2. The quota-only policy shows a CPU/p95 versus throughput trade-off: it sometimes lowers queued batch p95 and CPU use relative to aggressively parallel fixed strategies, but loses on total Wall Time.
3. Direct OS reading with identical policy has approximately the same economics as ResourceState. No independent stable application CPU/Wall gain of AlpenCat Core was established.
4. `best_fixed` is a **conservative optimistic per-round baseline** selected among fixed policies using hindsight for each metric, not a deployable single fixed strategy.
5. No production default strategy, version tag, or `main` change is justified. The experiment supports an application-specific measurable objective and amortization of observation/calibration cost, not unconditional global optimization.

This is controlled experimental CPU-bound queued processing under actual Cgroup quota, not production traffic or cloud billing evidence.
