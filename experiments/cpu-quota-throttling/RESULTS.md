# Real CPU Quota / Throttling — Verified Outcome

Validated 2026-10-24 on [GitHub Actions #40097348030](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40097348030). Both Ubuntu 22.04 and Ubuntu 24.04 jobs **passed experiment integrity** (including resource library fixtures and isolated deterministic workload tests). The economics decision is **NO-GO**: the simple quota-based worker policy has no stable incremental gain versus reasonable fixed and direct-OS baselines.

## What was genuinely changed

Each workload trial ran in a **dedicated systemd transient service**. The experiment modified the service's real Cgroup v2 `cpu.max` from **2 CPU → 0.5 CPU → 2 CPU** and read back the actual quota. Linux `cpu.stat` showed nonzero `nr_throttled` changes in the restricted middle phase, including the initial warmup. It did not simulate quota or interfere with the rest of the runner.

Same checksum and request count across strategies; 5 paired, shuffled rounds each on short and long deterministic saturated parallel request queues. CPU time came from `CLOCK_PROCESS_CPUTIME_ID`, request p95 was measured from a common phase enqueue instant, and wall time enclosed observation, thread creation and actual work. Values below are medians of the **sum of three phases**, not individual-request HTTP service timings.

## Ubuntu 24.04 (long workload)

| Strategy | Wall ms | Process CPU ms | Sum of phase p95 ms | Sum of throttled periods |
| --- | ---: | ---: | ---: | ---: |
| Fixed 1 worker | 3537.2 | 2692.0 | 3352.7 | 17 |
| Fixed 2 workers | 2656.4 | 2699.4 | 2514.7 | 19 |
| Fixed 4 workers | 2580.1 | 2770.5 | 2457.5 | 26 |
| Fixed 8 workers | **2561.0** | 2773.9 | **2440.8** | 26 |
| Direct OS API, workers=ceil(quota) | 2650.1 | 2699.1 | 2510.5 | 18 |
| AlpenCat ResourceState, same policy | 2647.1 | 2695.7 | 2507.7 | 17 |

## Ubuntu 22.04 (long workload)

| Strategy | Wall ms | Process CPU ms | Sum of phase p95 ms |
| --- | ---: | ---: | ---: |
| Fixed 1 worker | 2630.0 | 2021.7 | 2511.2 |
| Fixed 2 workers | 1967.6 | 2032.2 | 1842.4 |
| Fixed 4 workers | 1869.0 | 2059.5 | **1804.3** |
| Fixed 8 workers | **1858.6** | 2034.6 | 1833.1 |
| Direct OS API, workers=ceil(quota) | 1973.1 | **1999.2** | 1856.8 |
| AlpenCat ResourceState, same policy | 1944.0 | 2015.7 | 1863.2 |

Short workload results and every individual round are preserved in the [Actions artifacts](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40097348030), together with the CPU usage and throttling counters.

## Findings and release decision

- CPU quota controls and throttling: **PASS**.
- Multi-worker computation equivalence: **PASS**.
- Faster than fixed one-worker wall time: **YES**, but does not establish incremental value versus the strongest choices.
- Stable p95 improvement ≥5% in ≥4/5 trials versus best fixed, without CPU regression >2%: **NO**.
- Stable p95 improvement ≥2% in ≥4/5 trials versus direct OS API using identical policy: **NO**.
- Robust net AlpenCat acceleration claim: **NOT SUPPORTED**. A configured CPU quota is a capacity bound, not the number of workers that minimizes application latency.

The experiment found a genuine resource-adapter correctness issue on VM-based runners: the top-level cgroup v2 hierarchy need not expose `cpu.max`/`memory.max`. The **research branch only** now handles this scoped root case and has a regression fixture.

No release or `main` changes. No default automatic tuning should be enabled on the basis of this experiment. Any later candidate policy should use a separately specified application objective (latency SLO vs CPU usage) and measured benefit/rollback rather than unconditional `workers=ceil(quota)`. Negative evidence is retained rather than tuned away.
