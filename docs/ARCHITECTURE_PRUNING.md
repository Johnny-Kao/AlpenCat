# Runtime Architecture Pruning

> Status: Working classification for the post-convergence AlpenCat architecture
> Updated: 2026-10-04

AlpenCat's research scope has narrowed substantially through measurement and falsification.

The repository still contains much of the earlier adaptive-runtime implementation. This document separates the code that still matches the current architecture from code that should be treated as support, historical research, or removal candidates.

## Current architectural target

```text
native platform transition
        |
        v
ResourceEpoch / stale
        |
        v
FastRoute using a published Boundary
        |
        v
future near-boundary demand
        |
        v
bounded local revalidation when justified
        |
        v
publish updated Boundary
```

The desired production core is intentionally small:

```text
FastRoute
Boundary
ResourceEpoch / stale
PlatformAdapter
BoundedRevalidation
ConservativeFallback
```

Continuous telemetry, global online prediction, general work planning, and continuous rebalancing are no longer default architectural requirements.

## Crate classification

### A. Current core / likely to survive

| Crate | Status | Reason |
| --- | --- | --- |
| `runtime-core` | KEEP / REWORK | Backend-neutral primitives remain useful, but it should gain the new Boundary / ResourceEpoch concepts and shed unrelated assumptions. |
| `runtime-selector` | KEEP / REWORK | Closest existing implementation to FastRoute and calibrated boundaries. Current fixed serial/CPU/GPU thresholds should evolve into explicit published boundary state. |
| `runtime-cpu-rayon` | KEEP AS BACKEND | Execution backend only. It does not decide routing and remains compatible with the thinner architecture. |
| `runtime-gpu-wgpu` | KEEP AS BACKEND | Execution backend only. Useful where GPU routing remains supported; it should stay outside the control-plane core. |

### B. Support code — keep only if it remains independently useful

| Crate | Status | Reason |
| --- | --- | --- |
| `runtime-machine` | SUPPORT / REASSESS | Lightweight host discovery can remain useful for capability reporting and validation, but it should not become continuous routing telemetry. |
| `runtime-cffi-bridge` | SUPPORT / REASSESS | Thin foreign-function integration may still be useful, but it is not part of the boundary-validity mechanism itself. |

### C. Public/API layer that must be rewritten around the new core

| Crate | Status | Reason |
| --- | --- | --- |
| `runtime-api` | REWRITE | The current API directly owns telemetry, online cost modeling, policy cache, adaptive planning, broker logic, and rebalancing. It therefore exposes the old architecture and currently pulls most legacy crates back into the workspace graph. |

The current `runtime-api` is the main reason the source tree still looks like the old project.

### D. Historical adaptive control plane — move out of the active runtime

| Crate | Status | Reason |
| --- | --- | --- |
| `runtime-telemetry` | HISTORICAL | Implements continuous runtime-owned telemetry; this is explicitly no longer a default requirement. |
| `runtime-cost-model` | HISTORICAL | Implements online performance prediction and learned cost estimates; current research no longer requires a global online predictor. |
| `runtime-execution-planner` | HISTORICAL | Converts learned costs and live telemetry into adaptive execution plans and backend mixes; this belongs to the earlier broad architecture. |
| `runtime-rebalancer` | HISTORICAL | Implements continuous telemetry-window rebalancing; current architecture instead uses native invalidation plus lazy bounded revalidation. |
| `runtime-policy-cache` | HISTORICAL | Caches policies based on telemetry/resource fingerprints from the earlier adaptive control plane. |
| `runtime-broker` | HISTORICAL | Resource-pressure-aware work assignment belongs to the broader scheduler/broker design that AlpenCat no longer targets. |
| `runtime-planner` | HISTORICAL / POSSIBLE STANDALONE | Work chunk planning is technically valid code, but general work decomposition is not part of the current boundary-validity contribution. Preserve as research/support unless a concrete surviving use appears. |
| `runtime-integration` | HISTORICAL / REASSESS | Migration/lease logic depends on the old `ExecutionPlan` abstraction. Preserve the safety ideas, but the current crate shape is coupled to the historical planner. |

## Dependency problem today

The active graph is still effectively:

```text
runtime-api
  +-- telemetry
  +-- cost-model
  +-- execution-planner
  +-- planner
  +-- broker
  +-- policy-cache
  +-- rebalancer
  +-- integration
  +-- selector
  +-- CPU/GPU backends
  +-- machine discovery
```

That graph describes the earlier adaptive runtime, not the converged AlpenCat design.

The target graph should become closer to:

```text
runtime-api
  |
  +-- runtime-core
  |     +-- Boundary
  |     +-- ResourceEpoch
  |     +-- stale state
  |
  +-- runtime-selector
  |     +-- FastRoute
  |     +-- bounded revalidation contract
  |
  +-- platform adapter(s)
  |
  +-- optional execution backends
        +-- CPU/Rayon
        +-- GPU/wgpu
```

## Pruning sequence

### P1 — Freeze historical control-plane code

Move the historical adaptive-control crates out of the active production narrative and clearly mark them as research history.

No semantic changes are required before relocation.

### P2 — Rewrite `runtime-api`

Remove ownership of:

- `RuntimeTelemetry`
- `OnlineCostModel`
- `ExecutionPolicyCache`
- `AdaptiveExecutionPlanner`
- `ResourceBroker`
- `RebalanceSession`

Replace the public control path with the converged primitives:

- published boundary;
- resource epoch;
- stale check;
- fast route;
- bounded revalidation entry point;
- conservative fallback.

### P3 — Remove legacy crates from the active Cargo workspace

Once no active code imports them, move historical crates under a research/archive location or remove them from the workspace.

This keeps the research record without implying that they are production components.

### P4 — Introduce native platform adapters separately

Platform adapters should normalize external transitions into a minimal event contract.

They should not own routing decisions.

Conceptually:

```text
native event
-> CapacityEvent / ResourceEpoch change
-> runtime core
```

### P5 — Re-run exact production overhead validation

After the new minimal core exists, repeat the hot-path benchmark on the exact implementation.

The synthetic evidence already suggests the epoch check can remain sub-nanosecond to low-sub-nanosecond on x64, but the final claim should use the production code.

## Non-goals during pruning

Do not use pruning as an opportunity to:

- design another scheduler;
- add continuous monitoring;
- preserve old modules only because effort was spent building them;
- prematurely delete useful research evidence;
- mix native adapter implementation into the core routing policy.

The criterion is simple:

> Keep code in the active runtime only if the converged boundary-validity architecture needs it.
