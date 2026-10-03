# AlpenCat — Current Research Direction

> Updated: 2026-10-04  
> Status: Experimental research project

AlpenCat is currently investigating a deliberately narrow question:

> **How little runtime information is sufficient to keep an execution boundary valid when compute is constrained?**

The project is no longer centered on continuous telemetry, online cost modeling,
global prediction, or general-purpose scheduling.

## Current target

AlpenCat targets workloads where:

- multiple equivalent execution paths already exist;
- choosing the wrong path wastes scarce compute;
- control overhead must be much smaller than the work being protected;
- execution conditions can change after a boundary was calibrated;
- the OS, kernel, driver, or platform may already expose useful state transitions.

The current primary research environment is constrained x86-64 server execution,
especially serial-vs-parallel CPU routing under limited core budgets,
contention, quota changes, and related resource pressure.

CPU/GPU routing remains relevant, but it is no longer the only or primary
definition of the problem.

## Current hypothesis

The runtime may not need to predict full backend performance.

A much smaller mechanism may be sufficient:

```text
published route boundary
        |
platform / OS state transition
        |
mark affected boundary stale
        |
no immediate recalibration
        |
future call arrives near old boundary
        |
localized revalidation
        |
publish new boundary
```

The upper layer should ideally consume only small state-change information such
as:

```text
CPU_CAPACITY_CHANGED
MEMORY_PRESSURE_CHANGED
GPU_CAPACITY_CHANGED
THERMAL_STATE_CHANGED
```

Platform adapters may derive those transitions differently.

Examples currently under study:

- Linux: PSI events;
- ARM/Linux: hardware-capacity / hw-pressure mechanisms where available;
- Android: thermal-state/headroom callbacks;
- NVIDIA: limiting/throttle state;
- Apple: thermal-state notifications.

The platform-specific signal is not itself AlpenCat's contribution.

## Candidate contribution

If the current hypothesis survives falsification, AlpenCat's contribution is:

1. **Boundary validity**
   - maintain whether an execution crossover is still trustworthy.

2. **Localized revalidation**
   - re-measure only around the previous crossover rather than rebuilding a
     global performance model.

3. **Minimal control plane**
   - keep the hot routing path tiny and leave the control plane dormant until a
     real state transition and relevant demand occur.

The current research framing is therefore:

> **Can existing hardware/OS resource-state transitions be used only as
> invalidation events, allowing execution routing to adapt without continuous
> performance prediction?**

## Current minimal architecture candidate

```text
FastRoute
+
Boundary
+
ResourceEpoch / stale bits
+
LocalizedRevalidation
+
PlatformAdapter
```

Research components such as continuous telemetry, global cost models,
`RecentUseRate`, `RecalibrationEconomics`, active probing, and adaptive
predictors are not assumed to belong in the final runtime.

They remain useful as historical experiments, baselines, and falsification
evidence.

## Research discipline

The project explicitly allows negative results.

Possible outcomes include:

- the minimal boundary-validity runtime is sufficient;
- a small fallback signal is required;
- static routing is already optimal enough for the target regime;
- some earlier adaptive machinery is unnecessary and should be removed.

A smaller surviving architecture is considered progress.

## Immediate research program

The next evidence program focuses on constrained x86-64 execution:

- 4 → 2 → 1 effective CPU budget;
- CPU quota transitions;
- external co-tenant contention;
- serial-vs-parallel crossover movement;
- stale static-boundary regret;
- localized recovery cost;
- comparison against static and oracle references.

Hosted GitHub Actions are useful for this algorithmic layer because they provide
repeatable constrained Linux/x64 environments.

They do **not** establish vendor-specific Intel/AMD thermal, frequency, or
hardware-pressure behavior. Those require physical or explicitly identified
hardware later.

## Historical architecture

Earlier AlpenCat work explored a much broader adaptive-runtime architecture:
telemetry, online cost models, planners, brokers, rebalancers, CPU/GPU routing,
and learned selection.

That work is preserved as research history and evidence. It should not be read
as the current architectural commitment.

See:

- `docs/HISTORICAL_ARCHITECTURE_V0_1.md`
- historical validation and experiment records under `docs/`

The repository remains open while the research direction evolves.
