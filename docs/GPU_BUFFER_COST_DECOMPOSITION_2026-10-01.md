# GPU Buffer and Residency Cost Decomposition — 2026-10-01

> Local/free measurement record using Mesa Vulkan software execution.
> These numbers are environment-specific and are not hardware-GPU defaults.

## Question

After removing repeated pipeline construction, determine whether the next useful optimization is:

1. buffer allocation/bind-group reuse;
2. host -> GPU upload reduction;
3. GPU -> host readback reduction;
4. keeping data resident across multiple GPU operations.

## Environment

- Rust 1.87
- release build
- wgpu 30.0.1
- Mesa Vulkan software adapter
- 9 repetitions
- median timing
- no paid runner

## Direct comparison

Both paths reuse the same prepared compute pipeline.

Fresh-buffer path:

    cached pipeline
    -> allocate data/params/readback buffers
    -> create bind group
    -> upload
    -> compute
    -> copy/readback/map

Resident-buffer path:

    cached pipeline
    -> reuse data/params/readback buffers + bind group
    -> upload
    -> compute
    -> copy/readback/map

| Elements | Fresh buffers (µs) | Resident buffers (µs) | Speedup |
| ---: | ---: | ---: | ---: |
| 4,096 | 92.805 | 89.004 | 1.043x |
| 65,536 | 364.581 | 358.608 | 1.017x |
| 1,048,576 | 2,699.582 | 2,659.277 | 1.015x |

## Interpretation

Buffer allocation + bind-group creation is not a major cost in this environment.

Observed benefit from resource reuse:

    ~1–4%

This does not justify adding a general buffer cache to the runtime now.

## Stage decomposition

The resident path was also measured with explicit synchronization between stages.

| Elements | Upload (µs) | Compute/wait (µs) | Readback/map (µs) |
| ---: | ---: | ---: | ---: |
| 4,096 | 31.868 | 78.061 | 35.084 |
| 65,536 | 37.340 | 335.552 | 39.678 |
| 1,048,576 | 172.914 | 2,339.262 | 185.443 |

The stage timings include extra synchronization and therefore should not be summed as an exact replacement for the single-submit end-to-end measurement.

They are useful for relative cost decomposition.

At 1,048,576 elements:

    upload       ~173 µs
    compute      ~2339 µs
    readback     ~185 µs

On the software Vulkan adapter, compute dominates.

## Residency implication

If multiple GPU operations can consume the same resident data without returning to the host between each operation, host transfer/readback can potentially be avoided.

For the measured 1,048,576-element case, upload + readback are roughly:

    ~358 µs

This is approximately 13% of the resident end-to-end time.

This is not a hardware-GPU performance claim.

On a physical GPU, the relative balance between compute and PCIe/unified-memory transfer may differ substantially.

## Decision

Do not implement a generic buffer cache now.

Reason:

- direct buffer/bind-group reuse only produced ~1–4% improvement;
- complexity would increase lifetime/aliasing/capacity management;
- current evidence does not justify that complexity.

Instead preserve the architectural capability for future resident execution.

Priority:

    1. keep current pipeline cache
    2. do not add generic buffer cache
    3. future: support multi-operation GPU residency when a real workload needs it
    4. validate residency economics on physical Metal/Vulkan/DX12 hardware before changing selector policy

## Architecture consequence

The current registered-task API still materializes host results per task.

A future GPU-resident task chain would require an explicit data-lifetime contract rather than silently caching buffers behind the existing API.

That should be treated as a separate architecture milestone, not as a hidden optimization.

## Reconciliation consequence

The resident-buffer probe remains inside runtime-gpu-wgpu and does not expose wgpu types through runtime-api.

No upstream baseline is advanced by this measurement.

No paid runner was used.
