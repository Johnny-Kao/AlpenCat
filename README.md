<h1 align="center">
  <img src="https://raw.githubusercontent.com/Johnny-Kao/AlpenCat/main/branding/alpencat-logo.png" width="800" alt="AlpenCat">
</h1>
<br>

[![Status: Experimental](https://img.shields.io/badge/status-experimental-orange)](./docs/ARCHITECTURE.md)
[![Architecture: Converged](https://img.shields.io/badge/architecture-converged-2ea44f)](./docs/ARCHITECTURE.md)
[![Validation: Native hardware](https://img.shields.io/badge/validation-native%20hardware-blue)](./docs/NATIVE_VALIDATION.md)
[![Rust](https://img.shields.io/badge/Rust-1.87%2B-000000?logo=rust)](./Cargo.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green)](#license)

> **Experimental systems research — architecture converged, native integration validation in progress.**

AlpenCat is a lightweight runtime mechanism for keeping an execution boundary valid when available compute changes.

It is intentionally **not** a general scheduler, continuous telemetry engine, or online performance predictor.

## The problem

Applications and libraries often already have equivalent execution paths:

```text
serial CPU
parallel CPU
GPU / accelerator
specialized fast path
generic fallback
```

The crossover between two paths may be calibrated correctly and later become wrong when the execution environment changes.

Examples include:

- effective CPU count changes;
- CPU quota or cpuset changes;
- co-tenant contention;
- thermal or power-capability changes;
- VM steal / preemption;
- accelerator throttling.

The central AlpenCat question is:

> **How little runtime machinery is needed to keep a previously calibrated execution boundary trustworthy?**

## Current architecture

The architecture has converged through measurement and falsification to a small control path:

```text
native platform transition
        |
        v
ResourceEpoch / stale
        |
        v
continue using the old boundary
        |
        v
future demand reaches the old boundary region
        |
        v
bounded local revalidation, only when justified
        |
        v
publish a new boundary
```

Candidate production core:

```text
FastRoute
+ Boundary
+ ResourceEpoch / stale
+ native transition semantics
+ near-boundary demand
+ bounded revalidation
+ conservative fallback
```

Normal routing remains close to:

```text
load epoch
compare cached epoch
compare workload key with boundary
select route
```

No continuous telemetry or global performance model is required on the hot path.

## What the experiments established

The current evidence supports several reductions:

- **Static boundaries can become materially wrong.**
  Under strong x64 contention, stale routing produced very large regret.
- **A resource transition does not imply immediate recalibration.**
  Mild capacity changes often moved the crossover little or not at all.
- **Unbounded recalibration is too expensive.**
  Bounded local revalidation reduced pathological search cost substantially.
- **A single slowdown threshold is not a universal capacity classifier.**
  Multi-runner Intel/AMD measurements showed overlap between mild and severe CPU-budget cases.
- **Slowdown remains useful as a secondary accelerator.**
  Strong external contention was consistently visible across the heterogeneous runner matrix.
- **The normal epoch/stale hot path is extremely small.**
  The measured incremental cost of a relaxed epoch check was approximately **0.016–0.514 ns/call** across eight independent x64 runners.

Detailed evidence and limitations are recorded in [Validation Status](./docs/VALIDATION_STATUS.md).

## Heterogeneous x64 validation

The current constrained-compute experiments have run across independently allocated Intel and AMD GitHub-hosted x64 systems, including:

- Intel Xeon Platinum 8370C / 8573C-class runners observed during the study;
- AMD EPYC 7763;
- AMD EPYC 9V45;
- AMD EPYC 9V74.

These experiments validate the **mechanism and economics** of the boundary-validity design.

They do **not** yet establish vendor-native event delivery. That is the current validation phase.

## Native validation phase

The remaining architecture gate is:

```text
real hardware / OS capacity event
        |
        v
platform adapter
        |
        v
AlpenCat ResourceEpoch changes
        |
        v
bounded runtime response
```

Intel Hardware Feedback Interface (HFI) is the leading first target because Linux can relay CPU performance / efficiency capability changes to userspace through thermal Generic Netlink when `CONFIG_INTEL_HFI_THERMAL` is enabled.

For hardware vendors, server OEMs, research labs, and platform teams:

- [Native validation protocol](./docs/NATIVE_VALIDATION.md)
- [One-command validation probe](./tools/native-validation/)

The probe is deliberately independent from the AlpenCat runtime. A partner can run it on suitable physical hardware and return the generated evidence bundle without integrating AlpenCat first.

## Research history

Earlier AlpenCat work explored a much broader runtime architecture:

```text
observe -> estimate -> plan -> choose -> execute -> measure -> learn
```

That work was useful for falsification, but the current project is deliberately removing machinery rather than preserving it.

Historical telemetry, planners, cost models, brokers, rebalancers, CPU/GPU experiments, and earlier architecture records remain in the repository for traceability.

See:

- [Architecture and research direction](./docs/ARCHITECTURE.md)
- [Validation status](./docs/VALIDATION_STATUS.md)
- [Historical v0.1 architecture](./docs/HISTORICAL_ARCHITECTURE_V0_1.md)

## Reproducible benchmarks

The repository includes the x64 experiments used to test the converged architecture:

- `benchmarks/x64-boundary-economics/`
- `experiments/x64-boundary-economics/`
- `benchmarks/x64-hotpath-overhead/`

The benchmarks are evidence tools, not production APIs.

## Project status

**Architecture exploration is effectively complete. Native integration validation remains open.**

AlpenCat is still experimental and is not recommended for production-critical workloads.

The next milestone is not a larger scheduler. It is a verified native adapter path and an implementation-level end-to-end measurement on physical hardware.

## Contributing / validation partners

High-value contributions now include:

- physical Intel HFI measurements;
- AMD / KVM / cpuset native transition evidence;
- server-OEM reproduction;
- hardware/OS event delivery latency;
- native-adapter correctness;
- final hot-path measurements against exact production code;
- evidence that falsifies or further simplifies the current design.

Evidence that lets AlpenCat remove logic is considered a successful contribution.

## License

AlpenCat is dual-licensed under either:

- [MIT License](./LICENSE-MIT)
- [Apache License 2.0](./LICENSE-APACHE)

The Cargo license expression is `MIT OR Apache-2.0`.
