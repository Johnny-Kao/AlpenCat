<h1 align="center">
  <img src="https://raw.githubusercontent.com/Johnny-Kao/AlpenCat/main/branding/alpencat-logo.png" width="800" alt="AlpenCat">
</h1>
<br>

[![Status: Experimental](https://img.shields.io/badge/status-experimental-orange)](./CURRENT_RESEARCH_DIRECTION.md)
[![Research Direction](https://img.shields.io/badge/research-boundary--validity-blue)](./CURRENT_RESEARCH_DIRECTION.md)
[![Rust](https://img.shields.io/badge/Rust-1.87%2B-000000?logo=rust)](./Cargo.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green)](#license)

> **Experimental research project — direction updated 2026-10-04**
>
> AlpenCat remains open while its architecture is being reduced through
> measurement and falsification. Earlier adaptive-runtime work is preserved, but
> it is no longer the current architectural commitment.

AlpenCat is investigating a narrow systems question:

> **How little runtime information is sufficient to keep an execution boundary
> valid when compute is constrained?**

The current focus is not a general scheduler, continuous telemetry engine, or
online performance predictor.

It is an **ultra-thin boundary-validity runtime**.

## Current idea

Applications and libraries may already have multiple equivalent paths:

```text
serial CPU
parallel CPU
GPU / accelerator
specialized fast path
generic fallback
```

The expensive mistake is not necessarily lacking another predictor. It may be
continuing to trust a crossover boundary after the execution environment has
changed.

The current AlpenCat hypothesis is:

```text
published boundary
        |
platform / OS state transition
        |
mark boundary stale
        |
do nothing until relevant demand arrives
        |
call reaches old crossover region
        |
localized revalidation
        |
publish new boundary
```

No global model is required in this design.

## Candidate minimal core

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

The desired hot path remains tiny. Platform adapters should consume state that
the kernel, OS, hardware, or driver already maintains rather than rebuilding the
same information inside AlpenCat.

Potential transition sources under study include:

| Environment | Candidate source |
| --- | --- |
| Linux | PSI events |
| ARM/Linux | hardware-capacity / hw-pressure mechanisms where available |
| Android | thermal/headroom callbacks |
| NVIDIA | throttle / limiting state |
| Apple | thermal-state notification |

These sources are not assumed to be complete or equivalent. AlpenCat only needs
a small common invalidation contract above them.

## Primary target now

The immediate research target is **compute-constrained x86-64 server
execution**.

The strongest near-term question is:

> When effective CPU capacity changes — fewer cores, CPU quota, co-tenant
> contention, or related pressure — how much does the serial/parallel execution
> boundary move, and can a tiny stale-bit + localized-revalidation mechanism
> recover the lost performance at lower cost than continuous adaptation?

Hosted GitHub Actions are being used for the algorithmic constrained-compute
layer. Physical Intel/AMD hardware will be required later for vendor-specific
thermal/frequency/capacity evidence.

## What changed from the earlier design

Earlier AlpenCat versions explored:

```text
observe
→ estimate
→ plan
→ choose
→ execute
→ measure
→ learn
```

That work produced useful evidence, but several directions have since been
weakened or falsified as default mechanisms:

- complex global prediction;
- periodic active probing;
- selected-route timing as a complete validity signal;
- eager recalibration;
- standard PSI as a complete immediate detector;
- raw application-cgroup PSI as a clean external-pressure detector.

The project is now intentionally trying to **remove** control-plane machinery,
not add more.

Historical modules such as telemetry, cost models, planners, brokers, and
rebalancers remain in the repository as research assets and baselines.

## Research principle

AlpenCat is allowed to falsify itself.

If static routing is sufficient in the intended target regime, that is a valid
result. If the correct design is only a few state bits around a tiny FastRoute,
that is preferable to preserving a larger architecture.

The goal is the smallest mechanism supported by evidence.

## Current documentation

Start here:

- [Current research direction](./CURRENT_RESEARCH_DIRECTION.md)
- [Historical v0.1 architecture](./docs/HISTORICAL_ARCHITECTURE_V0_1.md)

Historical validation and milestone documents remain under `docs/` for
traceability.

## Project status

AlpenCat is experimental and not recommended for production-critical workloads.

APIs, modules, and even the surviving architecture may change while the current
hypothesis is tested.

The repository remains public specifically so that benchmarks, negative results,
architectural reductions, and portability evidence remain inspectable.

## Contributing

Useful contributions currently include:

- reproducible constrained x86-64 server measurements;
- Intel / AMD physical-hardware evidence;
- CPU quota / cgroup / contention experiments;
- crossover-movement datasets;
- low-overhead OS or hardware state-transition mechanisms;
- falsification of the current boundary-validity hypothesis.

A contribution does not need to make AlpenCat more complex. Evidence that lets
the project remove machinery is equally valuable.

## License

AlpenCat is dual-licensed under either:

- [MIT License](./LICENSE-MIT)
- [Apache License 2.0](./LICENSE-APACHE)

The Cargo license expression is `MIT OR Apache-2.0`.
