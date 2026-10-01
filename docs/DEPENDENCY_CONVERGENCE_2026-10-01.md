# M16 — Dependency Convergence

Status: BASELINE IMPLEMENTED (2026-10-01).

## Goal

Use mature external components where they are justified, keep them behind narrow wrappers, and make dependency ownership/convergence auditable instead of implicit.

The target is not "fewest dependencies".

The target is:

```text
borrow mature wheels
-> isolate behind wrappers
-> record semantic contract
-> reconcile upstream changes
-> remove temporary pins when possible
-> replace only when internal ownership is justified
```

## Dependency ownership model

Every tracked dependency now records:

- status;
- ownership class;
- convergence state;
- package/version;
- upstream repository;
- wrapper paths;
- semantic contract;
- upgrade checks;
- removal/replacement rule.

Ownership classes:

```text
external_mature
external_support
compatibility_pin
internal
```

Convergence states:

```text
retain_wrapper
review_host_executor
remove_when_constraint_clears
replace_when_justified
internal_owned
```

## Current decisions

```text
rayon
  ownership: external_mature
  convergence: retain_wrapper

wgpu
  ownership: external_mature
  convergence: retain_wrapper

bytemuck
  ownership: external_support
  convergence: retain_wrapper

pollster
  ownership: external_support
  convergence: review_host_executor

ordered-float MSRV pin
  ownership: compatibility_pin
  convergence: remove_when_constraint_clears
```

This explicitly avoids rewriting Rayon/wgpu merely to reduce dependency count.

## Executable validation

New validator:

```text
tools/validate_dependency_convergence.py
```

It cross-checks:

- upstream-dependencies.toml schema;
- unique dependency IDs;
- allowed status/ownership/convergence states;
- wrapper paths actually exist;
- active packages exist in Cargo.lock;
- active dependencies exist in upstream-reconciliation.toml;
- reconciliation sources are not missing dependency records;
- contracts/upgrade checks/removal rules are non-empty;
- compatibility pins have an explicit removal convergence rule.

This validator is part of the public GitHub runtime CI.

## Four-way reconciliation remains canonical

M16 does not replace upstream-reconciliation.toml.

The two files have different roles:

```text
upstream-dependencies.toml
  -> what we depend on and who owns it

upstream-reconciliation.toml
  -> how upstream U0/U1 maps to downstream D through transformation M
```

Both must validate.

## Replacement rule

A mature external component is replaced only when there is evidence that internal ownership improves one or more of:

- runtime control;
- correctness;
- portability;
- measurable performance;
- long-term maintenance cost.

Dependency-count reduction alone is not sufficient evidence.

## Historical retention

Removed/replaced dependencies are not deleted from the map.

Their status and provenance remain so later maintainers can reconstruct why a wheel was borrowed, changed, or removed.
