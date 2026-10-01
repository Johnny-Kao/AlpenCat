# External Runtime Interoperability Review

Status: design boundary accepted after M7.

## Decision

The Rust core runtime must **not** directly mutate arbitrary external OpenMP / BLAS thread-pool state.

Reason:

- external libraries expose different control semantics;
- some controls are current-thread scoped;
- some are process-wide;
- some settings affect unrelated concurrent callers;
- a generic "set threads = N" abstraction would hide unsafe side effects.

The framework therefore separates:

```text
internal execution control
from
external runtime coordination
```

## Classification

### 1. Internal Rayon

Action:

```text
DETECT + LIMIT
```

Already implemented in M7.

The runtime owns this pool and may safely:

- enforce `ExecutionBudget`;
- detect nested Rayon execution;
- collapse nested execution to serial;
- report the constraint.

### 2. OpenMP

Action:

```text
DETECT WHEN AN OPENMP ADAPTER IS PRESENT
DO NOT LINK OR MUTATE OPENMP FROM CORE BY DEFAULT
```

OpenMP exposes runtime information such as whether execution is inside a parallel region.

However:

- the core runtime is not currently linked against a specific OpenMP runtime;
- multiple OpenMP implementations can coexist in scientific Python processes;
- introducing a hard OpenMP dependency would reduce portability.

Policy:

> OpenMP detection belongs in an optional interoperability adapter, not in runtime-core.

### 3. Intel oneMKL

Action:

```text
OPTIONAL THREAD-LOCAL COORDINATION
NO PROCESS-GLOBAL CONTROL FROM CORE
```

oneMKL provides both global and thread-local controls.

The preferred integration principle is:

> if direct MKL coordination is ever added, prefer thread-local control around the owned call scope.

Process-global mutation is not acceptable as the default framework behavior.

### 4. OpenBLAS

Action:

```text
DETECT / DELEGATE
DO NOT GENERICALLY MUTATE FROM CORE
```

OpenBLAS control semantics depend on build/runtime mode.

Some configurations expose process-wide controls, so changing them can affect unrelated threads.

Policy:

> delegate OpenBLAS coordination to a host-specific interoperability layer.

### 5. BLIS / FlexiBLAS / other BLAS runtimes

Action:

```text
DELEGATE
```

No generic control is added to the Rust core.

### 6. Python scientific stack

Action:

```text
HOST-LEVEL ADAPTER
```

For Python embedding, the preferred architecture is:

```text
Python host
  -> threadpoolctl-style introspection / limiting
      -> runtime framework
          -> Rayon / wgpu
```

This lets the Python layer coordinate already-loaded native libraries without making the Rust runtime depend on Python or on specific BLAS vendors.

## Public interoperability contract

The runtime may receive an external execution-context hint:

```text
ExternalParallelism {
    active: bool
}
```

Meaning:

- `active = false`: no external parallel context is known;
- `active = true`: the caller declares that surrounding work is already parallel.

When `active = true`:

- Auto CPU execution should conservatively avoid creating additional internal parallelism;
- forced CPU execution may still be allowed, but must respect the declared constraint;
- diagnostics must record that external parallelism constrained execution.

The core does **not** claim to infer every external runtime automatically.

## Why explicit hints are necessary

Automatic detection cannot be universal because the process may contain:

- libgomp;
- libomp;
- Intel OpenMP;
- OpenBLAS pthread pools;
- OpenBLAS OpenMP builds;
- MKL;
- BLIS;
- Python ThreadPoolExecutor;
- other schedulers.

No single portable Rust API can safely inspect and control all of them without vendor/runtime coupling.

Therefore:

> explicit host context + optional adapters is safer than hidden global mutation.

## Dependency decision

Do not add these dependencies to the Rust core now:

- OpenMP runtime;
- MKL;
- OpenBLAS;
- Python;
- threadpoolctl;
- BLIS/FlexiBLAS bindings.

Keep:

- Rayon;
- wgpu;
- current thin runtime crates.

## Promotion conditions for an optional interoperability adapter

An adapter may be added only if it has:

1. explicit runtime identification;
2. known scope semantics (thread-local vs process-wide);
3. reversible changes;
4. no hidden process-global mutation;
5. tests covering concurrent callers;
6. clear fallback when the target runtime is absent.

## Current recommendation

Implement one backend-neutral external-parallelism hint in the public runtime contract.

Do **not** implement vendor-specific thread-pool mutation yet.
