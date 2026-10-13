# AlpenCat Universal v0.1 Preview — Getting started

**Status:** usable experimental Rust workspace preview; not a production-certified or native-event-autonomous SDK.

## What is actually usable
- Explicit `Runtime::invalidate_resources()` upon an application-known resource change.
- Cached `PublishedBoundary` and `ExecutionMode::Auto` routing for registered serial/CPU execution.
- Bounded `revalidate_serial_cpu()` with caller-provided comparable costs.
- Correctness-guarded result equivalence, stale state, unavailable-route fallback.
- Portable core on Linux, macOS and Windows.

## Install / run from repository
Requires Rust 1.87+:

```bash
cargo test -p runtime-api --test convergence_runtime
cargo run -p runtime-api --example universal_quickstart
```

For another Rust project in the same checkout, use a path dependency:

```toml
[dependencies]
runtime-api = { path = "../AlpenCat/crates/runtime-api" }
```

To pin the research preview in a separate repository, use the freeze branch (which does **not** include this quickstart example):

```toml
[dependencies]
runtime-api = { git = "https://github.com/Johnny-Kao/AlpenCat", branch = "baseline/universal-e2e-pass-2026-10-13" }
```

## API contract
The v0.1 Universal preview is **application-driven**: a caller or integration explicitly raises a resource-change notification and supplies measurement evidence only when necessary. It does NOT discover operating-system events automatically or choose economic policies. `BoundaryProfile::new(4096, ...)` and example cost curves are teaching values, not recommended performance thresholds.

## Verified evidence
- Portable universal gate: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38414472890 (19/19).
- Linux affinity-to-Runtime E2E research: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38476369737 (5/5).
- Last known convergence regression, prior to v0.1 cleanup: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38476354332 failed **formatting only**, subsequently corrected; fresh sign-off is required.
- Native HFI / AMD events and production economics: **UNVALIDATED**.

## v0.1 preview exit criteria
1. `cargo fmt --all -- --check` and `cargo clippy -p runtime-api --all-targets -- -D warnings`.
2. `cargo test -p runtime-api` and executable quickstart on Linux, macOS, Windows.
3. Previous universal baseline reference remains unchanged.
4. Explicitly document that native hardware delivery and real-economic validation remain out of scope.

No generic threshold tuning or continuously polling hot path is part of this preview.
