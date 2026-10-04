# Contributing to AlpenCat

AlpenCat is currently an experimental v0.1 runtime under active validation.

Contributions are welcome when they improve correctness, portability,
measurement quality, integration quality, documentation, or evidence-backed
runtime behavior.

## High-value contribution areas

- Linux, macOS, and Windows portability testing
- Intel, AMD, and Apple Silicon benchmark evidence
- CPU/GPU crossover measurements
- reproducible correctness or performance bugs
- external-runtime interoperability tests
- backend adapters
- examples and documentation
- validation of cold/unseen workload behavior

## Design boundary

AlpenCat v0.1 is architecture-complete for the current experimental scope.

Please open a design discussion before proposing:

- a new scheduler architecture
- a new mandatory dependency
- hardware-specific hard-coded policy thresholds
- a broad public API redesign
- a second overlapping control plane for functionality already owned by the
  runtime

The preferred default is the smallest change that preserves the existing
execution model.

## Development principles

Priority order:

1. safety
2. correctness
3. evidence
4. simplicity
5. performance
6. extensibility

Performance claims should include reproducible measurements and should not mix
results from different machines into an A/B comparison.

Fallback behavior is part of the correctness contract. Optimized paths should
remain additive where practical: unsupported or uncertain cases should retain a
safe reference or generic path.

## Validation expectations

For Rust changes, the normal validation surface includes:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

Changes touching dependency ownership or upstream-derived behavior should also
run the repository's reconciliation/dependency validators.

Changes touching GPU execution should preserve a non-GPU correctness path and
must distinguish software-GPU correctness evidence from hardware-accelerator
performance evidence.

Benchmark changes should record:

- machine/environment
- compiler/toolchain
- workload definition
- warmup/repetition method
- p50/p90 or equivalent distribution summary
- correctness tolerance
- whether measurements are in-sample, held-out, or cross-machine

## Pull requests

Keep pull requests narrow and reviewable.

A useful PR description should state:

- the problem
- the behavioral boundary
- the implementation choice
- rejected alternatives when material
- correctness validation
- performance/resource evidence when relevant
- remaining limitations

Do not present experimental measurements as universal defaults.

## Public API stability

The v0.x API is experimental. Compatibility is not guaranteed before v1.0.

Changes should still avoid unnecessary churn. A breaking API change should
explain why the existing interface prevents a materially better or safer
implementation.

## Licensing

AlpenCat is dual-licensed under `MIT OR Apache-2.0`.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in AlpenCat is provided under the same dual-license terms.
