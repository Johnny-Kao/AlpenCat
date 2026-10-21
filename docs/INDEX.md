# Documentation index

AlpenCat's resource-only source alpha is [v0.5.0-alpha](https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.5.0-alpha). The previous execution runtime remains preserved at [v0.4.0-alpha](https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.4.0-alpha).

## Start here

| Need | Document |
| --- | --- |
| Get a standalone resource-only snapshot | [Resource alpha quickstart](RESOURCE_ALPHA_QUICKSTART.md) |
| Understand the product and run the SDK | [README](../README.md) |
| Browse folders, archived research and workflow inventory | [Project structure](PROJECT_STRUCTURE.md) |
| System components and resource-transition flow | [Architecture](ARCHITECTURE.md) |
| What was released, and what was excluded | [Versions and release notes](RELEASE_NOTES.md) |
| Exact test runs and their limitations | [Test evidence](TEST_EVIDENCE.md) |
| Next priorities and release gates | [Roadmap](ROADMAP.md) |
| v0.4 alpha sign-off criteria | [v0.4 sign-off record](UNIVERSAL_V0_4_SIGNOFF.md) |
| v0.1 preview usage | [v0.1 preview](UNIVERSAL_V0_1_PREVIEW.md) |
| Native event/hardware research | [Native validation](NATIVE_VALIDATION.md) |

## Project navigation

- `crates/runtime-api/` — SDK and optional Linux affinity adapter; examples and integration tests.
- `crates/runtime-core/` — epoch and publication primitives.
- `crates/runtime-selector/` — hot-path selection.
- `crates/runtime-cpu-rayon/` — CPU execution.
- `crates/runtime-gpu-wgpu/` — optional GPU backend.
- `crates/runtime-machine/` — hardware capability discovery.
- `tests/external-consumer/` — standalone Rust consumer, intentionally outside the main Cargo workspace.
- `benchmarks/` — hot-path and boundary economics.
- `experiments/` — supporting scripts, analyzers and historical research.
- `tools/native-validation/` — hardware/platform validation utilities.
- `.github/workflows/` — current release gates and historical research workflows.

## Research archive policy

The dated documents under `docs/` are research records, not necessarily current SDK instructions. They are retained to preserve negative findings, historical evidence and design evolution; consult this index and the versioned release notes for the **current contract**. Do not delete/rename dated studies merely to improve presentation: doing so would break evidence references. Earlier removed runtime components remain recoverable from Git history and `archive/pre-runtime-pruning-2026-10-04`.

## Evidence rules

A passing job only validates the commit and checks that were actually executed. Cross-machine results are portability evidence, not proof of universal performance advantage. Native Intel HFI/AMD CPPC event delivery remains unverified.
