# Compressed Universal Gate Finalists — 2026-10-08

> Status: frozen for final heterogeneous holdout
> Scope: Generic eligibility/economic gating only
> Runtime hot-path change: none

## Why compression

The earlier layer-by-layer research loop was too slow and risked fitting transient runner states.

The remaining Generic question is compressed to one final heterogeneous holdout comparing three fixed candidates. No finalist may be changed after seeing the holdout result.

## Finalists

### F0 — Safe structural gate

```
logical_cpus > 1
AND resource transition occurred
```

Role:
- maximum observed opportunity recall;
- weakest pruning;
- control candidate.

### F1 — Natural disjoint envelope

F0 plus:

> At least one naturally executed currently selected route has a latency sample envelope fully disjoint from its baseline historical envelope.

Properties:
- no alternate-route probe;
- no percentage threshold;
- no CPU vendor/model branch;
- uses already-occurring execution evidence.

Role:
- balanced pruning/recall candidate.

### F2 — Economic natural slowdown

F0 plus:

```
natural slowdown exposure over expected calls
>
last known revalidation cost
```

Where natural slowdown exposure is accumulated only from the route that would execute anyway, relative to the existing baseline history.

Properties:
- no alternate-route probe;
- no percentage threshold;
- compares value with measured cost directly;
- revalidation-cost history updates only when a revalidation actually occurs.

Role:
- downside-control candidate.

## Development evidence

Combined round 3 + round 4 retrospective indicated approximately:

| Finalist | Opportunity recall | No-opportunity pruning | Worst gated savings |
|---|---:|---:|---:|
| F0 | 100% | 41% | -61.5% |
| F1 | 94.8% | 55% | -61.5% |
| F2 | 92.2% | 50% | -6.9% |

These are development results only, not final claims.

## Final holdout rule

Run one unchanged 7-runner heterogeneous sweep.

The decision after the holdout is GO/STOP:

- If a candidate materially reduces downside while preserving useful opportunity recall across architectures, freeze the Generic layer.
- If no candidate produces acceptable downside/generalization, stop Generic refinement and treat Generic AlpenCat as a conservative eligibility shell only. Move further performance capture to platform/vendor specialization.

No new Generic gate is to be invented from the final holdout.
