# Validation Status

> Updated: 2026-10-04

## Status

| Question | Status |
| --- | --- |
| Can a calibrated boundary become wrong after resource conditions change? | Validated |
| Should every resource transition trigger immediate recalibration? | Falsified |
| Is unbounded crossover search economically acceptable? | Falsified |
| Is global SERIAL fallback safe when a bounded search finds no crossover? | **Falsified** |
| Does localized evidence without extrapolation avoid that regression? | **Supported: 0/320 regressions in rerun** |
| Is one fixed slowdown threshold a universal capacity-loss classifier? | Falsified |
| Is the production PublishedBoundary + ResourceEpoch core concurrency-safe under stress? | **Validated across 12 cross-platform jobs** |
| Is the production hot path still lightweight after pruning? | **Validated across 8 x64 jobs** |
| Has a real vendor-native hardware event reached AlpenCat end-to-end? | Open |

## 1. Production-core robustness

Cross-platform stress run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186182010

Matrix:

- Ubuntu 22.04 × 3
- Ubuntu 24.04 × 3
- Windows 2025 × 3
- macOS 14 / Apple Silicon × 3

Observed Linux allocations included AMD EPYC 7763 and AMD EPYC 9V74. The macOS runners reported aarch64-apple-darwin / Apple M1 (Virtual).

All **12/12 jobs passed**.

Each job exercised:

- 200,000 concurrent boundary publications;
- 8 reader threads × 300,000 lock-free snapshots;
- 1,000,000 randomized FastRoute decisions checked against a reference model;
- 100,000 randomized invalidate/publish epoch cycles;
- Rayon execution-budget tests.

Across the 12 jobs this is approximately:

- 28.8 million concurrent boundary snapshots;
- 2.4 million boundary publications;
- 12 million randomized route checks;
- 1.2 million randomized epoch cycles.

No torn boundary snapshot, stale-state invariant failure, or routing mismatch was observed.

## 2. Expanded x64 resource matrix

First expanded run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186182005

The 8 independently allocated Linux x64 jobs covered Ubuntu 22.04 / 24.04 and observed AMD EPYC 7763 plus Intel Xeon Platinum 8370C hardware.

Each job measured:

- full budget;
- 3-thread budget;
- half budget;
- 1-thread budget;
- Rayon oversubscription;
- light external contention;
- heavy external contention;
- post-contention recovery.

Random replay evaluated 32 deterministic seeds × 4,096 calls × 5 workload distributions across every measured phase, or roughly **41.9 million replayed calls** across the 8 jobs.

### Falsification: global fallback

The earlier research policy used:

~~~text
bounded search finds no crossover
-> assume crossover disappeared
-> global SERIAL fallback
~~~

That is not safe.

In the first expanded run, **7/320 phase/distribution combinations** showed a routing-regret regression under this policy.

A representative one-thread case moved the actual crossover from 16,384 to 262,144. The bounded search inspected 16,384 -> 32,768 -> 65,536, found no crossover, and incorrectly extrapolated that local evidence to all larger workloads.

For some large/bimodal random streams, cumulative routing regret became roughly **4.15× worse** than retaining the stale boundary.

The correct conclusion is: a bounded search proves only what it measured. Failure to find a crossover locally is not evidence that no crossover exists globally.

## 3. Localized evidence, no extrapolation

Rerun:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186607338

Rule:

~~~text
bounded local search
-> update only the measured/validated region
-> leave unmeasured regions stale/unknown
-> do not publish a global SERIAL fallback
~~~

Across the rerun:

- 8/8 economics jobs succeeded;
- 320 phase/distribution combinations were replayed;
- **0/320 localized-policy regressions** were observed.

On the first-run raw datasets, the same localized interpretation reduced one-thread routing regret by roughly **22%–60%** without extrapolating into unmeasured large-workload regions.

Heavy contention often requires additional future local validation farther from the old boundary; one small local check cannot safely infer the entire crossover curve.

## 4. Production hot-path overhead

Production-code run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37186703335

This benchmark imports the current production runtime-core and runtime-selector implementation directly, including PublishedBoundary and ResourceEpoch.

All **8/8 x64 jobs passed**.

| Path | Incremental cost |
| --- | ---: |
| Production epoch-match path | **~0.198 to ~0.614 ns/call** |
| Stale but far from boundary | **~0.471 to ~1.123 ns/call** |
| Stale + near-boundary bookkeeping | **~0.494 to ~6.461 ns/call** |

The normal production path therefore remained below approximately **0.7 ns/call** across this matrix.

## 5. Current architectural conclusion

The stable core is:

~~~text
native transition
+ ResourceEpoch
+ PublishedBoundary
+ FastRoute
+ localized validity
+ lazy bounded revalidation
~~~

Key rule from robustness testing:

> **Local evidence stays local.**

A revalidation routine may publish a new global boundary when it actually observes a nearby crossover. If it exhausts its local budget without finding one, it must not infer boundary = infinity. The boundary remains stale outside the validated region, and future relevant demand may justify another local check.

## 6. Remaining evidence

The main remaining architecture gate is native event delivery:

~~~text
physical platform transition
-> native kernel / firmware signal
-> thin userspace adapter
-> ResourceEpoch increment
-> stale boundary observed
-> localized bounded runtime response
~~~

Intel HFI remains the first target.

See [Native Validation](./NATIVE_VALIDATION.md).
