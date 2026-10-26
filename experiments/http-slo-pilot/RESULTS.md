# Independent HTTP Pilot — Verified Results (2026-10-26)

**CI/experiment integrity: PASS on Ubuntu 22.04 and Ubuntu 24.04. Predeclared overall GO: FALSE.**

[GitHub Actions #40342992899](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40342992899) tested commit `0099d0cd236fb26e2c92ed998ac5956ea2802293`. Complete `http-slo-results.csv`, JSON and console summary are workflow artifacts. The previous first run #40342657355 failed because the shorter sample did not trigger measurable throttling; workload sizes were increased and documented before this successful run. Source `main` and released tags were not touched.

## Honest protocol

- Independent reference server based on open-source **tiny_http** 0.12; 8 persistent Rust worker threads with 1/2/4/8 active receivers and real HTTP over localhost TCP; external Python driver uses 16 closed-loop clients.
- Each service runs inside a dedicated systemd Cgroup v2 with real verified quota transitions **2→0.5→2 CPU** and actual throttling during the limited phase. Only its transient service receives quota changes.
- `short`: 96 HTTP requests × 1,800,000 integer iterations each per phase; `long`: 144 × 2,000,000. Eight modes × two cases, four randomized paired trials plus warmup.
- All modes return the **same response checksums** for matching request IDs/phase/round, with zero HTTP status failures.
- `direct_guard` and `alpencat_guard` both apply the **same** preselected 4→2→4 worker rule; only direct `snapshot()` vs `ResourceState.invalidate()+refresh()` differs. Controls include four fixed worker counts and the older quota-ceiling rule via both direct OS and AlpenCat.
- Server process-wide CPU time, external client end-to-end HTTP response p95/p99, wall/throughput and `cpu.stat` are all recorded. This is 16-client **closed-loop**, not open-loop arrivals or a deployed production application.
- Research absolute SLO assumptions (250ms per phase for short, 700ms long) were too permissive: all eight modes passed on all 12 measured phases per workload and runner, so **absolute SLO attainment did not discriminate policies**. This is a valid negative finding.

## Three-phase aggregate medians

Entries are **sum of phase-specific p95**, total process CPU time across phases and total client Wall Time across phases, in milliseconds. Summing per-phase p95 is **not** a single p95 across all HTTP requests; each individual phase's real end-to-end HTTP p95 is preserved in raw JSON/CSV.

### Ubuntu 22.04

| Scenario / policy | Sum p95 ms | CPU ms | Client Wall ms |
|---|---:|---:|---:|
| short fixed2 | 131.3 | 664.3 | 602.0 |
| short fixed8 | 151.2 | 681.5 | 538.0 |
| short Alp quota 2→1→2 | **124.9** | 663.5 | 609.1 |
| short direct guard 4→2→4 | 160.2 | 690.7 | 618.6 |
| short Alp guard 4→2→4 | 135.7 | 654.0 | **512.2** |
| long fixed2 | 138.9 | 1111.9 | 1055.2 |
| long fixed8 | 195.4 | **1094.3** | 955.4 |
| long Alp quota 2→1→2 | **137.2** | 1120.2 | 1074.4 |
| long direct guard 4→2→4 | 147.3 | 1107.0 | **925.7** |
| long Alp guard 4→2→4 | 166.2 | 1101.8 | 928.0 |

### Ubuntu 24.04

| Scenario / policy | Sum p95 ms | CPU ms | Client Wall ms |
|---|---:|---:|---:|
| short fixed2 | **153.1** | 922.3 | 863.5 |
| short fixed8 | 258.4 | **903.3** | 776.7 |
| short Alp quota 2→1→2 | 153.8 | 925.9 | 890.4 |
| short direct guard 4→2→4 | 165.2 | 915.5 | **777.4** |
| short Alp guard 4→2→4 | 194.0 | 916.6 | 813.5 |
| long fixed2 | 237.3 | 1539.5 | 1509.5 |
| long fixed8 | 316.6 | **1501.4** | 1407.5 |
| long Alp quota 2→1→2 | **218.7** | 1544.1 | 1486.7 |
| long direct guard 4→2→4 | 300.4 | 1520.1 | **1363.8** |
| long Alp guard 4→2→4 | 300.7 | 1520.2 | 1367.8 |

## Post hoc: the restricted **0.5 CPU** phase alone

These are newly observed *subgroup* comparisons and **not** the predeclared full-protocol GO criterion. Relative to fixed 8 workers, **alpencat_guard** reduced the actual end-to-end HTTP p95 in *all 16/16 paired runs* (2 runners × 2 sizes × 4 rounds), but CPU cost often increased. Median paired percentage gains for p95 / process CPU / external HTTP Wall:

| Runner / scenario | p95 gain | CPU gain | Wall gain |
|---|---:|---:|---:|
| Ubuntu 22.04 short | +5.6% | -0.8% | +2.8% |
| Ubuntu 22.04 long | +8.1% | -1.9% | -0.7% |
| Ubuntu 24.04 short | +37.4% | -3.5% | -2.2% |
| Ubuntu 24.04 long | +6.6% | -3.2% | -0.2% |

Positive percentage = lower cost/latency for AlpenCat. Results reflect a **latency/CPU/throughput tradeoff under real resource restriction**, not generalized or economic net benefit, and still require independent holdout verification with a predeclared *restricted-phase* target. The **identical direct-OS guard** had similar behavior and resource acquisition itself does not confer unique compute gains.

## Decision

- **PASS:** real Linux quota control, persistent HTTP worker pool, checksum equivalence, end-to-end request latency instrumentation, throttling observability, 2-platform CI.
- **NO-GO:** overall preregistered 5% p95 / CPU+Wall budgets / direct parity conditions across both scenarios and runners; evidence cannot justify a universal faster-by-default policy.
- **CONDITIONAL R&D hypothesis, not acceptance:** under tight 0.5 CPU quota, lower active worker concurrency may improve HTTP p95 relative to a saturated fixed-8 baseline, at variable CPU and Wall cost.
- The research application has explicit worker policy; **AlpenCat Core stays read-only** and neither production runtime nor default behavior changes.
- Next independent test, *if worthwhile*: predeclare a restricted-phase p95 target and CPU/throughput guardrails in a heldout traffic mix. If not robust, stop policy optimization and treat AlpenCat as a resource-context SDK. Do not cherry-pick favorable Runner/workload observations.

