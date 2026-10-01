# M12 — Online Cost Model

Status: BASELINE IMPLEMENTED (2026-10-01).

## Goal

Make backend economics machine-specific and self-learning without requiring the user to choose CPU/GPU thresholds.

The runtime now combines:

- discovered host capability;
- discovered GPU identity/backend/vendor/device type;
- runtime telemetry;
- task identity;
- logarithmic work-size class;
- per-task/backend execution history;
- backend failures;
- current in-flight pressure.

The model learns from actual execution observations and returns:

- predicted backend cost when evidence exists;
- selected backend;
- confidence;
- estimate source: task history, runtime telemetry, or bootstrap.

## Zero-tuning principle

Ordinary callers do not provide performance thresholds.

Bootstrap behavior is safe and conservative. As successful observations accumulate, the model uses an exponential moving average of measured nanoseconds per item.

If a backend begins failing, its learned cost is penalized and confidence falls.

## Machine awareness

Host discovery already supplies:

- logical CPU count;
- total host memory when available;
- available host memory when available;
- OS / architecture.

GPU discovery now also preserves:

- device name;
- backend;
- device type;
- PCI/vendor ID when reported by wgpu;
- device ID when reported by wgpu;
- vendor classification for NVIDIA / AMD / Intel / Apple / Other.

`dedicated_memory_bytes` is optional.

Important: wgpu's buffer limits are not VRAM capacity. The runtime does not fabricate a VRAM value from those limits. Exact dedicated GPU memory remains unknown unless a trustworthy platform/backend source provides it.

## Learning model

History key:

```text
task identity
+ backend
+ logarithmic work-size class
```

Current baseline tracks:

- sample count;
- successful samples;
- failed samples;
- EMA nanoseconds per item;
- failure penalty;
- confidence.

Task-specific history is preferred over global runtime telemetry.

Global runtime telemetry is used as a weaker fallback when task history is not yet available.

If neither exists, the existing safe calibration profile acts only as bootstrap.

## M11.5 integration

M11.5 now memoizes M12 decisions.

```text
policy cache hit
-> return cached route

policy cache miss / resource fingerprint changed
-> M12 estimates backend economics
-> cache chosen backend
-> adapter stores route at its task/signature boundary
```

This preserves the CFFI benchmark lesson: do not re-enter the control plane for every nanosecond-scale dataplane operation.

## Automatic decision and observation

`ExecutionMode::Auto` now asks M12 for the backend decision at the task boundary. With no history, M12 reproduces the safe bootstrap behavior. As task-specific measurements accumulate, later Auto submissions can change backend without user intervention.

`Runtime::submit_range_task(...)` records task-aware cost observations for Serial, CPU and GPU attempts.

The public API also exposes:

```rust
Runtime::record_cost_observation(...)
Runtime::cost_model_decision(...)
Runtime::reset_task_learning(...)
```

The explicit observation API is intended for host adapters whose work executes outside the built-in task wrappers.

## What M12 does not do yet

M12 estimates economics and selects a backend policy.

It does not yet produce the full multi-backend execution plan with chunk size, max in-flight work, memory budget, or GPU residency strategy. That belongs to M13 Adaptive Execution Planner.

M12 therefore supplies evidence; M13 turns that evidence into a concrete execution plan.

## Safety / boundedness

- model storage is bounded;
- poisoned mutexes recover their inner state;
- backend availability constrains candidate selection;
- no physical GPU is assumed when discovery fails;
- no user tuning is required for ordinary operation;
- bootstrap confidence is explicitly zero until measurements exist.


## M12 v2 — Parametric machine-aware model

Status: IMPLEMENTED (2026-10-01).

The original M12 baseline used a per-size-class EMA of nanoseconds per item. That was useful for proving the feedback path, but it could not separate fixed scheduling/setup cost from scalable compute cost.

M12 v2 now models successful executions as:

```text
T(work) = setup + per_item * work + transfer
```

### Regression instead of fixed thresholds

For each:

```text
task identity
+ backend
+ machine fingerprint
```

the runtime accumulates multiple real execution sizes.

With enough size diversity it fits:

```text
setup_nanos
per_item_nanos
```

using an online linear regression summary.

If observations do not yet span enough sizes, it falls back to the existing EMA behavior rather than pretending the intercept is known.

This means CPU/GPU crossover can emerge from observed machine behavior instead of being encoded as a fixed work-size threshold.

### Machine fingerprint

A stable runtime fingerprint is derived from:

- OS / architecture;
- logical CPU count;
- total host memory;
- GPU name/backend/device type;
- GPU vendor ID;
- GPU device ID;
- reliable dedicated-memory value when one exists.

Available/free memory is deliberately excluded from the identity because it is dynamic pressure, not machine identity.

The fingerprint separates learned models across materially different machines/devices.

No GPU model marketing table or hard-coded RTX/Radeon performance score is used.

### Transfer cost hook

`CostObservation` can optionally report:

```text
transfer_bytes
transfer_elapsed
```

When supplied by an adapter, M12 learns an empirical transfer nanoseconds/byte term and can include it in cost prediction.

The normal observation API remains valid for tasks where transfer information is not observable.

### Runtime integration

Built-in Serial/CPU/GPU range-task observations are now tagged with the automatically discovered machine fingerprint.

External adapters can provide richer observations through:

```rust
Runtime::record_detailed_cost_observation(...)
```

This allows host integrations that know exact H2D/D2H transfer sizes and times to feed those measurements into M12 without exposing tuning parameters to users.

### Design principle

The model now follows the same broad principle used by mature heterogeneous runtimes:

```text
hardware capability
+ cheap real observations
+ parametric task model
+ online correction
```

rather than:

```text
hardware model name
-> hard-coded benchmark score
-> permanent threshold
```

This makes the runtime naturally adapt as CPUs, GPUs, drivers, and machine configurations change.


## M12 v2.1 — Local residuals, uncertainty, and replan hysteresis

Status: IMPLEMENTED and locally validated (2026-10-02).

The global M12 v2 model remains:

```text
T(work) = setup + per_item * work + transfer
```

M12 v2.1 deliberately does not replace that model. It adds three lightweight corrections for crossover regions where two backends have similar predicted completion times.

### 1. Local work-size residual

Each task/backend/machine model keeps a logarithmic work-size bucket EMA of observed total execution time.

Prediction becomes conceptually:

```text
global parametric estimate
    +
bounded local size-class correction
```

The local observation is blended rather than replacing the global regression. Its maximum influence is capped, so a noisy bucket cannot fully override the broader model.

Purpose:

- preserve global extrapolation;
- correct repeatable local non-linearity;
- improve decisions near CPU/GPU crossover;
- avoid polynomial or hardware-specific models.

### 2. Uncertainty confidence

When the best and second-best predicted backends are within 10%, the backend ranking is treated as uncertain.

The selected backend remains the predicted best backend on a first decision, but confidence is reduced in proportion to the gap.

This prevents M13 from treating a near-tie as a high-confidence mixed-backend plan.

### 3. Replan hysteresis

Initial planning remains unbiased.

During an M14-triggered replan only, the current backend is passed to M12 as a preference. If that backend is still within the 10% uncertainty band of the new best estimate, it remains selected.

If another backend is more than 10% better, the preference is ignored and the runtime switches.

This avoids oscillation around a crossover without creating hidden global state.

### Apple M5 FIR evidence

A free local benchmark on Apple M5 used the same FIR shape that motivated the original `upfirdn` execution research.

Measured full-path p50 examples:

```text
n=16K, h64:   CPU 109.54 us, GPU 410.83 us
n=32K, h256:  CPU 716.58 us, GPU 690.42 us
n=65K, h64:   CPU 331.71 us, GPU 503.42 us
n=65K, h128:  CPU 665.12 us, GPU 750.25 us
n=65K, h256:  CPU 1517.54 us, GPU 875.46 us
n=262K, h64:  CPU 1168.54 us, GPU 1023.38 us
n=1M, h128:   CPU 9398.08 us, GPU 4288.12 us
n=1M, h256:   CPU 20095.00 us, GPU 7615.96 us
```

The observed crossover moves earlier as FIR tap count increases, confirming that a single hard-coded work-size threshold is inappropriate.

Numerical comparison between CPU and Metal paths remained small:

```text
max absolute difference: approximately 4e-9 to 6e-9
```

### Validation

Final local Rust 1.87 QC on commit series ending in `7cffc7b`:

- `cargo fmt --all -- --check`: PASS
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS
- `cargo test --workspace`: PASS
- all doctests: PASS
- `cargo test -p runtime-cost-model --release`: 11/11 PASS
- reconciliation validator: PASS
- dependency convergence validator: PASS
- `git diff --check`: PASS

New regression coverage includes:

- local bucket correction of a global-fit miss;
- preserving the current backend inside the uncertainty band;
- switching despite hysteresis when another backend is materially faster.

A free GitHub real FIR adaptive benchmark also passed on a 4-logical-CPU runner with exact output and optimal Serial/CPU backend selection across the tested h64/h128 workload matrix.

### Cost impact

The adaptive-control benchmark after adding local residual logic remained sub-microsecond:

```text
M12 decision: approximately 0.46 us
M13 plan: approximately 0.09 us
M14 keep/trigger checks: approximately 0.04–0.05 us
```

The additional correction therefore remains appropriate for the control plane and does not justify introducing a heavier model.
