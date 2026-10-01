# M15 — Integration / Migration Layer

Status: BASELINE IMPLEMENTED (2026-10-01).

## Goal

Make adoption cheap for existing low-level projects without making legacy execution part of the runtime core.

M15 is intentionally an integration layer outside the resource-control core.

## Migration decision

An adopting project provides whether a workload has been validated for runtime use.

The runtime combines that readiness with the M13 execution plan and returns:

```text
MigrationPlan {
    execution_plan,
    decision,
}
```

Possible owners:

```text
Runtime
HostExisting
```

Possible reasons:

```text
RuntimeValidated
UnsupportedWork
NotYetValidated
```

The default migration policy is conservative.

Unsupported/unvalidated work stays on the host implementation.

Performance-model confidence does not gate semantic migration safety. A workload that is explicitly supported and differential-validated may enter the runtime even during M12 cold start.

## Public API

```rust
Runtime::migration_plan(...)
```

The caller does not need to manually reconstruct M12/M13 decisions before deciding whether the workload is ready for migration.

## Single-execution safety

M15 introduces `ExecutionLease`.

The lease separates:

```text
decision
-> pre-execution fallback window
-> commit one execution owner
```

Fallback to the host is legal only before runtime execution has been committed.

After:

```text
commit_runtime()
```

a later fallback request returns an error.

This prevents the dangerous sequence:

```text
runtime performs side effect
-> caller interprets result as failure
-> legacy path performs same side effect again
```

## Architectural boundary

The host implementation remains outside the runtime core.

M15 does not add a permanent Legacy backend.

It only provides migration ownership and safety primitives so an adopting project can temporarily coexist with its existing path.

Target migration:

```text
Stage 0  host implementation
Stage 1  validated subset -> runtime
Stage 2  runtime default, host fallback before execution
Stage 3  runtime owns execution; old host path can be deleted
```

## Validation policy

Migration safety is based on host integration evidence:

- supported by the adapter;
- differential/reference validation completed.

M12/M13 confidence remains a performance-learning signal only. It is deliberately not reused as a correctness gate, because doing so would prevent validated cold-start workloads from entering the runtime and generating the observations needed for automatic learning.

## Validation target

- unsupported work never enters runtime;
- low-confidence migration can stay on host;
- validated confident work can enter runtime;
- host-selected work cannot commit runtime execution;
- post-runtime-commit fallback is rejected;
- migration ownership is decided before side effects.
