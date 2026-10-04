# AlpenCat — Architecture and Research Direction

> Updated: 2026-10-04  
> Status: Architecture converged; native integration validation open

## Current research statement

AlpenCat asks:

> **Can execution boundaries remain valid under changing compute conditions using only native state transitions, a tiny stale/epoch mechanism, and bounded local revalidation — without continuous performance prediction?**

The architecture-exploration phase is now effectively complete.

The remaining work is validation of real platform adapters and exact implementation overhead.

## Converged architecture

```text
native transition semantics
        |
        v
ResourceEpoch / stale
        |
        +---- normal calls remain on FastRoute
        |
        v
future near-boundary demand
        |
        v
bounded revalidation when justified
        |
        v
publish updated boundary
```

Minimal candidate:

```text
FastRoute
+ Boundary
+ ResourceEpoch / stale
+ optional transition direction / magnitude
+ near-boundary demand
+ optional routed-call slowdown accelerator
+ bounded revalidation
+ conservative fallback
+ thin platform adapters
```

## What has been removed as a default requirement

The current evidence does not justify placing the following on the normal runtime path:

- continuous telemetry;
- periodic active probing;
- global cost prediction;
- online ML;
- a general-purpose scheduler;
- eager recalibration after every resource transition;
- unbounded crossover search;
- a universal fixed slowdown classifier.

These may remain useful as research baselines, but they are not part of the current architectural commitment.

## Core findings

### 1. Boundary invalidation is real

Controlled x64 experiments demonstrated that a previously correct serial/parallel crossover can move substantially under strong contention or severe capacity loss.

### 2. Invalidation must be lazy

Mild capacity changes frequently produced little or no boundary movement. Therefore:

```text
resource transition != immediate recalibration
```

The transition should first invalidate confidence, not automatically spend measurement budget.

### 3. Revalidation must be bounded

Full outward crossover search becomes economically poor when the parallel route disappears.

A bounded neighborhood search plus conservative fallback sharply reduced this pathological cost.

### 4. Revalidation spending must also be lazy

Even bounded revalidation can be wasteful when stale-boundary regret is tiny.

The control plane should remain dormant until relevant demand makes the check economically meaningful.

### 5. Route slowdown is secondary evidence

Across heterogeneous Intel/AMD runners, heavy contention produced a strong slowdown signal.

However, mild and severe CPU-budget reductions overlapped. A fixed slowdown threshold therefore cannot be the primary universal classifier.

Native transition semantics should be used first when the platform exposes them.

### 6. The hot path remains lightweight

A dedicated microbenchmark across eight independent x64 runners measured the normal relaxed epoch-check path at approximately:

```text
+0.016 to +0.514 ns / routed call
```

This supports retaining a tiny resource-epoch check in the normal FastRoute.

## Current validation boundary

Completed or provisionally passed:

- problem existence;
- stale-boundary regret;
- lazy invalidation;
- bounded revalidation;
- revalidation economics;
- heterogeneous Intel/AMD x64 mechanism testing;
- normal hot-path lightness;
- control-policy convergence.

Still open:

- real hardware/OS event -> adapter;
- adapter -> ResourceEpoch end-to-end delivery;
- event latency and stale-window measurement;
- exact production implementation overhead;
- vendor/platform coverage.

## Leading native adapter: Intel HFI

Linux `CONFIG_INTEL_HFI_THERMAL` can relay CPU performance and efficiency capability updates to userspace through the thermal Generic Netlink event family.

The first native proof should establish:

```text
physical capability transition
-> Intel HFI
-> Linux thermal Generic Netlink
-> userspace validation probe
-> AlpenCat ResourceEpoch
-> bounded routing response
```

The external validation protocol is documented in:

- `docs/NATIVE_VALIDATION.md`
- `tools/native-validation/`

## Research discipline

AlpenCat continues to prefer the smallest surviving mechanism.

The next phase should validate the converged design, not reopen scheduler architecture unless native integration evidence forces that change.
