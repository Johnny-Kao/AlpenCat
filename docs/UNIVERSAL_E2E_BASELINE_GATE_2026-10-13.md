# Universal End-to-End Baseline Gate — 2026-10-13

## Scope
Validate frozen, portable AlpenCat runtime on 7 runner classes x2 independent allocations and x64 production hotpath on two Ubuntu images x2. No estimator tuning, no PSI thresholds, no new runtime path or optimization.

## Reused production tests
- runtime-core: torn snapshots, ResourceEpoch behavior
- runtime-selector: FastRoute equivalence
- runtime-api convergence_runtime: fresh zero-measurement no-op, stale publication and route transition, no-local-crossover stale retention, invalidation during measurement, CPU route unavailable
- runtime-cpu-rayon: effective execution-budget and single-core fallback
- x64 hotpath: existing epoch-match <=2ns and stale-far <=3ns gross regression ceilings

## Evidence interpretation
This gate checks portable **explicit** invalidation and bounded revalidation end to end through the real Runtime API. Synthetic measurement callbacks are used in existing tests; it does NOT demonstrate operating-system native event arrival, automatic adapters, true physical-server capacity change, or autonomous invalidation detection. Mark those UNVALIDATED regardless of portable gate PASS.

## Exit
All jobs success -> Portable Universal Baseline PASS; pin current commit and preserve Foundation freeze.
Any job failure -> gate FAIL, inspect invariant before changes.
Native Adapter integration is a separate, evidence-driven workstream requiring actual platform events; do not introduce guessed thresholds or unnecessary generic parameters.
