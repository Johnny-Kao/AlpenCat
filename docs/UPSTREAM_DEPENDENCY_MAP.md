# Upstream Dependency Map and Wrapper Maintenance Policy

Status: canonical maintenance policy.

Machine-readable source of truth:

    runtime-framework/upstream-dependencies.toml

## Principle

This project intentionally borrows mature external execution/runtime components.

The maintained product is primarily:

    public contract
    -> selector
    -> wrapper / adapter
    -> external wheel

The goal is not to copy upstream implementations into this repository.

When an upstream project changes, the preferred repair point is the wrapper/adapter.

## Why provenance must survive dependency reduction

Dependency reduction can create a maintenance failure if a dependency or adapter is removed without preserving why it existed.

Example failure mode:

    external wheel v1
    -> local wrapper workaround
    -> workaround later removed without provenance
    -> external wheel v2 changes contract
    -> maintainers no longer know which old behavior was intentionally compensated

Therefore, removing code is not equivalent to erasing its maintenance history.

## Mandatory rule: dependency entries are tombstoned, not deleted

When an external dependency is removed:

1. keep its entry in upstream-dependencies.toml;
2. change status = "active" to "removed" or "replaced";
3. retain package name, last version/version requirement, upstream repository, former wrapper paths, relied-on contract, and upgrade checks;
4. add removal date, removal commit, reason, and replacement dependency/path when applicable.

This creates a durable upstream lineage.

## Wrapper ownership rule

External-runtime-specific behavior belongs behind one local owner.

| Upstream wheel | Local owner | What we own |
| --- | --- | --- |
| Rayon | runtime-cpu-rayon | budget, nested-context handling, conversion to framework result |
| wgpu | runtime-gpu-wgpu | GPU lifecycle, buffers, shader dispatch, readback, error translation |
| bytemuck | GPU wrapper | byte/slice conversion boundary |
| pollster | GPU wrapper | synchronous bridge to wgpu async initialization |
| ordered-float 5.3 pin | GPU manifest / lock | Rust-1.87 dependency-resolution compatibility |

The public API must not expose these upstream-specific types.

## Upgrade workflow

For every direct external dependency upgrade:

    1. Read upstream release/change notes.
    2. Read the dependency-map entry.
    3. Identify which wrapper contract points may have changed.
    4. Upgrade only the external dependency and its wrapper first.
    5. Run the entry upgrade_checks.
    6. Re-run full workspace tests.
    7. Re-run calibration when scheduling/performance behavior may have changed.
    8. Update the map if the relied-on contract changed.
    9. Record any compatibility workaround added or removed.

Do not spread an upstream API migration through selector/core/public API unless the framework contract itself needs to change.

## Upstream update impact categories

### A. Transparent upstream update

Examples: internal bug fix, performance improvement, implementation detail change, no wrapper API/semantic impact.

Action:

    upgrade dependency
    -> run mapped checks
    -> no framework redesign

### B. Wrapper adaptation

Examples: wgpu method signature change, Rayon pool/context API change, changed error type, changed initialization contract.

Action:

    update wrapper only
    -> preserve runtime public contract
    -> run mapped checks

This is the normal expected maintenance path.

### C. Contract-breaking upstream change

Examples: upstream removes a relied-on capability, thread-scope semantics change, device/buffer lifecycle semantics change, or MSRV moves beyond the project target.

Action:

    do not silently absorb
    -> record incompatibility
    -> decide pin / adapter workaround / replacement wheel
    -> update dependency map

### D. Duplicate wheel / redundant layer

Removal is allowed when two dependencies or wrappers perform materially the same role and one does not add a distinct contract.

Before removal, record overlap, retained implementation, removed implementation, migration path, tests proving equivalent behavior, and an upstream-lineage tombstone.

## Compatibility pins are dependencies too

A package can be present solely to control dependency resolution.

Example:

    ordered-float = 5.3.0

It is still a maintenance dependency even though local Rust source never imports it.

Such pins must have a documented reason, removal condition, validation command, and tombstone when removed.

## Cargo.lock role

Cargo.lock is retained for reproducible research builds.

It records the concrete dependency graph used for validation.

Cargo.lock does not replace the upstream dependency map.

The lockfile tells us what resolved.

The dependency map tells us why we depend on it, where the boundary is, and what must be retested when it changes.

## Dependency-reduction gate

No dependency or adapter may be removed during architecture reduction until all of the following are answered:

    What upstream capability does it provide?
    Where is the local wrapper?
    Which framework contract depends on it?
    Is another dependency truly duplicating that capability?
    Which tests prove equivalent behavior after removal?
    What tombstone/provenance record will remain?

If these cannot be answered, the dependency is not ready to be removed.

## Future update monitoring

Automated version monitoring may be added later.

Any automation that can trigger paid CI or paid runners must not be enabled without explicit user approval.

The dependency map itself is independent of the monitoring mechanism and remains the canonical maintenance record.
