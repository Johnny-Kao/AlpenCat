# Final Architecture QC and Package Readiness — 2026-10-01

Status: implementation architecture is internally coherent and test-clean.

## Validation

Executed with Rust 1.87:

- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets -- -D warnings
- cargo test --workspace
- cargo doc --workspace --no-deps
- reconciliation ledger validator

Result:

    PASS

Source scan:

    TODO / FIXME / XXX: none
    todo! / unimplemented!: none

## Architecture state

Implemented:

- serial/reference fallback;
- Rayon CPU backend;
- wgpu GPU backend;
- backend selector;
- task-specific calibration;
- explicit execution budget;
- nested-Rayon protection;
- host-declared external parallelism constraint;
- generic registered range-task model;
- observable fallback/constraint diagnostics;
- per-runtime bounded Rayon pool reuse;
- per-runtime wgpu pipeline reuse;
- upstream U0/U1/D/M reconciliation ledger;
- cross-source integration mapping.

Measured and intentionally not promoted:

- generic GPU buffer cache;
- software-Vulkan-derived hardware thresholds.

## Dependency/wrapper boundaries

Current external runtime ownership remains thin:

    Rayon
      -> runtime-cpu-rayon

    wgpu / bytemuck / pollster
      -> runtime-gpu-wgpu

    selector/core/public contract
      -> no upstream runtime-specific types

Upstream reconciliation records remain mandatory for future dedup, override,
replacement, or vendoring work.

## Performance cleanup completed

Measured fixed setup costs that were worth removing:

    bounded Rayon pool rebuild
      ~18–22 us/call
      -> removed through per-adapter pool reuse

    wgpu pipeline rebuild
      ~100–124 us/call
      -> removed through per-adapter pipeline cache

Measured resource reuse that was not worth generalizing:

    fresh GPU buffers vs resident buffers
      ~1–4% improvement in current Mesa Vulkan environment
      -> no generic buffer cache promoted

## Remaining technical boundaries

### 1. Physical GPU validation

Still not validated on:

- native Apple Metal hardware;
- physical discrete/integrated Vulkan GPU;
- DX12 hardware.

Current software Vulkan results prove execution/correctness paths but are not
hardware performance evidence.

Any validation that may incur incremental cost requires explicit user approval.

### 2. Hardware calibration

The current bootstrap selector thresholds remain placeholders unless replaced by
task/environment-specific calibration.

Real hardware calibration is needed before making GPU crossover claims.

### 3. GPU residency across multiple tasks

The current registered-task contract materializes host results per task.

A future multi-operation GPU-resident flow would require an explicit data
lifetime / ownership contract.

This is a separate architecture feature, not a hidden buffer-cache optimization.

### 4. External runtime adapters

OpenMP / MKL / OpenBLAS / Python thread-pool coordination remains optional
host-layer interoperability work.

The core intentionally does not mutate arbitrary external thread-pool state.

### 5. Production error hardening

Internal wrapper invariants still use expect() for:

- poisoned internal cache mutexes;
- Rayon pool creation with validated budgets.

This is acceptable for the current experimental prototype but should be
reviewed before treating the crates as a stable library.

## Package publication state

The workspace is now explicitly:

    rust-version = 1.87
    publish = false

This prevents accidental crates.io publication while names, licensing,
repository metadata, stability guarantees, and public API policy are still
internal decisions.

The project is therefore:

    implementation-complete for the current experimental v0.1 scope
    not yet stable-package release ready

## Recommended next state

Do not continue adding optimizations without a new measured need.

The next meaningful branches are:

1. physical hardware validation and calibration;
2. introduce a second real workload to test whether the generic contract holds;
3. design GPU-resident multi-task data ownership only if a workload requires it;
4. complete packaging and API-stability work before a stable release.

Until one of those is selected, this implementation remains at an experimental
v0.1 checkpoint.
