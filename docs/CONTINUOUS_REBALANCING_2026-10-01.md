# M14 — Continuous Rebalancing

Status: BASELINE IMPLEMENTED (2026-10-01).

## Goal

Close the adaptive loop without putting the full planner in the hot path.

M14 observes compact runtime telemetry and asks M12+M13 to recompute only when a material event occurs.

The control loop is:

```text
execute queued work
-> record lightweight telemetry
-> M14 checks compact counters/fingerprint
-> no material event: keep current plan
-> material event: re-run M12 + M13
-> replace plan for future/pending work only
```

Already-running chunks are never interrupted by M14.

## Design rule

M14 is intentionally **not**:

```text
every chunk
-> rediscover machine
-> run cost model
-> run execution planner
```

Instead, the common path is only a small comparison of counters and pressure bands.

The expensive control path runs only after a trigger.

## Rebalance session

A task starts a lightweight:

```text
RebalanceSession
```

The session retains:

- current ExecutionPlan;
- baseline telemetry window;
- last evaluated telemetry window;
- internal rebalance policy.

Public runtime entry points:

```rust
Runtime::begin_rebalancing(...)
Runtime::rebalance_if_needed(...)
```

A successful replan returns:

```text
RebalanceUpdate {
    reason,
    plan
}
```

## Trigger classes

### 1. Backend failure

A newly observed backend failure may trigger an immediate replan.

Failure is allowed to bypass the ordinary minimum observation window because continuing to feed a failing resource can be both expensive and unsafe.

### 2. Material pressure change

CPU/GPU in-flight pressure is quantized into coarse bands.

Small fluctuations inside one band do not trigger M12/M13.

Crossing a pressure band can trigger re-planning, but only after a minimum completed-work window.

Pressure changes are symmetric:

- pressure rises -> M13 may reduce that backend's share;
- pressure falls -> M13 gets a chance to restore share.

### 3. Sustained throughput degradation

M14 does not use a single latest sample as sufficient evidence.

It compares cumulative work/time across an observation window and requires multiple completed executions.

The default baseline requires a material degradation relative to the established backend history before re-planning.

### 4. No material change

Return Keep.

M12/M13 are not called.

## Default throttling

The baseline policy uses internal defaults:

```text
minimum completed executions before ordinary replan: 8
pressure bands: 4
sustained degradation threshold: 25%
```

These are runtime defaults, not ordinary user tuning requirements.

They exist to make the control loop stable and cheap.

Future benchmark evidence may change these defaults.

## Interaction with M11.5 cache

When M14 decides to replan:

1. the task's cached execution policy is invalidated;
2. M12 recomputes backend economics;
3. M13 generates a new ExecutionPlan;
4. the RebalanceSession adopts the new plan.

This prevents a stale tiny/hot-path cached route from surviving a material resource change.

## Pending-work rule

M14 modifies only future/pending work.

It does not cancel already-running CPU/GPU chunks.

This keeps task semantics predictable and avoids cancellation/replay complexity.

## Recovery behavior

M14 is bidirectional.

A backend that was previously under high pressure can re-enter planning when its pressure materially falls.

The baseline does not actively launch synthetic benchmark probes merely to test recovery.

Recovery evidence therefore comes from:

- residual workload;
- other runtime tasks;
- observed pressure/failure changes;
- later task executions.

This keeps M14 lightweight and avoids turning the resource controller into its own workload.

## Relationship to M10/M11

M10 continues to own work decomposition.

M11 continues to allocate individual pending WorkUnits according to current availability.

M14 sits above them:

```text
M14 = when should the plan itself change?
M11 = where should the next eligible WorkUnit go under the current plan/state?
```

## Completion boundary

M14 baseline is complete when:

- failures can trigger immediate replan;
- ordinary replan is throttled;
- pressure change works in both directions;
- sustained degradation requires accumulated evidence;
- stable workloads do not replan;
- zero remaining work never replans;
- replanning invalidates stale cached policy;
- only pending/future work is affected.
