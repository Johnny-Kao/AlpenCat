# Universal Policy Reset — 2026-10-07

> Status: active research direction
> Scope: Generic AlpenCat only
> Runtime hot-path change: none

## Decision

Stop refining the current runner-specific estimator stack.

The immediate objective is not maximum route-selection accuracy on the present W1/W2/W3 measurements. The objective is a portable Generic policy that can retain useful value across machines, processors, and stable or non-stable operating regimes.

A lower hit rate is acceptable if the rule is reusable.

## Why the reset is necessary

Recent research showed that W2 can move between materially different measured states across fresh CI runs:

- real recoverable opportunity + candidate miss;
- published candidate;
- near-zero recoverable opportunity.

That means the current measured state is not a trustworthy stationary target for fine fitting.

Further refinement of:
- EVSI proxies;
- bridge-value estimators;
- empirical bridge bounds;
- opportunistic sampling rates;
- event-driven sampling rules;
- workload-specific stopping behavior

would risk optimizing the estimator to transient runner behavior rather than learning a portable control law.

## Generic objective

The Generic layer should aim to capture a meaningful fraction of available value, not all of it.

A useful target is:

- modest but repeatable positive net value across heterogeneous systems;
- low downside when the environment is different from the research runner;
- graceful no-op behavior when evidence is weak;
- no dependency on CPU vendor/model or workload-specific constants.

Capturing approximately 50–70% of the economically available opportunity can already be valuable if that behavior generalizes.

Higher capture belongs to later specialization layers.

## Minimal Generic control law

Keep only principles that remain valid under state uncertainty:

1. Resource state changes can invalidate routing assumptions.
2. Observation has a cost.
3. Switching has a cost.
4. Do not adapt unless observed evidence indicates material economic exposure.
5. Prefer bounded observation over broad search.
6. Do not extrapolate beyond directly observed evidence.
7. Do not switch unless conservative expected gain exceeds measured adaptation cost.
8. Verify cheaply after switching and allow rollback.
9. When evidence is ambiguous, stay/defer rather than buying precision aggressively.

The Generic layer does not need to identify the true global optimum.

## Current Generic decomposition

```
resource transition
    ↓
cheap evidence
    ↓
material opportunity?
    ├─ no / uncertain → stay
    ↓ yes
safe candidate directly observed?
    ├─ no → stay / defer
    ↓ yes
conservative gain > adaptation cost?
    ├─ no → stay
    ↓ yes
switch
    ↓
small verification window
    ├─ confirms → keep
    └─ fails → rollback
```

This intentionally removes deeper search as a requirement for the Generic layer.

## Parked research

The following remain useful research artifacts but are not part of the active Generic policy:

- fixed p3/p5 estimator comparisons;
- observed-only EVSI v1/v2;
- sentinel bridge value;
- empirical bridge bounds;
- opportunistic evidence models;
- event-driven opportunistic sampling;
- detailed unresolved-state search policies.

They may be revisited later for:
- platform-aware policy;
- Intel specialization;
- AMD specialization;
- Apple/ARM specialization;
- machine-experienced policy.

They must not be used to tune the Generic layer against the current runner.

## Validation standard from this point

Do not optimize against one run.

A Generic candidate must be judged on:

1. directionally positive economics across multiple fresh runs;
2. multiple workload families;
3. different resource regimes;
4. cross-runner or cross-machine evidence;
5. low false-positive adaptation cost;
6. acceptable value capture without model-specific tuning.

Primary success criterion:

> Does one unchanged rule produce repeatable positive value across heterogeneous conditions?

Not:

> Does it match the oracle boundary or maximize capture on this runner?

## Specialization ladder

```
Static
  → Generic AlpenCat
  → Platform-aware AlpenCat
  → Vendor-optimized AlpenCat
  → Machine-experienced AlpenCat
```

Expected role:

- Generic: robust partial value capture.
- Platform-aware: exploit OS/runtime signals.
- Vendor-optimized: Intel/AMD/Apple/ARM-specific topology and telemetry.
- Machine-experienced: local learned priors and historical profiles.

The 70–90%+ capture regime is a specialization problem, not a requirement for Generic AlpenCat.

## Immediate next research step

Freeze estimator refinement.

Next experiments should test the minimal Generic control law across heterogeneous environments and determine its value-capture distribution.

No new Generic thresholds or estimator families should be added until cross-machine evidence exists.
