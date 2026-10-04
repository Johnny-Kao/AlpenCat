# Validation Status

> Updated: 2026-10-04

## Status

| Question | Status |
| --- | --- |
| Can a calibrated boundary become wrong after resource conditions change? | Validated |
| Should every resource transition trigger immediate recalibration? | Falsified |
| Is unbounded crossover search economically acceptable? | Falsified |
| Does bounded local revalidation materially reduce pathological search cost? | Validated |
| Is one fixed slowdown threshold a universal capacity-loss classifier? | Falsified |
| Is slowdown still useful as secondary severe-case evidence? | Supported |
| Can the epoch mechanism be extremely small in absolute cost? | Supported by prior 8-runner microbenchmark |
| Has the new production `PublishedBoundary + ResourceEpoch` path been re-benchmarked? | Open |
| Has a real vendor-native hardware event reached AlpenCat end-to-end? | Open |

## Boundary economics

Primary heterogeneous x64 run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37156808130

Eight independently allocated Ubuntu x64 runners included Intel Xeon and AMD EPYC systems.

Observed slowdown ranges at the old crossover:

| Regime | Range |
| --- | ---: |
| Half effective CPU budget | 1.086x - 1.397x |
| One effective worker | 1.341x - 1.817x |
| Strong external contention | 1.518x - 2.233x |

Half-budget and one-worker ranges overlap. Therefore slowdown alone cannot be the primary universal classifier.

Strong contention remained clearly visible across all eight samples.

## Revalidation economics

Representative earlier controlled results:

| Regime | Full localized search | Bounded policy |
| --- | ---: | ---: |
| One-worker collapse | ~33 ms | ~1 ms |
| Strong contention | ~100 ms | ~1-2 ms |

The stable conclusion is architectural rather than tied to one exact number: bounded local validation avoids paying the full cost of proving that a distant or nonexistent crossover exists.

The analyzer now prices near-boundary break-even using the measured point nearest the old crossover rather than averaging across the entire benchmark grid.

## Hot-path evidence

Prior 8-runner microbenchmark:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37177541260

Observed incremental cost versus the synthetic baseline:

| Path | Incremental cost |
| --- | ---: |
| Relaxed epoch-match path | ~0.016 to ~0.514 ns/call |
| Stale but far from boundary | ~0.266 to ~0.878 ns/call |
| Stale + near-boundary + atomic demand counter | ~0.939 to ~7.403 ns/call |

These numbers motivated keeping the epoch mechanism.

They are **not** the final production claim.

The current benchmark now imports the production `runtime-core` and `runtime-selector` implementation directly, including the lock-free published-boundary snapshot. That matrix must be rerun before replacing the prior numbers.

## Current architecture

```text
native transition
+ ResourceEpoch
+ PublishedBoundary
+ FastRoute
+ near-boundary demand
+ bounded revalidation
+ conservative fallback
```

Not active requirements:

- continuous telemetry;
- online cost prediction;
- general scheduling;
- resource brokering;
- continuous rebalancing;
- periodic active probing.

## Remaining evidence

### Production hot path

Run the current `x64-hotpath-overhead` matrix against exact production code.

### Native event path

Establish:

```text
physical platform transition
-> native kernel / firmware signal
-> userspace adapter
-> ResourceEpoch increment
-> stale boundary observed
-> bounded runtime response
```

Intel HFI remains the first target.

See [Native Validation](./NATIVE_VALIDATION.md).
