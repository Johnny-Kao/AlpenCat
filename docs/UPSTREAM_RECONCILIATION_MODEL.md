# Upstream Reconciliation Model

This document defines how the downstream runtime remains maintainable while it is composed from multiple upstream projects and then simplified locally.

## The actual maintenance problem

The downstream project is not just a list of dependencies.

Its state is the result of four things:

    U0 = the upstream baseline originally imported or integrated
    U1 = the upstream state observed later
    D  = the current downstream implementation after pruning, deduplication,
         overrides, connectors, and local fixes
    M  = the transformation map that explains how U0 became D

The central maintenance operation is therefore:

    reconcile(U0, U1, D, M)

not merely:

    update dependency version

## Why ordinary dependency tracking is insufficient

If an upstream project changes after local deduplication, the same upstream change can require very different downstream action.

Example:

    Upstream function A changed.

Case 1:
    A is still used directly downstream.
    -> adopt or rebase the upstream change.

Case 2:
    A was wrapped locally.
    -> inspect whether the wrapper contract still matches;
       usually modify only the wrapper.

Case 3:
    A was deduplicated because another upstream project provides the
    canonical implementation downstream.
    -> do not re-add A automatically.
       Compare the new upstream semantics against the canonical retained
       implementation and decide whether the canonical implementation or
       connector must change.

Case 4:
    A was locally overridden because of an incompatibility.
    -> perform a three-way reconciliation:
       old upstream baseline vs new upstream vs local override.

Case 5:
    A was intentionally dropped.
    -> ignore ordinary implementation churn unless the new upstream change
       alters a semantic contract we still rely on elsewhere.

Without the transformation map, these cases become indistinguishable over time.

## Global reconciliation ledger

Canonical machine-readable file:

    upstream-reconciliation.toml

It records, per upstream source:

- upstream repository;
- import mode;
- baseline reference U0;
- currently observed upstream reference U1;
- downstream owner;
- sync state;
- sync policy.

It also records transformations at the relevant unit boundary:

- upstream unit;
- downstream unit;
- action;
- canonical owner;
- retained/removed scope;
- semantic contract;
- upstream-change policy;
- validation.

Cross-source connectors are recorded separately as `integration` entries.

An integration entry exists when multiple upstream wheels feed one downstream contract, for example:

    Rayon + wgpu
        -> RangeTaskImplementations
        -> Runtime::submit_range_task

This matters because an update to either upstream may require re-validating the same shared connector even when the other upstream did not change.

## Transformation actions

Recommended action vocabulary:

    retained
    connector_wrapper
    deduplicated
    deduplicate_or_prune
    override
    replaced
    dropped
    compatibility_pin

These describe what happened between U0 and D.

Do not infer them later from source code. Record them when the transformation is made.

## Update algorithm

When an upstream source publishes a new version or commit:

### 1. Freeze the current reconciliation state

Read:

    U0
    D
    M

Do not immediately change the baseline.

### 2. Observe U1

Record the new upstream tag/commit as:

    observed_upstream_ref

At this point:

    baseline_ref = U0
    observed_upstream_ref = U1

### 3. Compute upstream delta

Conceptually:

    ΔU = diff(U0, U1)

For vendored/forked code this should eventually be file/symbol aware.

For package-based integrations it may initially be API/feature/behavior based.

### 4. Intersect ΔU with M

For every changed upstream unit, find its transformation entry.

Then route by policy:

    retained
      -> direct update / rebase candidate

    connector_wrapper
      -> inspect and usually modify wrapper only

    deduplicated
      -> compare new semantics against canonical downstream owner
      -> do not restore the duplicate automatically

    override
      -> three-way merge:
         U0 vs U1 vs D

    dropped
      -> ignore unless semantic contract changed

    compatibility_pin
      -> re-run removal condition

### 5. Validate downstream behavior

Use the transformation's validation list.

Run full workspace tests after targeted checks.

Re-run calibration if scheduling, memory movement, or performance semantics changed.

### 6. Update M if the relationship changed

Examples:

    wrapper moved
    canonical owner changed
    a duplicate is now needed again
    upstream removed the old API
    local override became unnecessary
    an upstream project gained a capability previously supplied elsewhere

Update the ledger before advancing U0.

### 7. Advance baseline only after validation

After downstream is reconciled and validated:

    baseline_ref = U1
    observed_upstream_ref = UNSET or U1
    sync_state = synchronized

This is important.

If the baseline is advanced before reconciliation, the project loses the
reference point needed to understand the local divergence.

## Deduplication is a tracked transformation, not deletion

Suppose:

    upstream A provides feature X
    upstream B also provides feature X

The downstream project selects B as canonical and removes A::X.

Record:

    upstream_unit = A::X
    action = deduplicated
    canonical_owner = B::X

If A later improves X, the update process asks:

    Did A add semantics/performance/correctness that B::X now lacks?

Possible outcomes:

    no
      -> keep downstream unchanged

    yes, but connector can compensate
      -> modify connector

    yes, canonical B should change
      -> update/replace canonical implementation

    yes, A is now better canonical owner
      -> explicitly change canonical owner and record migration

This is the global-table behavior the project needs to stay maintainable.

## Overrides and wrappers

A wrapper is preferred when upstream interfaces move but downstream semantics should stay stable.

Target shape:

    external upstream API
        -> local adapter/wrapper
            -> stable downstream contract

An override is appropriate when upstream behavior itself must be changed locally.

Overrides carry higher maintenance cost than wrappers and therefore require a stronger transformation entry:

- original upstream unit;
- baseline behavior;
- local behavior;
- reason;
- merge policy;
- validation.

## Source import modes

The ledger supports different integration depths:

    external_dependency
        upstream code is not copied locally;
        only wrapper/API relationship is tracked.

    vendored_source
        source is copied into downstream;
        U0/U1 file and symbol diffs become first-class.

    forked_source
        downstream directly maintains a modified fork;
        three-way reconciliation is expected.

    compatibility_pin
        downstream constrains upstream resolution/version behavior.

The same reconciliation model applies to all four.

## Current prototype status

The current runtime prototype mainly uses:

    external_dependency

for Rayon, wgpu, bytemuck, and pollster.

Therefore current reconciliation is mostly at wrapper/API/feature level.

If future phases vendor or merge upstream source trees for deeper deduplication, the same ledger should become more granular:

    repository
      -> module
        -> file
          -> symbol/function
            -> transformation

The schema should be extended rather than replaced.

## Relationship to upstream-dependencies.toml

There are now two different records:

    upstream-dependencies.toml
        package-level dependency/provenance information

    upstream-reconciliation.toml
        U0/U1/D/M transformation and synchronization information

The first tells us:

    what external component exists and why

The second tells us:

    how that upstream component has been transformed downstream
    and how to reconcile future upstream updates

Neither replaces the other.

## Long-term path

The eventual goal may be to replace selected external wheels with native downstream implementations.

That is a later optimization.

Until then, the maintainable path is:

    borrow mature wheel
    -> integrate
    -> deduplicate overlap
    -> wrap/override behind stable connectors
    -> record transformation globally
    -> reconcile upstream changes through the map
    -> replace wheel only when local ownership is justified

This avoids prematurely rewriting every scheduler/runtime/backend while still preventing the downstream codebase from drifting into an unmaintainable fork.
