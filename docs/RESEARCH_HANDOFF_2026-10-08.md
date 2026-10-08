# AlpenCat Research Handoff — 2026-10-08

> Branch: `research/convergence-adaptive-compute-sdk`
> Phase: Generic convergence closed; platform specialization begins
> Runtime hot-path change: none

## Executive state

The Generic research line is closed.

Final heterogeneous holdout:

- GitHub Actions run: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37706179018
- 7 runner classes
- Linux / macOS / Windows
- x64 / ARM64
- 126 workload/regime cases

Raw Generic behavior remained negative:

- positive cases: 2.38%
- negative cases: 97.62%
- median savings vs static: -0.665%
- worst savings vs static: -11.714%
- opportunity cases: 47
- median Oracle capture when opportunity existed: 0%

Finalists:

- F0 structural shell: recall 95.74%, worst gated -3.892%
- F1 natural-disjoint: recall 76.60%, reject
- F2 economic-natural-slowdown: recall 91.49%, reject as autonomous Generic optimizer

Canonical decision:

- `docs/UNIVERSAL_GENERIC_HOLDOUT_DECISION_2026-10-08.md`

## Architecture decision

Generic AlpenCat is now a conservative substrate, not an autonomous performance optimizer.

Generic responsibilities:

1. expose whether parallel execution is structurally possible;
2. expose resource invalidation/epoch changes;
3. expose cheap existing execution history;
4. default to no-op when stronger evidence is absent;
5. provide a common economic contract for specialized layers.

Do not add more Generic gates or estimators to improve the closed holdout.

## Surviving control contract

All specialization layers must preserve:

```
cheap signal
-> material opportunity?
-> candidate
-> expected gain > measured adaptation cost?
-> switch
-> small verification
-> keep / rollback
```

Specialized layers may improve signal quality, priors, topology awareness, and observation cost.

They must not replace the economic contract with model-specific magic constants.

## Next research stage

Linux server x86 first.

Split the work into two levels.

### Level A — platform-aware Linux x86

Vendor-neutral inputs:

- Linux PSI
- cgroup CPU/memory pressure
- schedulable/effective CPU set
- topology / NUMA information
- current ResourceEpoch transition
- existing route execution history

Goal:

> Determine whether cheap Linux-native state can identify when Generic eligibility should escalate to a specialized adaptation attempt.

This level must work on both Intel and AMD.

### Level B — vendor specialization

Intel:
- HFI when available
- topology/cache/NUMA information
- frequency/capability asymmetry only when exposed cheaply

AMD:
- CPPC/capability signals when available
- CCD/CCX/topology/NUMA information
- frequency/capability asymmetry only when exposed cheaply

Vendor layers may change signal quality and priors, but not the economic switching contract.

## Native integration target

Existing architecture already identifies Intel HFI as the first native validation target.

The immediate implementation/research sequence is:

1. inventory current runtime-machine/native adapter hooks;
2. define a Linux x86 signal snapshot that is cheap and vendor-neutral;
3. validate PSI/cgroup/topology signal availability on GitHub Linux x64;
4. separate signals that are available in virtual CI from those requiring physical servers;
5. only then add Intel HFI / AMD-specific adapters;
6. physical-server native-event validation remains required before production claims.

## Guardrails

Do not:

- reopen Generic estimator tuning;
- fit thresholds to the final 7-runner holdout;
- branch policy on CPU model names;
- assume Intel/AMD signal availability in CI;
- claim HFI/CPPC behavior without physical/native evidence;
- run user-local machine tests without explicit need/approval.

Do:

- keep Generic shell unchanged;
- use GitHub Actions for portable/platform signal inventory;
- mark physical-server-only evidence explicitly;
- keep every test run URL in the work log;
- prefer one matrix run that inventories many signals over one workflow per signal.
