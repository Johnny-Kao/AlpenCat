# M11.5 — Cached Execution Policy Layer

Status: BASELINE IMPLEMENTED (2026-10-01).

## Why this exists

The CFFI #282 integration benchmark showed a concrete control-plane boundary:

- consulting the Swiss Knife on every ~200 ns FFI call is functionally safe but adds measurable tax;
- moving the decision to a signature/task boundary and caching the result removes most of that repeated arbitration cost.

The runtime therefore needs two execution speeds:

```text
ultra-hot / tiny work
-> resolve policy at task/signature boundary
-> cache
-> dataplane executes cached route

medium / large work
-> M11 dynamic broker
-> future M12/M13 adaptive planning
-> chunk-by-chunk reassignment when profitable
```

M11.5 does not replace M11.

## Core API

```rust
Runtime::cached_execution_policy(...)
Runtime::invalidate_cached_policy(...)
Runtime::clear_cached_policies()
Runtime::cached_policy_count()
```

The cache returns:

```text
CachedExecutionPolicy {
    backend,
    status: Hit | Planned,
}
```

Adapters are expected to retain the returned policy at their own signature/task boundary. They should not call this API from every nanosecond-scale dataplane operation.

## Cache key

A policy is scoped by:

- task identity;
- logarithmic work-size class.

This prevents one decision for a tiny workload from being blindly reused for a materially different workload size.

## Resource fingerprint / automatic invalidation

A cached entry is reused only while its resource fingerprint remains compatible.

Current fingerprint includes:

- CPU capacity;
- GPU capacity;
- quantized CPU pressure band;
- quantized GPU pressure band;
- GPU range eligibility;
- CPU failure counter;
- GPU failure counter.

Pressure is intentionally quantized. Small fluctuations inside one band do not cause re-planning, while materially different load or full-capacity states do.

A new backend failure also changes the fingerprint, forcing the next policy lookup to re-plan.

## Explicit invalidation

Host integrations may explicitly invalidate one task's cached policy after a semantic/configuration change.

The entire cache can also be cleared.

## Bounded memory

Default maximum entries:

```text
1024
```

The cache evicts the least recently used entry on insertion when full.

This keeps the policy layer from becoming another unbounded long-running resource cache.

## Poison handling

The internal mutex recovers the inner cache on poison rather than panicking.

## Important architecture rule

```text
control plane != dataplane
```

The cache itself is a control-plane object. The best integration is:

```text
observe/classify
-> resolve cached policy
-> host adapter stores route locally
-> hot path uses local route
-> material state change / failure
-> invalidate or re-resolve
```

Do not put a lock, runtime selector, environment lookup, or resource scan on every sub-microsecond operation.

## Relationship to future milestones

M12 will replace M11's simple pressure-only backend selection with task-specific online cost estimates.

M11.5 remains useful after M12: it becomes the cheap policy memoization layer in front of the more expensive cost/planning logic.

M14 continuous rebalancing can invalidate/revise policy leases only when observed state changes enough to justify the control-plane cost.
