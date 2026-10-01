# Runtime Telemetry Baseline — 2026-10-01

## Goal

Provide a lightweight, runtime-owned observation layer that can feed future adaptive scheduling without requiring privileged OS counters or vendor-specific GPU APIs.

## Implemented

New crate:

    runtime-telemetry

Per backend (Serial / CPU / GPU), the runtime now tracks:

- in-flight execution count;
- completed executions;
- failed executions;
- cumulative work items;
- cumulative elapsed nanoseconds;
- last execution work items;
- last execution elapsed nanoseconds;
- derived cumulative items/second;
- derived latest items/second.

Execution paths use RAII observations so in-flight state becomes visible at execution start and is cleared on completion or drop.

The runtime API now exposes:

    Runtime::telemetry_snapshot()
    Runtime::resource_snapshot()

ResourceSnapshot combines:

    current lightweight HostProfile
    + RuntimeTelemetrySnapshot

On Linux this means current available memory can be sampled together with current runtime-owned execution pressure.

## Scope boundary

This baseline intentionally does not depend on:

- NVIDIA/AMD vendor telemetry;
- privileged OS counters;
- instantaneous GPU utilization;
- global process mutation;
- a background monitoring thread.

The scheduler should first learn from the work it actually dispatches.

External machine signals may be added later as optional inputs.

## Important semantics

Telemetry records the actual backend that performed work.

Examples:

- CPU requests constrained to serial are recorded as Serial work;
- failed GPU attempts increment GPU failure telemetry;
- successful GPU execution contributes GPU throughput;
- external-parallelism fallback contributes Serial telemetry.

Extremely short durations are normalized to at least 1 ns so throughput calculations remain defined.

## Validation

    cargo fmt --all -- --check                         PASS
    cargo clippy --workspace --all-targets -- -D warnings PASS
    cargo test --workspace                             PASS
    runtime-telemetry unit tests                       PASS
    runtime-api telemetry integration tests            PASS
    forced Vulkan GPU execution regression             PASS
    reconciliation ledger validator                    PASS

No paid runner was used.

## Deferred M9 extensions

Useful later if M10-M14 require them:

- short rolling/EMA throughput windows beyond the latest sample;
- explicit queued-work counters;
- process/system CPU pressure;
- vendor-specific GPU queue/utilization hints;
- device-loss generation counters;
- configurable telemetry sampling/export.

These should remain optional so the core stays lightweight.

## Next

M10: WorkUnit + Profitable Chunk Planner.

The scheduler needs work small enough to rebalance dynamically, but not so small that dispatch/queue overhead dominates.
