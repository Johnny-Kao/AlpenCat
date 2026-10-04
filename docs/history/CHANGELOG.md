# Changelog

All notable AlpenCat changes will be recorded here.

The project is currently in experimental pre-public-release validation.

## [0.1.0] - Unreleased

### Status

First architecture-complete experimental release candidate.

### Added

- backend-neutral task registration and execution API
- serial/reference execution
- bounded reusable Rayon CPU execution
- wgpu-backed GPU execution proof path
- execution budgets and nested-Rayon protection
- host-declared external-parallelism constraint
- machine and GPU capability discovery
- runtime telemetry
- work-unit planning
- dynamic resource brokerage
- cached execution policy for tiny/hot operations
- online machine-aware cost model
- setup/per-item/transfer cost separation
- local size-bucket residual correction
- uncertainty-aware backend selection
- backend-switch hysteresis
- adaptive execution planning
- continuous rebalance triggers
- integration/migration layer
- C FFI bridge
- upstream dependency and reconciliation ledgers

### Validation snapshot

At runtime commit `eca250d2e9ab8d04e2483d4e9049a5d0f870953a`:

- workspace tests and doctests: 108 PASS
- cost-model release suite: 13/13 PASS
- synthetic crossover: exact at 22,500 items
- Apple M5 FIR correctness: 15/15
- observed-shape replay: 15/15
- leave-one-size-out validation: 13/15

### Known limitations

- public API remains experimental
- cold/unseen workload generalization remains imperfect
- Apple FIR benchmark is not a live Metal Runtime-adapter integration
- broad hardware portability evidence is still incomplete
- external OpenMP/BLAS/Python pool coordination is not automatically controlled
- package publication remains disabled while the public API is experimental
