# Adaptive Execution Guard Alpha — Actual independent adopter outcomes

**Result: FUNCTIONAL ALPHA PILOT PASS on Linux; automatic performance advantage NOT SUPPORTED.**

This is a real Rust `tiny_http` service, with 8 persistent Rust worker threads and `alpencat-guard` controlling **new request admission** in explicit Enforce mode. It is independently packaged at `experiments/guard-http-consumer/` and has no direct Runtime dependency.

## Verifiable artifacts

- [Real two-platform Linux Cgroup2 HTTP pilot #40533509451](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40533509451), exact tested source SHA `b47bc1e2864632300adaac8dc21d6a46df94b2d3`.
- Ubuntu 22.04 and 24.04 both SUCCESS, raw CSV/JSON uploaded for each.
- [SHA-pinned out-of-repo Cargo integration #40533110370](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40533110370) PASS, tested at `ff3579917af082b7938427ae48f55578dd7f92f3`.
- [Portable API and standalone Cargo build #40532960343](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40532960343): Ubuntu 22.04, Ubuntu 24.04, Windows 2025 PASS; macOS 15 runner queued at the time of this note. Do not call the complete matrix PASS until it completes.

## What the real Linux pilot proves

Each independent service instance ran in its own disposable systemd Cgroup v2, with actual readback of `cpu.max` after **2 → 0.5 → 2 CPU** transitions and actual `cpu.stat.nr_throttled` increments. 84 requests/phase, 12 client workers, 1.5 million integer transformations per request, 3 shuffled paired rounds and equivalent response SHA digests across all modes.

The mode semantics are now an **API-enforced contract**, not an example convention:

| Mode | Real active admission limit sequence | What it means |
|---|---|---|
| fixed8 baseline | 8 → 8 → 8 | App owns its own 8-slot gate |
| Observe | 8 → 8 → 8 | Guard observes but cannot give the app a controlled gate |
| Recommend | 8 → 8 → 8 | Guard recommends 4 → 2 → 4 but never changes admission |
| Enforce | 4 → 2 → 4 | Guard's only mode that hands out a live controlled gate |
| direct | 4 → 2 → 4 | Same exact guard policy but uses direct `snapshot()` on each event, not ResourceState cached observation |

The independent Consumer validates outputs and live quota after each event, including the recovery phase. It performs no implicit OS reads on request hot path. Both Ubuntu platforms PASS.

## Honest 0.5 CPU restricted-phase metrics (median of 3 paired trials)

End-to-end client p95, total batch client Wall Time, service Cgroup CPU usage. **Lower is better** for each time column. This is a controlled reference workload, not production telemetry.

| Runner | Policy | HTTP p95 ms | HTTP Wall ms | Cgroup CPU ms |
|---|---|---:|---:|---:|
| Ubuntu 22.04 | fixed8 | **98.27** | 371.55 | 216.78 |
| Ubuntu 22.04 | Observe (passive 8) | 99.12 | 384.03 | 216.38 |
| Ubuntu 22.04 | Enforce (2 slots) | 349.76 | **359.02** | 213.33 |
| Ubuntu 22.04 | Direct same policy | 362.37 | 369.68 | **211.37** |
| Ubuntu 24.04 | fixed8 | **98.44** | **339.52** | 216.71 |
| Ubuntu 24.04 | Observe (passive 8) | 98.97 | 356.99 | 215.49 |
| Ubuntu 24.04 | Enforce (2 slots) | 359.83 | 377.94 | **211.92** |
| Ubuntu 24.04 | Direct same policy | 355.03 | 365.12 | 212.69 |

**Critical negative result:** On this workload, the candidate 4→2→4 admission profile **significantly worsens HTTP p95 under 0.5 CPU compared with fixed8**, while barely affecting Cgroup CPU cost. Identical-policy direct OS behaves similarly. It would be irresponsible to ship this profile as a universal automatic accelerator, regardless of the 2-platform functional CI PASS.

## Product engineering GO vs economic GO

- **GO as explicit opt-in admission-control Alpha:** verified library correctness, passive API enforcement boundaries, stale/unknown fallback, concurrency RAII, feedback rollback unit tests, external Cargo install, real Linux changes and HTTP admission behavior.
- **NOT GO for default automatic worker/admission profile:** the illustrative 4→2→4 profile is a losing policy under this independent workload. It must remain opt-in with application-provided budgets, comparable feedback and a rollback mechanism. Default `GuardConfig` uses Observe.
- **Not yet feature-complete middleware:** per-application end-to-end measured feedback has not been wired to a production framework; `report_comparison` is caller-fed and tested via unit tests. No async Tokio middleware, no readiness gating for external live adopters, no event subscription.
- Next finished product milestone: build a framework-level adapter with a caller-owned SLO sampler and rollback, and validate its protection on this **known losing** scenario. Such a negative-control test must be passed before anyone enables automatic enforcement by default.
- Do not merge to main, publish crates.io or create a release tag without a separate release signoff.

