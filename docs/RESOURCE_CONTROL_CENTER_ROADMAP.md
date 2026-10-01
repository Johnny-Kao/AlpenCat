# Universal Resource Control Center Roadmap

## North star

Build a lightweight, independent execution/resource-control infrastructure that can be inserted at a bottleneck or project entry point, prove itself quickly, and eventually become the local execution infrastructure.

The runtime should be able to:

- discover the machine it is running on;
- observe current resource pressure dynamically;
- decompose eligible work into the smallest profitable independent units;
- schedule work across serial / CPU / GPU resources;
- change allocation while the workload is running;
- learn from observed throughput instead of relying only on static thresholds;
- remain cheap enough to insert into low-level projects;
- isolate upstream dependencies behind thin wrappers;
- progressively replace borrowed implementation wheels only when justified.

Legacy implementations belong to adopting projects, not to this runtime core.

During migration, an adopting project may keep its existing path in parallel with this runtime until confidence is sufficient. Long term, the runtime may become the project's execution infrastructure.

## Design principle

The control loop is:

    observe
    -> plan
    -> dispatch profitable chunks
    -> measure
    -> rebalance
    -> repeat

The scheduler must not assume that CPU/GPU availability is static for the duration of a task.

GPU availability, CPU pressure, memory pressure, queue latency, and backend throughput may change while work is in flight.

Instantaneous utilization is only one signal. Prefer measured completion latency, queue depth, throughput, memory pressure, failures, and recent history.

## Architecture target

    Application / library
            |
            v
      Integration adapter
            |
            v
    +-----------------------+
    | Resource Control Core |
    +-----------------------+
       |               |
       v               v
    Machine        Runtime
    Profile        Telemetry
       \             /
        \           /
         v         v
          Cost Model
              |
              v
        Dynamic Planner
              |
              v
          Work Queue
        /     |      \
       v      v       v
    Serial   CPU     GPU
       \      |      /
        \     |     /
          feedback
              |
              v
       Online calibration

## Phase 0 — Execution substrate

Status: COMPLETE for current prototype scope.

Implemented:

- serial/reference execution;
- Rayon CPU backend;
- wgpu GPU backend;
- generic task/backend registration;
- execution budgets;
- nested-parallelism protection;
- host-declared external-parallelism constraint;
- selector skeleton;
- task-specific calibration;
- CPU pool reuse;
- GPU pipeline reuse;
- explicit fallback/diagnostics;
- upstream reconciliation ledger;
- final fmt/clippy/test/doc QC.

This phase provides engines and safe backend wrappers. It is not yet the adaptive resource-control system.

## M8 — MachineProfile / Capability Discovery

Status: BASELINE COMPLETE (2026-10-01).

Implemented baseline:

- lightweight host snapshot with OS / architecture / logical CPU count;
- Linux total/available memory discovery without new external dependencies;
- explicit GPU capability probe returning backend-neutral device name/backend/type;
- partial/unknown capability fields are allowed;
- GPU probing is explicit rather than part of the cheap host snapshot.

Goal:

    know what resources exist before deciding how to use them

Minimum profile:

- OS / architecture;
- logical CPU parallelism;
- memory total / available when discoverable;
- GPU availability;
- GPU backend/device identity when discoverable;
- capability flags required by registered backends.

Properties:

- cheap to snapshot;
- backend-neutral;
- partial information allowed;
- no expensive benchmark during ordinary discovery;
- no process-global mutation.

Deliverable:

    MachineProfile
    CapabilitySnapshot

### Deferred M8 follow-up — Configuration / forced override layer

Automatic discovery remains the default, but later the runtime should support an explicit configuration file or programmatic override for environments where discovery is unavailable, unreliable, intentionally restricted, or deployment policy requires fixed values.

Target precedence:

    explicit override
    -> discovered value
    -> safe unknown / zero-capability fallback

Important semantic rule:

- the planner may treat an unavailable capacity as zero usable capacity for safety;
- the profile layer should still preserve whether a value was truly observed or merely unavailable, so "unknown" is not silently confused with a physically measured zero;
- overrides may force CPU parallelism, memory limits, GPU enable/disable, selected device/backend, chunk bounds, and related capability limits;
- invalid overrides must fail validation rather than silently creating an unsafe execution plan;
- configuration loading must stay optional and lightweight; the runtime must continue to work with no config file present.

This is intentionally deferred until the dynamic telemetry/planner model is stable enough to define the override surface cleanly.

## M9 — RuntimeTelemetry

Status: BASELINE COMPLETE (2026-10-01).

Implemented baseline:

- runtime-owned per-backend in-flight counts;
- completed/failed execution counters;
- cumulative work and elapsed time;
- latest execution throughput signal;
- lightweight ResourceSnapshot combining current HostProfile with runtime telemetry;
- failed GPU attempts are observable;
- no background monitoring thread or vendor telemetry dependency.

Detailed record:

    docs/RUNTIME_TELEMETRY_2026-10-01.md

Goal:

    know what resources are actually available now

Signals:

- observed CPU task latency / throughput;
- CPU queue pressure;
- active runtime workers;
- GPU submission latency;
- GPU completion latency;
- GPU failures/device loss;
- memory pressure where safely observable;
- recent backend throughput.

Do not require privileged or vendor-specific monitoring to function.

Telemetry should degrade gracefully when signals are unavailable.

## M10 — WorkUnit + Profitable Chunk Planner

Status: BASELINE COMPLETE (2026-10-01).

Implemented baseline:

- backend-neutral WorkUnit / WorkPlan / ChunkPolicy;
- balanced decomposition that avoids tiny tail chunks;
- stable unit IDs;
- complete, contiguous, non-overlapping range coverage;
- WorkQueue claim/requeue primitives for future dynamic reassignment;
- Runtime::plan_work(...) public planning entry point;
- no performance-derived chunk thresholds promoted from the currently loaded development machine.

Detailed record:

    docs/WORKUNIT_CHUNK_PLANNER_2026-10-01.md

Goal:

    decompose work into independent units small enough to rebalance,
    but large enough that scheduling overhead is justified

Need:

- stable task identity;
- range/work-unit representation;
- minimum profitable chunk estimate;
- backend-specific chunk floor/ceiling;
- bounded outstanding work;
- ordered result reconstruction where required.

Rule:

    smallest profitable independent unit
    != smallest possible unit

## M11 — Dynamic Resource Broker

Status: BASELINE COMPLETE (2026-10-01).

Implemented baseline:

- ResourceBroker / BrokerCapacity / BrokerRequest / WorkAssignment;
- per-claim backend selection from current runtime telemetry;
- normalized CPU/GPU in-flight pressure comparison;
- no claim when all eligible capacity is full;
- returned units can be requeued;
- GPU-ineligible work never routes to GPU;
- range-aware GPU implementation registration for future chunk-level GPU execution;
- Runtime::claim_next_work(...) integrates live telemetry with the broker.

Detailed record:

    docs/DYNAMIC_RESOURCE_BROKER_2026-10-01.md

Goal:

    continuously allocate future chunks according to current state

Behavior:

- CPU/GPU allocations can change during one logical task;
- newly idle resources may steal future chunks;
- degraded resources receive fewer future chunks;
- already-running chunks are normally allowed to finish;
- memory pressure can reduce concurrency/chunk size;
- resource loss must not corrupt task state.

## M11.5 — Cached Execution Policy Layer

Status: BASELINE IMPLEMENTED (2026-10-01).

Evidence from the CFFI #282 integration showed that a resource-control decision on every nanosecond-scale call is too fine-grained. The runtime therefore now supports a cached control-plane policy for ultra-hot/tiny work while preserving M11 dynamic brokerage for larger workloads.

Implemented baseline:

- bounded execution-policy cache;
- task identity + logarithmic work-size class keys;
- quantized CPU/GPU pressure fingerprint;
- automatic re-plan after material pressure/capacity/eligibility changes;
- backend failure counters invalidate prior fingerprints;
- explicit per-task invalidation and full cache clear;
- poison-tolerant cache locking;
- public `Runtime::cached_execution_policy(...)` API;
- host adapters are expected to store the resolved route at their own task/signature boundary rather than re-entering the runtime on every dataplane operation.

Detailed record:

    docs/CACHED_EXECUTION_POLICY_2026-10-01.md

Architecture:

    tiny / ultra-hot work
    -> cached direct policy

    medium / large work
    -> dynamic broker / adaptive planner

    materially changed state
    -> re-plan / refresh cache

This milestone does not replace M11. It prevents the control plane from becoming the dataplane bottleneck.

## M12 — Online Cost Model

Status: BASELINE IMPLEMENTED (2026-10-01).

Implemented baseline:

- bounded task/backend/work-size cost history;
- exponential moving average of measured execution cost;
- task history preferred over global runtime telemetry;
- backend failure penalty and confidence reduction;
- current in-flight pressure incorporated into predicted cost;
- automatic machine capability input;
- GPU vendor/device identity preserved from discovery;
- zero-tuning bootstrap when no observations exist;
- M11.5 policy-cache misses now resolve through M12;
- Runtime task execution automatically contributes cost observations;
- explicit observation/reset APIs for external adapters.

Detailed record:

    docs/ONLINE_COST_MODEL_2026-10-01.md

Goal:

    replace static backend thresholds with measured environment-specific economics

Inputs:

- task identity;
- work size;
- machine profile;
- current telemetry;
- backend throughput history;
- transfer/setup cost;
- memory footprint;
- recent failures.

Outputs:

- predicted backend cost;
- predicted chunk size;
- predicted parallelism;
- confidence.

Calibration should be incremental and cheap after bootstrap.

## M13 — Adaptive Execution Planner

Status: BASELINE IMPLEMENTED (2026-10-01).

Implemented baseline:

- concrete ExecutionPlan synthesis from M12 + machine profile + live telemetry;
- automatic backend mix;
- automatic CPU parallelism;
- automatic chunk size;
- automatic max-in-flight limit;
- automatic host memory budget;
- automatic GPU device selection from discovery;
- GPU residency hint;
- confidence propagation from M12;
- Runtime::adaptive_execution_plan(...);
- Runtime::plan_adaptive_work(...) to directly create a lazy M10 WorkPlan from the automatic chunk size;
- no ordinary user tuning required.

Detailed record:

    docs/ADAPTIVE_EXECUTION_PLANNER_2026-10-01.md

Goal:

    produce a concrete execution plan, not only BackendKind

Target:

    ExecutionPlan {
        backend mix,
        cpu_parallelism,
        chunk_size,
        max_in_flight,
        memory_budget,
        gpu_device,
        residency_hint,
    }

The plan is revisable while work remains queued.

## M14 — Continuous Rebalancing

Status: BASELINE IMPLEMENTED (2026-10-01).

Implemented baseline:

- event-driven RebalanceSession rather than per-chunk full replanning;
- immediate replan eligibility on new backend failure;
- quantized CPU/GPU pressure bands;
- minimum completed-work window before ordinary replan;
- accumulated-window throughput degradation detection;
- pressure recovery can reopen planning;
- Runtime::begin_rebalancing(...);
- Runtime::rebalance_if_needed(...);
- M14 invalidates stale M11.5 policy before M12+M13 recomputation;
- zero remaining work never replans;
- already-running chunks are not interrupted.

Default internal throttling:

    min completed delta = 8
    pressure bands = 4
    sustained degradation = 25%

Detailed record:

    docs/CONTINUOUS_REBALANCING_2026-10-01.md

Goal:

    close the loop

Example:

    GPU chunks become slow
    -> reduce GPU allocation weight
    -> increase CPU share

    GPU latency recovers
    -> cautiously increase GPU share

    memory pressure increases
    -> reduce in-flight chunks

No single instantaneous utilization sample should dominate the decision.

## M15 — Integration / Migration Layer

Status: BASELINE IMPLEMENTED (2026-10-01).

Implemented baseline:

- separate runtime-integration crate outside the resource-control core;
- MigrationReadiness / MigrationDecision / MigrationPlan;
- conservative validated-work migration policy;
- Runtime::migration_plan(...) combines M13 planning and migration ownership;
- ExecutionLease prevents post-commit fallback and duplicate side effects;
- host-existing implementation remains outside the runtime core;
- unsupported/unvalidated work stays on the host during migration.

Detailed record:

    docs/INTEGRATION_MIGRATION_LAYER_2026-10-01.md

Goal:

    make adoption cheap for existing low-level projects

This is outside the resource-control core.

An adopting project may initially use:

    supported / validated work
        -> resource-control runtime

    unsupported / low-confidence work
        -> existing project implementation

This coexistence is a migration mechanism, not the target runtime architecture.

## M16 — Dependency Convergence

Status: BASELINE IMPLEMENTED (2026-10-01).

Implemented baseline:

- dependency map schema now records ownership and convergence state;
- mature runtime dependencies are explicitly retained behind wrappers rather than rewritten by default;
- temporary compatibility pins carry explicit removal conditions;
- new tools/validate_dependency_convergence.py cross-checks dependency records, Cargo.lock, wrapper paths, and the reconciliation ledger;
- dependency convergence validation is part of public CI;
- removed/replaced dependency history remains durable.

Detailed record:

    docs/DEPENDENCY_CONVERGENCE_2026-10-01.md

Current strategy:

    borrow mature wheels
    -> wrap
    -> reconcile upstream changes
    -> remove duplicate capability
    -> replace only when ownership is justified

Long-term possibility:

    fewer external runtime wheels
    -> more internally maintained primitives

Do not rewrite mature infrastructure merely to reduce dependency count.

## Validation strategy

Primary development validation:

- local/free Rust 1.87 container;
- public/free CI where appropriate;
- software Vulkan correctness path;
- deterministic unit/integration tests;
- workload-specific differential/reference tests.

Hardware validation:

- native Metal;
- physical Vulkan;
- DX12;
- different CPU/memory sizes.

Any incremental paid test requires explicit user approval before execution.

Repository visibility and release policy are separate from CI design. Keep validation reproducible without coupling architecture decisions to hosting visibility.

## Success criterion

The project succeeds when a low-level project can insert this runtime at a bottleneck and delegate execution/resource management without maintaining its own bespoke CPU/GPU scheduling fast path.

The runtime should become infrastructure only after sustained correctness, portability, observability, and performance evidence.
