# Universal v0.4 Alpha — Sign-off Contract

**Status: SIGNED OFF for the v0.4.0-alpha tag (2026-10-14).** The checks below passed on the same commit SHA. This document continues to define the criteria for that frozen release, not for future branch commits.

## Included alpha scope
Serial/CPU routing; bounded local calibration and revalidation; actual elapsed measurement; epoch invalidation; explicit portable resource notification; Linux CPU-affinity observer; diagnostics; execution fallback; public SDK example.

Not claimed: native Intel HFI/AMD CPPC, production readiness, arbitrary-workload performance superiority or crates.io publication.

## Mandatory release gates
1. Five-platform integration workflow: Linux x64, Linux arm64, macOS Apple Silicon, macOS Intel, Windows x64. Each must pass format, Clippy, runtime API suite, examples, independent Cargo consumer, and stress test. Aggregate gate must pass.
2. Convergence Smoke must pass at the *same* SHA, including economic evidence analyzers and test suites.
3. Independent consumer must compile as a standalone workspace via public runtime-api and validate output equivalence before and after invalidation.
4. Concurrent stress must exercise routing, resource notifications, bounded revalidation, and diagnostic reads with correct output and no crash or deadlock.
5. Unsupported hardware/platform must use explicit notifier without privileged hooks; Linux observer stop must be safe.
6. Release evidence must contain exact SHA, links to both green runs, platform job conclusions and known limitations. Immutable v0.4.0-alpha tag must point at that SHA.

## Verified sign-off record

- Release: https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.4.0-alpha
- Verified tag commit: `f775b528dca13024c915bcde698f7fafa04e7f5d`
- Five-platform gate, all five architecture jobs and aggregate PASS: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38575886948
- Convergence Smoke PASS on the same commit: https://github.com/Johnny-Kao/AlpenCat/actions/runs/38575887316
- External consumer and concurrent stress were required steps of the passing integration workflow.
- **Release assessment: PASS / feature-complete experimental alpha only.** This is not evidence of hardware-native HFI/CPPC or generalized performance benefit.

## Decision
- PASS: all criteria evidenced on one SHA and immutable release tag created.
- FAIL: any required test/job failed or correctness regression.
- BLOCKED: any required run is pending or unavailable; no release tag.

## Release constraints
Preserve v0.1.0-preview tag. No automatic PR merge, publishing to crates.io, or production-quality claims. Local timing results cannot be generalized into universal speedup claims.

## Evidence entry points
- Five-platform gate: https://github.com/Johnny-Kao/AlpenCat/actions/workflows/universal-v04-integration-gate.yml
- Convergence Smoke: https://github.com/Johnny-Kao/AlpenCat/actions/workflows/convergence-smoke.yml
- v0.1: https://github.com/Johnny-Kao/AlpenCat/releases/tag/v0.1.0-preview

## Final reviewer checklist
- [x] Five-platform integration PASS on release SHA
- [x] Convergence Smoke PASS on release SHA
- [x] Independent consumer + concurrent stress PASS
- [x] Known platform limitations and local-only calibration bounds recorded
- [x] Tag points to verified SHA

The passing run URLs above are the frozen evidence for the tagged alpha. Future research-branch commits require separate validation.
