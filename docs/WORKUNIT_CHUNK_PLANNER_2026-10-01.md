# WorkUnit / Profitable Chunk Planner Baseline — 2026-10-01

## Goal

Create backend-neutral work units that are small enough to reassign dynamically, but large enough to avoid pathological scheduling overhead.

This milestone defines the decomposition and queue semantics only. It does not yet choose CPU vs GPU.

## Implemented

New crate:

    runtime-planner

Core types:

    ChunkPolicy {
        target_items,
        min_items,
        max_items,
    }

    WorkUnit {
        id,
        range,
    }

    WorkPlan {
        source,
        chunk_items,
        units,
    }

    WorkQueue

Runtime API:

    Runtime::plan_work(range, policy)

## Balanced decomposition

The planner does not naively emit fixed-size chunks plus an arbitrarily tiny tail.

Instead it chooses a unit count near the target size and balances work across all units.

Properties:

- complete coverage of the source range;
- no overlap;
- stable monotonically increasing WorkUnit IDs;
- all units are contiguous;
- when the source is large enough, units remain inside min/max bounds;
- unit sizes differ by at most one item in a balanced plan;
- work smaller than the minimum stays as one unit.

This avoids creating tiny tail chunks that would become scheduling noise.

## Reassignment foundation

WorkQueue supports:

    claim_next()
    requeue_front(unit)
    requeue_back(unit)

This allows future brokers to:

- claim pending work;
- return work after a backend becomes unsuitable;
- prioritize an interrupted/retry unit;
- move future work between CPU and GPU without changing logical task identity.

The queue is deliberately not concurrent yet. M11 owns broker/concurrency semantics.

## Important boundary

M10 does not try to learn the best chunk size from the currently busy development machine.

The default chunk policy is only a structural bootstrap.

Later M12 online cost modeling can change target/min/max policy using measured runtime telemetry.

## Validation

    cargo fmt --all -- --check                         PASS
    cargo clippy --workspace --all-targets -- -D warnings PASS
    cargo test --workspace                             PASS
    runtime-planner unit tests                         7 PASS
    runtime-api planner integration test               PASS
    reconciliation ledger validator                    PASS

No paid runner was used.

## Next

M11: Dynamic Resource Broker.

The broker will consume pending WorkUnits and make backend allocation decisions repeatedly while work remains queued.
