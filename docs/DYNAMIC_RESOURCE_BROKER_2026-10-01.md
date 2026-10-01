# Dynamic Resource Broker Baseline — 2026-10-01

## Goal

Allocate the next pending WorkUnit using current runtime pressure, so future chunks can move between CPU and GPU while one logical task remains in progress.

This milestone introduces the broker decision boundary. It does not yet run a background worker pool or perform continuous concurrent dispatch.

## Implemented

New crate:

    runtime-broker

Core types:

    BrokerCapacity {
        cpu_slots,
        gpu_slots,
    }

    BrokerRequest {
        gpu_eligible,
    }

    WorkAssignment {
        unit,
        backend,
    }

    ResourceBroker

Runtime integration:

    Runtime::claim_next_work(...)

The runtime passes its live RuntimeTelemetrySnapshot into the broker for every claim.

## Current decision rule

The broker uses normalized in-flight pressure only.

If both CPU and GPU are available:

    cpu_pressure = cpu.in_flight / cpu_slots
    gpu_pressure = gpu.in_flight / gpu_slots

The less-loaded eligible backend receives the next WorkUnit.

If pressures are equal, CPU is the conservative bootstrap default.

If all eligible capacity is full, no unit is claimed.

This means pending work remains available instead of being prematurely bound to a busy resource.

## Dynamic behavior enabled

Because backend choice occurs at claim time rather than at whole-task submission time:

- CPU can receive one unit;
- later GPU can receive another unit if CPU pressure rises;
- GPU-full conditions redirect future units to CPU;
- a returned/failed assignment can be requeued;
- GPU-ineligible work never goes to GPU.

This is the first real resource-broker layer.

## Range-aware GPU contract

The registered GPU implementation contract now supports:

    with_gpu_range(|gpu, WorkRange| ...)

The existing:

    with_gpu(|gpu| ...)

remains supported and is wrapped as a full-range implementation for backward compatibility.

Range awareness is required before GPU chunks can be independently assigned by the broker.

## Scope boundary

M11 baseline does not yet:

- create concurrent broker workers;
- preempt an already-running chunk;
- predict execution cost;
- use measured throughput to prefer a backend;
- change chunk size;
- use memory footprint estimates;
- implement continuous background rebalancing.

Those concerns belong to M12-M14.

## Validation

    cargo fmt --all -- --check                         PASS
    cargo clippy --workspace --all-targets -- -D warnings PASS
    cargo test --workspace                             PASS
    runtime-broker unit tests                          6 PASS
    runtime-api broker integration                     PASS
    existing GPU/CPU/selector tests                    PASS
    reconciliation ledger validator                    PASS

No paid runner was used.

## Next

M12: Online Cost Model.

The next step is to combine:

    MachineProfile
    + RuntimeTelemetry
    + task/work size
    + setup/transfer cost
    + recent failures

to estimate which eligible backend should receive future WorkUnits.
