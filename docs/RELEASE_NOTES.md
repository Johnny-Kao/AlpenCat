# Versions and release notes

## Version summary

| Tag | State | Scope |
| --- | --- | --- |
| [v0.1.0-preview](https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.1.0-preview) | Preserved historical preview | Portable runtime, explicit invalidation, caller-provided bounded revalidation |
| [v0.4.0-alpha](https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.4.0-alpha) | **Released — experimental alpha** | Real elapsed calibration, notifications, Linux affinity observation, diagnostics, SDK and cross-platform integration evidence |

Cargo workspace package version remains `0.1.0`. The Git tag is the **source snapshot identity** for the alpha release; these are not published `0.4.0` crates. The v0.4 alpha source snapshot is immutable by convention. Later documentation commits on the research branch are **not** part of the tagged release.

## v0.4.0-alpha — 2026-10-14

- Commit: [`f775b528dca13024c915bcde698f7fafa04e7f5d`](https://github.com/Johnny-Kao/AlpenCat/commit/f775b528dca13024c915bcde698f7fafa04e7f5d)
- Release: [GitHub pre-release](https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.4.0-alpha)
- Full validation: [five-platform integration](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38575886948) and [Convergence Smoke](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38575887316), **both passing at the exact tagged SHA**.

### Included

- Serial/CPU routing and existing optional GPU code path; backend availability is conditional.
- Measured serial/CPU elapsed-time samples; bounded initial calibration and stale-boundary revalidation.
- `ResourceEpoch`, `PublishedBoundary`, explicit `ResourceNotifier`, `RuntimeDiagnostics`.
- Linux `/proc/self/status` CPU-affinity watcher with owned lifecycle and explicit stop; portable caller notifications on all platforms.
- Runnable Rust SDK examples and independent consumer; concurrent stress test.
- Five-platform integration gate: Linux x64, Linux ARM64, macOS Apple Silicon, macOS Intel and Windows x64.

### Not included / not demonstrated

- Native Intel HFI or AMD CPPC notification source; polling process affinity is not hardware HFI.
- Continuous automatic adaptation of arbitrary workloads. A sample may find no crossover, and some routes may be unavailable.
- Measured universal speedup, production-grade resource guarantees, or comprehensive GPU hardware benchmarking.
- crates.io packaging / `0.4.0` Cargo package version; production support or ABI/API stability.

## v0.1.0-preview

- Tag SHA: `5d159f05fe17a56da3b55dd83998b94bcd5807dc`.
- [Preview gate](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38479856600).
- Explicit application-driven invalidation and caller-provided comparable cost evidence; no claim of native autonomous hardware event support.
- Detailed usage: [v0.1 preview](UNIVERSAL_V0_1_PREVIEW.md).

## Suggested dependency for the released alpha

```toml
[dependencies]
runtime-api = { git = "https://github.com/Johnny-Kao/AlpenCat", tag = "v0.4.0-alpha" }
```

The main Cargo workspace is an unpublished experimental repository. For a working standalone integration example, see `tests/external-consumer/` and [README](../README.md).
