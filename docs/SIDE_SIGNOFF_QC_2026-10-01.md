# Side / Sign-off QC — Boundary Safety & Robustness — 2026-10-01

Status: **B-BLOCKERS CLOSED — boundary safety blockers identified in this review have been fixed and revalidated. Non-blocking hardening items remain on the backlog.**

This review intentionally does not compare against an upstream/original implementation. It treats the runtime as a standalone resource-control infrastructure and asks whether its current public boundaries are safe, stable, and bounded.

## Validation executed

Rust 1.87, local/free container:

- cargo fmt --all -- --check — PASS
- cargo clippy --workspace --all-targets -- -D warnings — PASS
- cargo test --workspace --all-targets — PASS
- cargo test --workspace --release — PASS
- RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps — PASS
- reconciliation ledger validator — PASS
- runtime-planner / runtime-broker / runtime-telemetry repeated 25 consecutive rounds — PASS

Previously validated on public GitHub ubuntu-latest:

- workspace tests — PASS
- software Vulkan capability discovery — PASS
- software Vulkan registered GPU execution — PASS

No paid runner was used.

## Sign-off blockers

### B1 — Work planning has unbounded materialization

Current behavior:

    WorkRange
    -> compute unit_count
    -> Vec::with_capacity(unit_count)
    -> materialize every WorkUnit

Then:

    WorkQueue::from_plan()
    -> copies every WorkUnit into a VecDeque

Consequences:

- memory cost grows linearly with chunk count;
- very large ranges combined with small chunk policies can request enormous allocations;
- WorkPlan + WorkQueue can hold roughly two full copies of unit metadata;
- an extreme valid public input can cause allocation failure / process termination before useful execution starts.

On a 64-bit target, WorkUnit currently contains three usize-sized values (id + begin + end), approximately 24 bytes before allocator overhead.

Approximate metadata only:

    1,000,000 units:
      WorkPlan ~24 MB
      + WorkQueue copy ~24 MB
      ~= 48 MB+

    10,000,000 units:
      ~= 480 MB+

This conflicts with the project's goal of being a lightweight infrastructure layer.

Required before sign-off:

- lazy/iterator-backed planning or a bounded rolling queue;
- no O(number_of_all_chunks) metadata requirement for arbitrarily large logical work;
- explicit maximum in-flight/prefetched units.

Severity: **BLOCKER / efficiency + robustness**

### B2 — ExecutionBudget is unbounded and pool creation panics

Current public behavior:

    ExecutionBudget::new(any usize > 0)

CPU adapter:

    ThreadPoolBuilder::new()
      .num_threads(max_parallelism)
      .build()
      .expect(...)

Consequences:

- an erroneous or forced override can request an absurd thread count;
- pool construction failure becomes a panic;
- large distinct budgets can also create many persistent pools.

Required before sign-off:

- validate or clamp requested CPU parallelism against a defensible machine/runtime limit;
- pool-creation failure must return an explicit runtime error, not panic;
- later configuration overrides must use the same validation path.

Severity: **BLOCKER / safety + resource exhaustion**

### B3 — latest telemetry sample is not pair-atomic

Current latest sample:

    last_work_items: AtomicU64
    last_elapsed_nanos: AtomicU64

Concurrent completions can interleave the two stores.

A snapshot can therefore theoretically observe:

    work_items from execution A
    elapsed from execution B

and derive a false latest throughput.

This does not affect cumulative correctness, but M12 intends to feed latest/recent throughput into backend cost decisions.

Required before M12:

- store the sample coherently (mutex, seqlock/versioned snapshot, or another paired-sample mechanism);
- add concurrent writer/reader stress coverage.

Severity: **BLOCKER before online cost model**

### B4 — whole-task GPU and range-aware GPU capabilities are not distinguishable

Current registration supports both:

    with_gpu(...)
    with_gpu_range(...)

but both currently reduce to:

    gpu.is_some() == gpu_eligible

The broker can therefore not distinguish:

- a GPU implementation that can execute one WorkUnit range;
- a legacy GPU implementation that only understands the whole logical task.

Today the broker only assigns and does not execute chunks, so no incorrect result is currently produced.

However, connecting M11 to real chunk execution without fixing this can cause a whole-task GPU implementation to be invoked once per chunk.

Required before adaptive chunk execution:

- explicit GPU capability metadata, e.g. WholeTask vs RangeAware;
- broker only assigns independent GPU WorkUnits to RangeAware implementations.

Severity: **BLOCKER before M13/M14 execution integration**

## Important non-blocking hardening items

### H1 — CPU pool cache cardinality is unbounded

CpuAdapter keeps one persistent Rayon pool per distinct max_parallelism.

If callers vary budgets continuously, pools/threads remain retained for the Runtime lifetime.

Recommended:

- small bounded cache or canonicalized pool sizes;
- preferably derive a small set of reusable capacities from MachineProfile.

### H2 — GPU pipeline cache cardinality is unbounded

Key:

    full WGSL source String + workgroup size

Every distinct dynamic shader source can permanently grow the per-runtime cache.

Recommended:

- bounded LRU / explicit cache policy;
- or make cache ownership/configuration visible.

### H3 — internal mutex poisoning still panics

CPU pool cache and GPU kernel cache use expect() after Mutex::lock().

This is acceptable for the current experimental prototype but is not suitable for hardened infrastructure.

Recommended:

- explicit poisoned-cache recovery or RuntimeError.

### H4 — telemetry counters are wrapping atomics

Long-lived cumulative counters use fetch_add and eventually wrap at u64/usize boundaries.

Not a practical short-term risk, but infrastructure intended for very long uptime should define rollover/saturation semantics.

### H5 — ChunkPolicy silently normalizes invalid values

Examples:

    min = 0 -> 1
    max < min -> max = min
    target outside bounds -> clamp

This is convenient for bootstrap code, but future user/config overrides were explicitly intended to reject invalid unsafe values.

Recommended:

- keep a normalized internal constructor if useful;
- provide a validated external/config constructor returning an error.

## What passed cleanly

No failure was found in the currently tested ordinary operating surface:

- Serial / CPU / GPU fallback semantics;
- nested Rayon safety behavior;
- external parallelism constraint;
- zero execution budget normalization;
- zero broker capacity;
- GPU-ineligible routing;
- all-capacity-full broker behavior;
- claim/requeue semantics;
- balanced chunk coverage;
- empty/reversed ranges treated as empty;
- Linux machine profile discovery;
- failed GPU attempt telemetry;
- Debug and Release behavior;
- repeated planner/broker/telemetry runs.

No unsafe blocks were found in the runtime crates.

## Empirical resource-limit validation

The two highest-risk resource-limit blockers were validated in isolated Docker containers so the host M5 could not be destabilized.

### Planner memory pressure

Environment:

    1 CPU
    256 MB RAM
    256 MB swap limit
    release build

Results with fixed chunk size = 1:

    1,000,000 units   PASS
    4,000,000 units   PASS
    8,000,000 units   PASS
    10,000,000 units  PASS
    12,000,000 units  EXIT 137 / OOM kill

This confirms B1 is an actual bounded-resource failure, not only a theoretical concern.

### Rayon thread-budget pressure

Environment:

    1 CPU
    256 MB RAM
    PID/thread limit = 128

Results:

    ExecutionBudget 2     PASS
    ExecutionBudget 32    PASS
    ExecutionBudget 96    PASS
    ExecutionBudget 128   PANIC
    ExecutionBudget 256   PANIC
    ExecutionBudget 1024  PANIC

Failure:

    ThreadPoolBuildError
    Resource temporarily unavailable

The panic originates from the current expect("valid Rayon execution budget") path.

### Priority / exclusivity hypothesis

The failing cases were repeated with the process raised to highest nice priority available in the isolated container while remaining pinned to one CPU.

Results:

    planner 12,000,000 units  EXIT 137 / OOM kill
    ExecutionBudget 128       PANIC

Therefore CPU scheduling priority / exclusivity does not remove either blocker.

Reason:

- CPU priority affects scheduling order, not address-space / memory availability;
- PID/thread limits are hard resource limits and are unaffected by scheduler priority;
- neither failing path uses the GPU, so GPU priority cannot affect these failures.

This means the correct fix must be structural:

- bound/lazily materialize planner metadata;
- validate CPU parallelism;
- make pool creation failure recoverable rather than panicking.

## B-blocker remediation and closure

All four sign-off blockers were fixed and revalidated after the initial HOLD decision.

### B1 CLOSED — lazy O(1) work planning

Old behavior:

    WorkPlan -> Vec<WorkUnit> for every chunk
    WorkQueue -> second copy in VecDeque

New behavior:

    WorkPlan {
        source,
        unit_count,
        base_items,
        remainder,
    }

    WorkPlan::unit(id)
    WorkPlan::iter()
    WorkQueue -> lazy next_id + only explicitly requeued units

The logical number of WorkUnits no longer determines baseline planner metadata allocation.

Resource-limit regression in the same 256 MB / 1 CPU container:

    12,000,000 logical units       PASS
    1,000,000,000 logical units    PASS
    1,000,000,000,000 logical units PASS

All completed with exit 0.

Additional boundary coverage:

- near-usize::MAX range remains contiguous;
- first/last lazy unit reconstruction validated;
- pending_count remains O(1);
- queue claim/requeue ordering preserved.

### B2 CLOSED — bounded CPU parallelism and non-panic pool failure

Requested CPU parallelism is now capped by:

    std::thread::available_parallelism()

An absurd request such as:

    ExecutionBudget::new(usize::MAX)

cannot cause an equally absurd Rayon thread request.

Pool construction no longer uses expect(). If the OS refuses pool construction, CPU execution falls back to serial and reports:

    CpuExecutionKind::SerialResourceLimited
    ExecutionConstraint::ResourceLimited

Original constrained regression:

    1 CPU
    PID limit 128
    requested 128 / 256 / 1024 / usize::MAX

All now exit 0 and safely normalize to serial execution.

Explicit pool-build-failure regression:

    4 visible CPUs
    PID limit 2 / 3 / 4
    requested budget 4

All now exit 0 with SerialResourceLimited instead of panicking.

Mutex poison handling in the CPU pool cache was also changed from expect() to recovery of the inner cache.

### B3 CLOSED — coherent latest telemetry sample

Latest execution telemetry is now stored as one paired sample:

    LatestSample {
        work_items,
        elapsed_nanos,
    }

protected by a small Mutex.

The snapshot therefore cannot combine work_items from execution A with elapsed_nanos from execution B.

Concurrent stress coverage:

- two concurrent writers repeatedly publish distinct known pairs;
- a reader performs 20,000 snapshots;
- every observed pair must be either a complete A sample, complete B sample, or initial zero sample.

PASS.

### B4 CLOSED — explicit GPU work granularity

GPU registration now records:

    GpuWorkGranularity::WholeTask
    GpuWorkGranularity::RangeAware

Semantics:

    with_gpu(...)       -> WholeTask
    with_gpu_range(...) -> RangeAware

The chunk broker request was also renamed and tightened to:

    gpu_range_eligible

rather than generic gpu_eligible.

This prevents the broker contract from implicitly treating a whole-task GPU implementation as independently chunkable.

Coverage confirms both registration modes are distinguishable and only range-aware work is represented as chunk-GPU eligible.

## Post-fix Sign-off validation

Rust 1.87 local/free container:

    cargo fmt --all -- --check                         PASS
    cargo clippy --workspace --all-targets -- -D warnings PASS
    cargo test --workspace --all-targets                PASS
    cargo test --workspace --release                    PASS
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps PASS
    reconciliation ledger validator                     PASS

Repeated stability:

    runtime-planner
    runtime-broker
    runtime-telemetry

25 consecutive rounds: PASS.

Current assessment:

    functional regression confidence: HIGH
    ordinary-path stability: HIGH
    B1 boundary memory safety: CLOSED
    B2 CPU resource-limit panic: CLOSED
    B3 telemetry latest-sample coherence: CLOSED
    B4 GPU granularity ambiguity: CLOSED
    M12 readiness: CLEAR
    M13/M14 blocker status from this QC: CLEAR

The H-series hardening items remain useful follow-up work but are not blockers from this QC pass.


## QC decision

Current state:

    functional regression confidence: HIGH
    ordinary-path stability: HIGH
    tested extreme-boundary safety: SIGNED OFF
    B-series blocker status: CLOSED
    M12 readiness: CLEAR
    M13/M14 blocker status from this QC: CLEAR

Remaining non-blocking hardening backlog:

    H1 bounded/canonicalized CPU pool cache policy
    H2 bounded GPU pipeline cache policy
    H3 remaining internal panic/poison hardening outside the closed B2 path
    H4 telemetry cumulative-counter rollover semantics
    H5 validated config-facing ChunkPolicy constructor

These items should be addressed before calling the runtime production-hardened, but they do not reopen the B-series sign-off blockers validated above.
