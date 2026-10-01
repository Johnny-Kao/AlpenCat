# M13 — Adaptive Execution Planner

Status: BASELINE IMPLEMENTED (2026-10-01).

## Goal

Turn M12 backend economics into a concrete execution plan without asking ordinary users to tune execution parameters.

The planner consumes:

- work size;
- M12 cost estimates and confidence;
- current machine profile;
- current runtime telemetry;
- GPU range eligibility.

It returns:

```text
ExecutionPlan {
    primary_backend,
    backend_mix,
    cpu_parallelism,
    chunk_size,
    max_in_flight,
    memory_budget_bytes,
    gpu_device,
    residency_hint,
    confidence_milli,
}
```

## Zero-tuning behavior

No ordinary caller needs to supply:

- CPU core count;
- chunk size;
- max in-flight work;
- CPU/GPU split;
- memory budget;
- GPU device choice;
- GPU residency preference.

The runtime derives these from the machine and learned costs.

## Backend mix

If M12 confidence is still low or fewer than two backends have comparable cost estimates, M13 keeps a single-backend plan.

Once comparable estimates exist, M13 can produce a weighted backend mix.

Backends predicted at more than 2x the best backend cost are excluded from the mix.

This prevents clearly poor resources from receiving work while preserving multiple viable resources for later M14 rebalancing.

## CPU parallelism

CPU parallelism starts from automatically discovered logical CPU capacity and is reduced by current runtime CPU in-flight work.

M13 never requires the caller to specify a core count for normal operation.

## Chunk size

Chunk size is derived from:

- total work;
- currently useful execution lanes;
- backend mix;
- memory pressure.

The planner targets multiple chunks per active lane so future M14 rebalancing remains possible.

As host memory pressure rises, chunk size decreases automatically.

The resulting chunk size can be passed directly into the existing lazy M10 WorkPlan.

## Max in-flight

M13 derives maximum in-flight work from:

- usable CPU lanes;
- GPU participation;
- Serial participation;
- already in-flight runtime work;
- memory pressure.

At very high memory pressure, the baseline contracts to one in-flight unit.

## Memory budget

When host memory information is available, M13 assigns a conservative working memory budget from both total and currently available memory.

This is a runtime safety budget, not a claim that all available host memory belongs to the runtime.

Unknown memory remains `None`; callers are not required to provide a value.

## GPU device and residency

If GPU work is part of the plan, M13 records the automatically discovered GPU device.

Residency hints:

```text
None
PreferDevice
KeepResident
```

A confident, large GPU-dominant plan may request `KeepResident`.

High host memory pressure disables the residency hint.

Exact VRAM remains optional/unknown unless discovered from a trustworthy source.

## Runtime API

```rust
Runtime::adaptive_execution_plan(...)
Runtime::plan_adaptive_work(...)
```

The second API returns both:

- the adaptive ExecutionPlan;
- a lazy WorkPlan generated from M13's automatic chunk size.

## Relationship to M14

M13 produces a snapshot plan from the current evidence.

It is intentionally safe to recompute the plan while work remains queued.

M14 will close the loop:

```text
execute some chunks
-> observe new latency / failures / pressure
-> call M12 + M13 again
-> revise future chunk allocation
```

M13 therefore supplies the revisable plan object; M14 owns continuous rebalancing.
