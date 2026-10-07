# Universal Eligibility Gate v1 — 2026-10-07

## Purpose

Reduce unnecessary Generic-layer observation before any dedicated probing occurs.

This is intentionally a coarse exclusion gate, not a routing optimizer.

## Inputs

Only cheap, platform-independent pre-probe facts:

- logical CPU count;
- whether a resource transition has actually occurred.

No route timing, no sentinel search, no vendor/model branch, no tuned threshold.

## v1 rule

```
if logical_cpus <= 1:
    skip adaptation

if no resource transition:
    skip revalidation

otherwise:
    eligible for the next Generic stage
```

## Cross-machine retrospective

Dataset:

- 7 GitHub-hosted runner classes;
- Linux / macOS / Windows;
- x64 / ARM64;
- 3 workloads;
- 6 resource regimes;
- 126 total cases.

Observed:

- total opportunity cases: 41;
- v1 skipped cases: 36 / 126;
- skipped no-opportunity cases: 36;
- missed opportunity cases: 0;
- opportunity recall: 100%.

This means the v1 exclusion rule removes 28.6% of all tested cases without losing any measured opportunity in this sweep.

This is not yet proof of universality. The rule must remain unchanged in future heterogeneous runs.

## Interpretation

The result supports an architectural principle:

> Generic AlpenCat should first prove that adaptation is eligible before paying to observe route economics.

This is preferable to attempting increasingly precise candidate estimation on every resource event.

## Next validation

Keep v1 fixed.

Repeat on heterogeneous machines/runs and evaluate:

- opportunity recall;
- skipped no-opportunity fraction;
- worst downside after the gate;
- whether additional cheap signals can remove more false-positive eligible cases without reducing recall.

Do not add a new signal merely because it improves this dataset.
