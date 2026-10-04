# Validation Status

> Updated: 2026-10-04

This document separates what AlpenCat has already established from what remains unverified.

## Status summary

| Question | Status | Evidence |
| --- | --- | --- |
| Can a calibrated execution boundary become wrong after compute conditions change? | Validated | Controlled x64 regime changes |
| Should every resource transition trigger immediate recalibration? | Falsified | Mild transitions often had little or no regret |
| Is unbounded crossover search acceptable? | Falsified | Search became expensive when the parallel route disappeared |
| Does bounded local revalidation materially reduce search cost? | Validated | x64 boundary-economics experiments |
| Is a single fixed slowdown threshold a universal capacity-loss classifier? | Falsified | Intel/AMD multi-runner overlap |
| Can route slowdown still accelerate severe cases? | Supported | Strong contention was consistently visible |
| Is the normal ResourceEpoch hot path lightweight? | Provisionally validated | 8-runner hot-path microbenchmark |
| Does a real vendor-native hardware event reach AlpenCat end-to-end? | Open | Native adapter validation is next |

## Boundary economics

Primary workflow run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37156808130

The matrix used eight independently allocated GitHub-hosted x64 runners across Ubuntu 22.04 and 24.04.

Observed systems included Intel Xeon and AMD EPYC hardware.

### Multi-runner slowdown ranges

At the old crossover:

| Regime | Observed slowdown range |
| --- | ---: |
| Half effective CPU budget | 1.086x - 1.397x |
| One effective worker | 1.341x - 1.817x |
| Strong external contention | 1.518x - 2.233x |

Interpretation:

- half-budget and one-worker cases overlap;
- therefore a single slowdown threshold cannot identify all capacity-loss regimes;
- strong external contention remained clearly visible in all eight samples.

### Revalidation economics

Earlier controlled runs showed that bounded revalidation materially reduced pathological search cost.

Representative results:

| Regime | Full localized search | Bounded policy |
| --- | ---: | ---: |
| One-worker collapse | ~33 ms | ~1 ms |
| Strong contention | ~100 ms | ~1-2 ms |

Exact values varied across runner allocations. The stable conclusion is architectural: bounded local validation avoids paying the full cost of proving that a distant or nonexistent crossover exists.

## Hot-path overhead

Primary workflow run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37177541260

The benchmark compared:

1. baseline boundary routing;
2. boundary routing plus relaxed ResourceEpoch load/match;
3. stale epoch while far from the boundary;
4. stale + near-boundary bookkeeping with a deliberately pessimistic relaxed atomic increment.

Observed incremental cost versus baseline:

| Path | Incremental cost |
| --- | ---: |
| Normal epoch-match path | ~0.016 to ~0.514 ns/call |
| Stale but far from boundary | ~0.266 to ~0.878 ns/call |
| Stale + near boundary + atomic counter | ~0.939 to ~7.403 ns/call |

The percentage overhead can look large because the synthetic baseline itself is sub-nanosecond to roughly one nanosecond. Absolute nanoseconds are the relevant quantity.

The normal production path therefore remains compatible with AlpenCat's lightweight design goal.

The stale-near path is intentionally pessimistic and is not a requirement to perform an atomic increment on every call in production.

## Current architectural conclusion

The surviving mechanism is:

```text
native transition semantics
+ ResourceEpoch / stale
+ old boundary
+ near-boundary demand
+ optional routed-call slowdown accelerator
+ bounded revalidation
+ conservative fallback
```

The following are not currently justified as normal-path requirements:

- continuous telemetry;
- periodic probing;
- global performance prediction;
- online learning;
- a general scheduler;
- eager full recalibration.

## What remains unverified

The current experiments deliberately separate mechanism economics from vendor-native signal delivery.

Still required:

```text
real physical event
-> native kernel / firmware signal
-> userspace adapter
-> ResourceEpoch update
-> first affected near-boundary decision
```

For the first proof, Intel HFI is the leading target.

See [Native Validation](./NATIVE_VALIDATION.md).
