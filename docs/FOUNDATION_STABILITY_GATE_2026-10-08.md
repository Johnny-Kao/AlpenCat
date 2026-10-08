# Foundation Stability Gate — 2026-10-08

> Status: REQUIRED before any platform/vendor specialization
> Specialization: BLOCKED until PASS

## Why

Previous evidence was fragmented across old robustness, economics, and hot-path runs. That is not sufficient to claim the current branch foundation is stable.

This gate validates the current branch as one coherent version.

## Hard requirements

The foundation is considered stable only if all of the following pass together:

1. 14/14 cross-platform correctness jobs:
   - 7 runner classes;
   - 2 independent allocations each.
2. PublishedBoundary never exposes torn snapshots under stress.
3. ResourceEpoch preserves stale/fresh semantics.
4. FastRoute matches the reference model under randomized routing.
5. Fresh revalidation performs zero measurements and publishes nothing.
6. Invalidation during measurement cannot publish a fresh boundary.
7. Route-unavailable and no-local-crossover cases never extrapolate global state.
8. CPU execution-budget invariants pass.
9. Current production hot path shows no gross regression against the previously validated production range.

## Hot-path regression ceilings

These are safety/regression ceilings, not optimization targets:

- epoch-match incremental cost <= 2.0 ns/call;
- stale-far incremental cost <= 3.0 ns/call.

Historical validated maxima were approximately:

- epoch-match: 0.614 ns/call;
- stale-far: 1.123 ns/call.

The wider ceilings intentionally allow runner noise while rejecting large regressions.

## Decision rule

```
all correctness jobs pass
AND all hot-path jobs pass
    -> FOUNDATION PASS
else
    -> FOUNDATION FAIL
    -> fix foundation before specialization
```

No Intel/AMD/Apple specialization work should proceed on FAIL.
