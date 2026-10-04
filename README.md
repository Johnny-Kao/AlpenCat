<h1 align="center">
  <img src="https://raw.githubusercontent.com/Johnny-Kao/AlpenCat/main/branding/alpencat-logo.png" width="800" alt="AlpenCat">
</h1>
<br>

[![Status: Experimental](https://img.shields.io/badge/status-experimental-orange)](./docs/ARCHITECTURE.md)
[![Architecture: Converged](https://img.shields.io/badge/architecture-converged-2ea44f)](./docs/ARCHITECTURE.md)
[![Validation: Native hardware](https://img.shields.io/badge/validation-native%20hardware-blue)](./docs/NATIVE_VALIDATION.md)
[![Rust](https://img.shields.io/badge/Rust-1.87%2B-000000?logo=rust)](./Cargo.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green)](#license)

> **Experimental systems research. Architecture converged; native hardware validation remains open.**

AlpenCat is a lightweight runtime mechanism for keeping an execution boundary valid when available compute changes.

It is not a general scheduler, continuous telemetry engine, or online performance predictor.

## Core idea

Applications often already have equivalent execution paths. A calibrated crossover can later become wrong when CPU availability, contention, thermal state, VM scheduling, or another resource condition changes.

AlpenCat keeps the control path small:

```text
native platform transition
        |
        v
ResourceEpoch++
        |
        v
published Boundary becomes stale
        |
        v
FastRoute continues using the old boundary
        |
        v
future near-boundary demand
        |
        v
bounded revalidation when justified
        |
        v
publish updated Boundary
```

The active runtime now consists of six crates:

```text
runtime-core        Boundary, PublishedBoundary, ResourceEpoch
runtime-selector    FastRoute
runtime-api         small public execution surface
runtime-cpu-rayon   optional CPU backend
runtime-gpu-wgpu    optional GPU backend
runtime-machine     lightweight capability discovery
```

Earlier telemetry, cost-model, planner, broker, policy-cache, rebalancer, migration, CFFI, and adaptive-control implementations have been removed from the active tree.

They remain recoverable from Git history and the snapshot branch:

```text
archive/pre-runtime-pruning-2026-10-04
```

## What the experiments established

- Static execution boundaries can become materially wrong.
- A resource transition does **not** imply immediate recalibration.
- Unbounded crossover search is economically poor when a route disappears.
- Bounded local revalidation sharply reduces pathological search cost.
- A universal fixed slowdown threshold does not classify all capacity-loss regimes.
- Native transition semantics should be primary; routed-call slowdown is secondary evidence.
- Earlier 8-runner x64 measurements showed the relaxed epoch-check mechanism can be extremely small in absolute cost.

See [Validation Status](./docs/VALIDATION_STATUS.md).

## Current validation phase

The remaining native integration gate is:

```text
real hardware / OS event
        |
        v
thin platform adapter
        |
        v
ResourceEpoch++
        |
        v
stale boundary
        |
        v
bounded runtime response
```

Intel Hardware Feedback Interface (HFI) is the leading first target.

For hardware vendors, server OEMs, platform teams, and research labs:

- [Native validation protocol](./docs/NATIVE_VALIDATION.md)
- [One-command passive validation probe](./tools/native-validation/)

The probe does not modify BIOS, BMC, SST, MSRs, power limits, or thermal settings.

## Active evidence

The repository intentionally keeps only the evidence needed for the converged design:

- `benchmarks/x64-boundary-economics/`
- `experiments/x64-boundary-economics/`
- `benchmarks/x64-hotpath-overhead/`
- `tools/native-validation/`

The hot-path benchmark now imports the production `runtime-core` / `runtime-selector` implementation directly. A fresh production-code matrix run is the next overhead checkpoint.

## Project status

Completed or substantially converged:

- boundary-validity problem;
- lazy invalidation;
- bounded revalidation;
- revalidation economics;
- heterogeneous x64 mechanism testing;
- control-policy convergence;
- production-core pruning.

Still open:

- real native hardware event delivery;
- adapter -> `ResourceEpoch` end-to-end validation;
- production-code hot-path matrix;
- physical-server end-to-end workload validation.

AlpenCat remains experimental and is not recommended for production-critical workloads.

## License

AlpenCat is dual-licensed under either:

- [MIT License](./LICENSE-MIT)
- [Apache License 2.0](./LICENSE-APACHE)

The Cargo license expression is `MIT OR Apache-2.0`.
