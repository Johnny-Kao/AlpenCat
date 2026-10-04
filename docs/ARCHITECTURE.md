# AlpenCat Architecture

> Updated: 2026-10-04  
> Status: Converged minimal runtime; native integration validation open

## Research question

> **How little runtime machinery is required to keep an execution boundary trustworthy when available compute changes?**

The answer has narrowed through measurement and falsification.

AlpenCat no longer attempts to be a general adaptive scheduler.

## Active architecture

```text
PlatformAdapter
      |
      | resource state changed
      v
ResourceEpoch
      |
      v
PublishedBoundary -----> FastRoute -----> backend
      |
      | stale?
      v
near-boundary demand
      |
      v
bounded revalidation
      |
      v
publish new boundary
```

### Core primitives

**ResourceEpoch**

A monotonic invalidation signal. Thin native adapters advance it when a relevant platform transition occurs.

**PublishedBoundary**

The currently trusted crossover state plus the resource epoch at which it was validated. Reads are lock-free; publication uses a small sequence protocol so readers cannot observe mixed boundary fields.

**FastRoute**

A direct comparison against the published boundary. It does not run telemetry, prediction, or alternate-route probes.

**Bounded revalidation**

A cold-path mechanism entered only when stale state and relevant demand justify paying measurement cost.

## Active crates

| Crate | Responsibility |
| --- | --- |
| `runtime-core` | Boundary, publication, ResourceEpoch, shared execution primitives |
| `runtime-selector` | FastRoute |
| `runtime-api` | Public runtime surface and explicit invalidation/publication |
| `runtime-cpu-rayon` | CPU execution backend |
| `runtime-gpu-wgpu` | Optional GPU execution backend |
| `runtime-machine` | Lightweight capability discovery |

## Removed from the active design

The following were implemented and tested during earlier exploration, but are no longer architectural requirements:

- continuous runtime telemetry;
- online cost models;
- adaptive execution planners;
- resource brokers;
- policy caches driven by live resource fingerprints;
- continuous rebalancing;
- general work-unit scheduling;
- migration/lease control layers;
- CFFI-specific routing experiments.

They were removed rather than retained as compatibility baggage.

Recovery point:

```text
archive/pre-runtime-pruning-2026-10-04
```

Git history and prior PRs preserve the full implementation record.

## Why the scope shrank

Experiments established:

1. boundary invalidation is real;
2. many transitions are harmless enough that eager work is wasteful;
3. full recalibration can be much more expensive than stale-boundary regret;
4. bounded local validation is sufficient for the severe cases tested;
5. slowdown magnitude is useful evidence but not a universal state classifier;
6. native platform semantics are a better trigger than continuous inference.

The surviving architecture therefore pays almost nothing in the steady state and spends only after a meaningful invalidation.

## Native adapter contract

A native adapter should remain thin:

```text
native event
-> normalize minimal transition metadata if available
-> ResourceEpoch.invalidate()
```

It should not:

- choose a backend;
- run a predictor;
- own a scheduler;
- continuously poll performance counters when an event interface exists.

Intel HFI is the first native validation target.

See [Native Validation](./NATIVE_VALIDATION.md).

## Remaining validation

The next two technical gates are:

1. run the production-core hot-path matrix using the current `PublishedBoundary` + `ResourceEpoch` implementation;
2. receive a real physical-server native event and connect it to the epoch invalidation path.

No broader scheduler redesign is planned unless those tests produce evidence that forces one.
