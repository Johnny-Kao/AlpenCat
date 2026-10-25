# Bounded Candidate Policy under real Cgroup v2 quotas

This is a **research experiment**, not a default AlpenCat optimization, Beta release, or general performance guarantee.

## Motivation
Previous 2 → 0.5 → 2 CPU Cgroup v2 tests proved that reducing workers to `ceil(cpu quota)` does not reliably reduce total runtime or request p95 compared with sensible fixed 4/8-worker settings. In this experiment, we test whether a bounded application-owned selection can improve the **p95/CPU tradeoff**, accounting for all time spent learning.

## Experimental protocol (fixed before interpreting results)
- Same real `systemd-run` transient service cgroup controls and verification used in the preceding study. CPU quotas really change from **2 → 0.5 → 2**; the worker sees the new actual `cpu.max`; `cpu.stat` must show at least one throttled period during the middle phase.
- Two CPU-bound queued service workloads (`short`, `long`). Each phase has **12 consecutive workload windows** and exactly the same deterministic request indices for every strategy. The first 8 windows of bounded policies are real customer-like work, **not uncharged training**; the final 4 are the exploitation period.
- Fixed controls use 1, 2, 4, or 8 worker threads for the full phase. Quota-based controls use workers `ceil(cpu quota)`. Bounded policy controls try each 1/2/4/8 candidate twice during the first 8 windows (1,2,4,8,8,4,2,1), and then evaluate the observed p95 plus CPU consumption.
- Candidate selection prefers the observed lowest p95 among choices whose CPU time is at most 4% above the best measured CPU candidate. It only changes the previous incumbent if the winner improves p95 by **more than 5%** (hysteresis). After two exploitation windows, if observed p95 exceeds the chosen candidate's probe reference by >25%, it rolls back once to the previous incumbent. This is a *candidate* objective, not a claim of globally optimal policy.
- `direct_search` and `alpencat_search` share the **identical policy algorithm and event signal**. The only difference is direct OS `snapshot()` once per quota-change notification versus `ResourceState.invalidate()+refresh()` once per notification. Neither has unbounded background work. Static and simple quota policies remain honest comparators.
- CPU time is Linux process-clock aggregate CPU; Wall Time encloses observation, exploration, worker creation and exploitation; p95 is the 95th percentile of request completion time since each window's common admission instant, **not HTTP p95**. Across the three phases, the report sums individual phase p95 as a scenario-level indicator; this is not itself the percentile over every request. Raw phase results are retained.
- Two Ubuntu runner environments and five order-randomized paired rounds each, with separate warm-ups. Each arm must produce the same checksum and request count. CI records every trial and group and treats economically negative outcomes as valid **NO-GO**.

## Predetermined decision thresholds
A positive distinctive gain would require AlpenCat bounded selection to show **>5% reduction in summed phase p95 on at least 4/5 paired rounds** versus the **best fixed strategy per round** with median CPU cost regression no worse than 2%. Additionally it must show **>2% p95 reduction on at least 4/5 rounds** versus `direct_search` with identical policy, and CPU regression no worse than 2%. Every workload size and both Ubuntu variants must meet the gate. A positive gain compared only to the simplistic quota-based policy is *informative*, not proof of independent SDK economics.

Negative or inconsistent results must not be represented as a pass. Experimental compilation/cgroup errors are fixed as test-infrastructure bugs, never converted to favorable economic samples.

## Scope and limitations
This uses deterministic CPU-bound batch queues, not live customer traffic, heterogenous GPU, persistent thread pools, CPU pinning, thermal throttling or production SLOs. Cgroup quota accounting measures a capacity cap, not necessarily the optimal worker count. The experimental code is not installed in production AlpenCat Core. Existing releases and `main` remain unchanged.
