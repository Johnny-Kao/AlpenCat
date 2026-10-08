# Universal Generic Holdout Decision — 2026-10-08

> Decision: STOP Generic refinement
> Runtime hot-path change: none
> Final holdout: GitHub Actions run 37706179018

## Final holdout

Seven heterogeneous GitHub-hosted runner classes were tested with the frozen finalist set:

- Linux x64 standard
- Linux x64 1 CPU / ~5 GB
- Linux ARM64 standard
- macOS Apple M1 ARM64
- macOS Intel x64
- Windows x64
- Windows ARM64

Each runner executed the same three workload families across six resource regimes.

Total cases: 126.

Raw current Generic economics remained poor:

- positive-case fraction: 2.38%
- negative-case fraction: 97.62%
- median savings vs static: -0.665%
- worst savings vs static: -11.714%
- opportunity cases: 47
- median Oracle capture when opportunity existed: 0%

This confirms that additional candidate/boundary refinement is not the right Generic-layer research direction.

## Frozen finalist results

### F0 — structural eligibility shell

Rule:

```
logical_cpus > 1
AND resource transition occurred
```

Final holdout:

- opportunity recall: 95.74%
- missed opportunity cases: 2 / 47
- no-opportunity pruning: 43.04%
- gated negative-case fraction: 69.05%
- gated median savings: -0.0133%
- gated worst savings: -3.892%

Interpretation:

F0 is useful as a cheap structural exclusion shell, but it is not sufficient as an autonomous adaptation policy.

### F1 — natural disjoint envelope

Final holdout:

- opportunity recall: 76.60%
- missed opportunity cases: 11 / 47
- no-opportunity pruning: 54.43%
- gated negative-case fraction: 54.76%
- gated median savings: -0.00337%
- gated worst savings: -3.892%

Decision: reject as Generic policy.

The stronger pruning is purchased with too much lost opportunity and does not eliminate material downside.

### F2 — economic natural slowdown

Rule:

```
natural slowdown exposure
>
last known revalidation cost
```

Final holdout:

- opportunity recall: 91.49%
- missed opportunity cases: 4 / 47
- no-opportunity pruning: 50.63%
- gated negative-case fraction: 62.70%
- gated median savings: -0.00683%
- gated worst savings: -3.892%

Decision: reject as autonomous Generic adaptation policy.

The economic framing is preferable to magic thresholds and substantially improves downside relative to raw Generic execution, but the holdout still contains too many negative decisions.

## GO / STOP decision

Generic AlpenCat does not pass the bar for autonomous performance optimization.

Do not continue creating additional Generic gates, estimators, bridge rules, or workload-specific thresholds.

Freeze the Generic layer as a conservative shell whose responsibilities are:

1. detect whether parallel execution is structurally possible;
2. detect whether a meaningful resource transition occurred;
3. expose cheap existing execution history;
4. allow higher layers to decide whether specialized adaptation is worth attempting;
5. default to no-op when specialized evidence is absent.

The Generic layer should not promise route optimization by itself.

## What was learned

The main result is architectural:

> The portable part of AlpenCat is better treated as a low-cost control and eligibility substrate, not as the layer that captures most backend-selection performance.

Cross-platform performance opportunity exists, but the useful signals are not sufficiently stable or informative under one universal estimator to justify autonomous Generic revalidation.

The next performance layer should therefore specialize signals while preserving the same economic control principles.

## Next stage

Move to specialization in this order:

1. Linux server x86: AMD / Intel
   - stable server environment;
   - rich topology/resource information;
   - PSI/cgroup signals;
   - NUMA/cache/topology differences;
   - high-value parallel workloads.

2. Apple Silicon
   - P/E core asymmetry;
   - ThermalState;
   - unified memory;
   - lower core-count and different scheduling behavior.

3. Windows x64 / ARM64
   - use platform-native resource and topology signals after the server/Apple abstractions are clearer.

Specialization must retain the same economic contract:

```
cheap signal
→ material opportunity?
→ candidate
→ gain > measured adaptation cost?
→ switch
→ verify
→ keep / rollback
```

Vendor/platform layers may improve the signal quality, prior, and observation cost. They must not replace the economic contract with model-specific magic constants.

## Research guardrail

The final holdout is closed.

Do not alter F0/F1/F2 and rerun the same holdout to improve their scores.

Any future Generic change requires a genuinely new independent hypothesis and independent validation set.
