# 0.5 CPU constrained HTTP p95 — heldout validation plan

**Prospective holdout, drafted after the exploratory HTTP experiment but before executing these new mixed-traffic cases.** It does not alter the reference server or the already observed exploratory results. The independent validation is a **new workload mix / new randomized rounds** on the same reference HTTP application, not validation on a different production application.

## Fixed traffic shape and controls

The experimental request generator is external Python, sends actual TCP HTTP GET requests with 16 closed-loop concurrent clients to a persistent `tiny_http` service inside a real transient Linux Cgroup v2 service. Actual cpu.max changes per service from 2 CPU → 0.5 CPU → 2 CPU, read back by controller. `cpu.stat` must report throttling during 0.5 CPU phase.

This is a **request-cost mix** held out from the original homogeneous exploratory workload:

- `mixed`: 120 requests/phase, alternating 2,800,000 vs 1,100,000 CPU iterations.
- `weighted`: 192 requests/phase, every fourth request 3,000,000 CPU iterations, other requests 1,500,000.
- 5 independent, order-randomized paired rounds per workload and per Ubuntu runner, with one warmup per mode, on Ubuntu 22.04 and Ubuntu 24.04.
- The same client request IDs, numerical computation, and HTTP response SHA-256 fingerprint must match across all modes within each phase/round.

Compared strategies: fixed8, fixed4, fixed2, direct_guard, alpencat_guard and alpencat_quota. The **direct_guard** and **alpencat_guard** both use precisely the same preselected 4→2→4 active-worker policy. No per-request tuning or calibration. The only difference is direct OS snapshot versus ResourceState invalidation/refresh. A global compute benefit unique to resource acquisition itself is not anticipated.

## Predeclared restricted-phase economic acceptance (not retrospective tuning)

The only **primary decision phase** is the middle, actual **0.5 CPU** quota. For each mixed workload on each OS runner, require:

1. `alpencat_guard` improves measured full-client HTTP response **p95 by greater than 5% against fixed8 in at least 4 of 5 paired rounds**.
2. Median additional process CPU Time is **no worse than 5%** against fixed8.
3. Median HTTP load-generator total Wall Time is **no worse than 5%** against fixed8.
4. The `direct_guard` identical policy arm has median p95 and CPU within **±5%** of `alpencat_guard`. This tests *parity*, not a promised AlpenCat-specific acceleration.

Predeclared **research SLO thresholds for that restricted phase** are 160ms for `mixed` and 220ms for `weighted` p95. Record how many of 5 restricted phases each mode meets these illustrative absolute targets; paired relative improvements are still required regardless of these targets.

A *conditional GO* requires the primary criteria in **both workloads on both Ubuntu runners**. The protocol always changes quota in three real stages to confirm recovery, and reports all three stages in raw data, but the conditional claim is specifically about the restricted middle phase. A 0.5-CPU conditional GO can neither guarantee net gains across the unrestricted phases nor guarantee general cloud/production benefits. The direct OS parity control prevents misleading credit attribution.

If either benchmark fails or adaptation adds too much CPU / Wall cost, report **NO-GO** without changing thresholds or cherry-picking favorable workloads. This test, like the original, uses a reference HTTP app, not a third-party deployed service.

