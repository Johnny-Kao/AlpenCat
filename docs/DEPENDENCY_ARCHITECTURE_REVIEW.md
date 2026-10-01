# Dependency and Architecture Review

Date: 2026-10-01

Status: post-M7 simplification pass complete.

## Goal

Reduce conceptual and dependency cost without weakening the runtime boundaries established by M1-M7.

## Changes applied

### 1. Removed selector wrapper state

Before:

~~~text
select(...)
-> Selection { backend }
~~~

After:

~~~text
select(...)
-> BackendKind
~~~

Reason:

\`Selection\` contained only one field and enforced no invariant.

Also removed the unused transitional \`select_v0()\` function.

### 2. Collapsed Runtime constructors into RuntimeConfig

Before:

~~~text
Runtime::new()
Runtime::with_calibration(...)
Runtime::with_execution_budget(...)
Runtime::with_calibration_and_budget(...)
Runtime::with_external_parallelism(...)
Runtime::with_budget_and_external_parallelism(...)
~~~

After:

~~~text
Runtime::new()
Runtime::with_config(RuntimeConfig { ... })
~~~

\`RuntimeConfig\` owns:

~~~text
CalibrationProfile
ExecutionBudget
ExternalParallelism
~~~

Reason:

Combination constructors grow combinatorially as configuration expands.

One canonical configuration representation is easier to reason about and serialize later.

### 3. Reduced wgpu feature surface

wgpu 30.0.1 default features included backends/features not required by the current desktop runtime target.

Removed:

~~~text
GLES
WebGPU
parking_lot feature
~~~

Retained:

~~~text
std
DX12
Metal
Vulkan
WGSL
~~~

Dependency-tree measurement:

~~~text
workspace normal dependency tree:
174 lines before review
164 lines after review

runtime-gpu-wgpu normal dependency tree:
152 lines before review
142 lines after review
~~~

These line counts are a coarse cargo-tree complexity indicator, not a package-quality metric.

### 4. Kept the ordered-float direct pin temporarily

\`ordered-float = 5.3.0\` is not a logical runtime dependency.

It currently exists as an MSRV guard because unconstrained wgpu 30.0.1 resolution previously selected \`ordered-float 5.5.0\`, which required Rust 1.90 while the project validates against Rust 1.87.

Decision:

~~~text
KEEP TEMPORARILY
~~~

Removal condition:

- dependency resolution on a fresh lockfile remains Rust-1.87 compatible without the direct pin.

## Crate boundary review

Current crates:

~~~text
runtime-api
runtime-core
runtime-selector
runtime-cpu-rayon
runtime-gpu-wgpu
~~~

### runtime-api — KEEP

Owns:

- caller-facing runtime lifecycle;
- task definitions;
- runtime configuration;
- execution diagnostics;
- backend orchestration/fallback.

### runtime-core — KEEP

Although small, it prevents backend adapters from depending on the public API crate.

Owns backend-neutral primitives:

- \`BackendKind\`;
- \`WorkRange\`;
- \`ExecutionBudget\`.

Removing it would either create dependency cycles or force adapters to depend on caller-facing API types.

### runtime-selector — KEEP

Owns pure backend selection policy and calibration thresholds.

It intentionally has no Rayon/wgpu knowledge.

This boundary is useful because calibration/selection can evolve without changing backend adapters.

### runtime-cpu-rayon — KEEP

Owns all Rayon-specific behavior:

- limited pools;
- nested-Rayon detection;
- CPU execution details.

The public API does not expose Rayon types.

### runtime-gpu-wgpu — KEEP

Owns all wgpu-specific behavior:

- adapter/device/queue;
- buffers;
- WGSL;
- dispatch;
- readback.

The public API does not expose wgpu types.

## Dependencies retained

### rayon

Status:

~~~text
KEEP
~~~

Reason:

- mature CPU scheduler;
- work stealing;
- nested worker detection;
- thin adapter;
- materially less risk than implementing a thread pool.

### wgpu

Status:

~~~text
KEEP
~~~

Reason:

- portable GPU abstraction;
- Metal/Vulkan/DX12 coverage;
- backend types remain isolated;
- avoids maintaining vendor-specific GPU stacks.

### pollster

Status:

~~~text
KEEP FOR SYNCHRONOUS v0
~~~

Reason:

The public v0 runtime currently exposes synchronous initialization/execution.

Revisit if the runtime becomes natively async.

### bytemuck

Status:

~~~text
KEEP
~~~

Reason:

Safe, narrow buffer-byte conversion utility used at the GPU boundary.

## Architecture debt still present

### 1. Generic task/backend registration is now implemented

The public `submit_vector_scale(...)` special case has been removed.

Current caller-facing model:

~~~text
TaskDefinition
+ WorkRange
+ RangeTaskImplementations
    - serial/CPU element implementation
    - optional GPU implementation
-> Runtime::submit_range_task
~~~

GPU execution is registered through `GpuExecutionContext`, which keeps wgpu types behind `runtime-gpu-wgpu`.

Remaining limitation:

The current generic task shape is still range-oriented and the GPU context currently exposes an f32 compute primitive. Broader data-shape/backend contracts should be added only when another real task requires them.

### 2. CPU limited Rayon pool is rebuilt per call

Current M7 behavior creates a limited Rayon pool for each non-nested CPU execution.

Correctness is acceptable, but repeated construction adds overhead.

Possible next step:

~~~text
cache pools by max_parallelism
~~~

Only implement after measurement; do not add caching complexity speculatively.

### 3. Calibration persistence is not implemented

Current calibration is supplied in memory.

Future persistence needs an environment identity before calibration records can safely be reused.

### 4. GPU pipeline/shader objects are rebuilt per call

The wgpu proof implementation currently recreates shader/pipeline resources for each \`vector_scale\`.

This is intentionally simple but materially inflates GPU steady-state cost.

A future GPU executor should cache reusable pipelines/resources.

### 5. GPU residency is not modeled

All GPU work currently assumes host input and readback output.

A mature selector must account for:

- already-resident GPU data;
- transfer avoidance;
- multi-operation reuse.

## Rejected simplifications

### Merge runtime-core into runtime-api

Rejected.

It saves one small crate but weakens dependency direction and risks cycles.

### Merge runtime-selector into runtime-api

Rejected.

Selection/calibration is a distinct policy concern and should remain testable without backend execution dependencies.

### Replace Rayon with custom threads

Rejected.

This reduces one dependency but greatly increases scheduler complexity and maintenance risk.

### Replace wgpu with direct Metal/Vulkan/DX12 bindings

Rejected.

This reduces abstraction dependency at the cost of multiplying backend-specific implementation and portability work.

## Current target architecture

~~~text
TaskDefinition
  + task-specific calibration
  + backend implementations

RuntimeConfig
  + execution budget
  + external parallelism
  + default calibration

Runtime
  -> selector
  -> serial
  -> Rayon CPU adapter
  -> wgpu GPU adapter
  -> observable fallback/constraint diagnostics
~~~

## Provenance gate

This review does not replace the canonical dependency-provenance system.

Machine-readable source of truth:

~~~text
upstream-dependencies.toml
~~~

Maintenance policy:

~~~text
docs/UPSTREAM_DEPENDENCY_MAP.md
~~~

Rules retained:

- dependency entries are tombstoned, not deleted;
- every external runtime has one local wrapper owner;
- removal requires a documented replacement/equivalence path;
- compatibility pins such as ordered-float remain tracked dependencies;
- Cargo.lock records what resolved, while the dependency map records why the dependency exists.

No dependency was removed in this pass, so no provenance tombstone was required.

The wgpu entry was updated to record the reduced feature surface and its upgrade check.

## Review conclusion

The current five-crate split is small but justified.

Further simplification should now target:

1. repeated runtime setup cost;
2. GPU pipeline/resource reuse;
3. broader task/data contracts only when demanded by another real workload;

not the crate boundaries themselves.

The operation-specific caller API and first task/backend registration layer are no longer open debt.

No paid GitHub runner or cloud execution was used in this review.
