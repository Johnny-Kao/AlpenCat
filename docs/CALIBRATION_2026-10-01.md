# Calibration Record — 2026-10-01

> Experimental local calibration record.
>
> These measurements are environment-specific and must not be treated as universal thresholds.

## Environment

Validation used the existing local Colima/Docker environment with:

- Rust 1.87
- release builds
- Rayon 1.12.0
- wgpu 30.0.1
- Mesa Vulkan software adapter
- 7 repetitions per size
- median timing

No paid GitHub runner or cloud GPU was used.

## Task

Logical task:

```text
vector_scale
y[i] = alpha * x[i]
```

The measurements are end-to-end from framework submission through completed result materialization.

## End-to-end measurements

| Elements | Serial (µs) | Rayon CPU (µs) | Vulkan software GPU (µs) |
| ---: | ---: | ---: | ---: |
| 256 | 0.042 | 9.754 | 216.539 |
| 1,024 | 0.084 | 3.709 | 193.489 |
| 4,096 | 0.208 | 4.877 | 205.702 |
| 16,384 | 0.792 | 10.712 | 230.752 |
| 65,536 | 4.293 | 19.632 | 343.002 |
| 262,144 | 16.172 | 51.728 | 810.552 |
| 1,048,576 | 64.941 | 195.198 | 2,797.415 |
| 4,194,304 | 288.774 | 761.700 | 19,729.627 |

Observed crossover:

```text
serial -> Rayon CPU: not observed
Rayon CPU -> software GPU: not observed
```

## GPU cost decomposition

Measured at 1,048,576 f32 elements:

```text
GPU adapter initialization:
18,397.408 µs

first vector_scale after initialization:
13,054.930 µs

steady-state pure buffer round-trip:
347.824 µs

steady-state vector_scale:
2,776.424 µs
```

The round-trip measurement includes:

```text
host upload
-> GPU buffer copy
-> readback
-> map
```

The vector-scale measurement includes:

```text
host upload
-> WGSL compute dispatch
-> readback
-> map
```

## Important interpretation

This task is extremely light per element.

The serial implementation is likely benefiting from compiler optimization/vectorization while Rayon pays task-splitting and collection overhead.

Therefore:

> Parallel CPU execution is not automatically profitable merely because the input is large.

Likewise, the Mesa Vulkan software adapter is useful for correctness and timing-pipeline validation but is not representative of a physical GPU.

Therefore:

> The software-GPU measurements must not be used as the hardware GPU threshold.

## Architecture consequence

Calibration must be associated with:

```text
task identity
+
execution environment
```

not with one global workload-size table.

The runtime now supports:

```text
Runtime default CalibrationProfile
TaskDefinition-specific CalibrationProfile override
```

The task-specific profile takes precedence over the runtime default.

## GPU dispatch bug found during calibration

The original M4 GPU adapter dispatched all workgroups along X.

At 4,194,304 elements this exceeded the per-dimension dispatch limit:

```text
65,536 workgroups > 65,535 allowed
```

Calibration exposed the issue.

The adapter was changed to use a 2D workgroup layout while preserving linear indexing in WGSL.

The 4,194,304-element workload now executes successfully.

## Current selector policy

The M5 bootstrap thresholds remain available as fallback defaults:

```text
Serial <= 1,024
CPU <= 262,144
GPU above that when eligible
```

They remain explicitly **bootstrap defaults**, not calibrated performance claims.

A measured task/environment profile can replace them through:

```text
Runtime::with_config(RuntimeConfig { calibration, ..Default::default() })
TaskDefinition::with_calibration(...)
```

## Current calibration decision

For the measured `vector_scale` + local software-GPU environment, no profitable parallel crossover was observed.

No permanent threshold is promoted from this run.

Real hardware calibration should be performed separately for:

- Apple Metal
- discrete/integrated Vulkan GPUs
- DX12 systems

Any paid runner must require explicit user approval before execution.
