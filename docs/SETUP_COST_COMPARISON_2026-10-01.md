# Repeated Setup Cost Comparison — 2026-10-01

> Local/free benchmark record. These measurements are environment-specific.

## Environment

- Rust 1.87
- release builds
- local Colima/Docker
- Rayon 1.12.0
- wgpu 30.0.1
- Mesa Vulkan software adapter
- no paid GitHub runner

## Question

Compare repeated runtime setup against reuse:

1. build a bounded Rayon pool on every call vs reuse one pool;
2. build a wgpu shader/pipeline on every call vs reuse one prepared pipeline.

The goal is to decide which setup costs are large enough to justify caching/reuse.

## Rayon bounded pool reuse

Configuration:

- 2 threads available in the benchmark container;
- 21 repetitions;
- median timing;
- operation: independent f32 scale/collect.

| Elements | New pool (µs) | Reused pool (µs) | Setup delta (µs) | New/reused |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 25.917 | 6.375 | 19.542 | 4.065x |
| 1,024 | 25.625 | 5.250 | 20.375 | 4.881x |
| 4,096 | 24.625 | 6.958 | 17.667 | 3.539x |
| 16,384 | 29.833 | 7.583 | 22.250 | 3.934x |
| 65,536 | 34.666 | 13.042 | 21.624 | 2.658x |
| 262,144 | 41.792 | 22.041 | 19.751 | 1.896x |
| 1,048,576 | 80.042 | 60.459 | 19.583 | 1.324x |

Observed pattern:

    Rayon bounded-pool construction adds about 18–22 µs per call.

This is effectively a fixed tax across the tested sizes.

Interpretation:

- small/medium CPU tasks are dominated by pool construction;
- even at 1,048,576 elements, pool reconstruction still adds roughly one-third over the reused-pool path;
- because Auto currently routes medium work toward CPU, this overhead materially distorts selector calibration.

Decision:

    bounded Rayon pool reuse is justified

Preferred design:

    cache/reuse pools by max_parallelism

while retaining:

- nested Rayon detection;
- ExecutionBudget semantics;
- external-parallelism constraints.

## wgpu pipeline reuse

Configuration:

- Mesa Vulkan software adapter;
- 11 repetitions;
- median timing;
- same vector_scale WGSL;
- both paths still allocate/upload/read back buffers every call;
- only shader/pipeline construction differs.

| Elements | Rebuild pipeline (µs) | Reused pipeline (µs) | Setup delta (µs) | Rebuild/reused |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 184.648 | 60.604 | 124.044 | 3.047x |
| 1,024 | 166.225 | 63.022 | 103.203 | 2.638x |
| 4,096 | 181.522 | 73.485 | 108.037 | 2.470x |
| 16,384 | 229.664 | 130.462 | 99.202 | 1.760x |
| 65,536 | 457.035 | 354.166 | 102.869 | 1.290x |
| 262,144 | 843.045 | 728.630 | 114.415 | 1.157x |
| 1,048,576 | 2,790.852 | 2,669.851 | 121.001 | 1.045x |

Observed pattern:

    shader/pipeline reconstruction adds about 100–124 µs per call.

This is also close to a fixed tax.

Interpretation:

- pipeline reuse is highly material for small and medium GPU submissions;
- for large workloads, host transfer / buffer creation / dispatch / readback dominate;
- at 1,048,576 elements, removing pipeline rebuild saves only about 4–5%.

Decision:

    GPU pipeline reuse is justified,
    but it is not the dominant large-workload optimization.

The next GPU cost decomposition should separately measure:

- input buffer reuse;
- params buffer reuse;
- readback buffer reuse;
- GPU residency / avoiding host round trips.

## Relative priority

Measured fixed setup taxes:

    Rayon pool rebuild:       ~20 µs/call
    wgpu pipeline rebuild:   ~100–120 µs/call

Absolute setup cost is larger on the GPU side.

However, runtime priority is:

1. Rayon pool reuse
2. wgpu pipeline reuse
3. GPU buffer/resource reuse and residency study

Reason:

- CPU is currently selected for a broad medium-workload region;
- Rayon pool construction is a large fraction of CPU execution cost across the entire tested range;
- GPU pipeline caching is worthwhile but large GPU workloads are already dominated by other costs;
- implementing GPU pipeline caching before understanding buffer/resource reuse would only remove one layer of the GPU overhead stack.

## Architecture consequence

The comparison supports reuse behind existing wrappers:

    runtime-cpu-rayon
        -> pool cache/reuse

    runtime-gpu-wgpu
        -> prepared pipeline cache/reuse

The public runtime contract does not need to change.

This preserves the upstream reconciliation model:

    upstream change
    -> wrapper adaptation
    -> stable registered-task contract

## Reconciliation note

The newly exposed adapter-level prepared-kernel primitive exists to measure and later support reuse.

It does not expose wgpu types through runtime-api.

No upstream baseline is advanced by this benchmark.
