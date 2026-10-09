# AlpenCat Research Handoff — Foundation PASS — 2026-10-09

> Branch: `research/convergence-adaptive-compute-sdk`
> Current phase: Foundation stabilized; specialization has NOT started
> Authoritative Foundation Gate: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37869233762
> Result: PASS

## 1. Executive state

The project has completed two major closures:

1. **Generic autonomous optimization is closed / STOP.**
2. **Foundation Stability Gate now passes on the current branch.**

Do not reopen Generic estimator tuning, and do not reinterpret this PASS as meaning platform/vendor specialization is already implemented.

The current state is:

```
stable passive foundation
        ↓
Generic autonomous optimizer: STOP
        ↓
next research stage: platform-aware specialization
```

## 2. What is now considered stable

The following foundation primitives are considered stable enough to build on:

- `PublishedBoundary`
- `ResourceEpoch`
- `FastRoute`
- bounded local revalidation semantics
- CPU execution-budget handling
- cross-platform correctness harness
- production hot-path baseline / gross-regression checks

The stability claim is about the passive/runtime foundation, not autonomous optimization quality.

## 3. Authoritative Foundation Stability Gate

Workflow:

- `.github/workflows/foundation-stability-gate.yml`

Authoritative run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37869233762

Result:

- **SUCCESS**
- 14/14 correctness jobs passed
- 8/8 hot-path jobs passed
- final foundation-gate job passed

Coverage:

- Linux x64 standard × 2
- Linux x64 1 CPU / ~5 GB × 2
- Linux ARM64 × 2
- macOS Apple Silicon × 2
- macOS Intel × 2
- Windows x64 × 2
- Windows ARM64 × 2
- Ubuntu 22.04 hot-path × 4
- Ubuntu 24.04 hot-path × 4

Validated invariants include:

1. no torn `PublishedBoundary` snapshots under stress;
2. correct `ResourceEpoch` stale/fresh behavior;
3. `FastRoute` equivalence with the reference model;
4. fresh revalidation performs zero measurement and publishes nothing;
5. invalidation during measurement cannot publish fresh state;
6. no-local-crossover remains local and does not extrapolate a global fallback;
7. unavailable route is not treated as timing evidence;
8. CPU execution-budget behavior is host-aware;
9. current production hot path shows no gross regression.

## 4. Important issue discovered and fixed during Foundation Gate

The first unified Foundation Gate exposed two failures only on the single-CPU `ubuntu-slim` runner.

Failed tests:

- `same_budget_reuses_pool`
- `different_budgets_get_distinct_cached_pools`

Root cause:

The tests assumed requested budgets 2 and 3 necessarily create parallel Rayon pools.

That assumption is invalid on a 1-CPU host because:

```
effective_parallelism = min(requested_budget, available_parallelism)
```

Therefore both budgets collapse to effective parallelism 1, and no parallel pool should be created.

The runtime behavior was correct; the test contract was not portable.

Fix:

- tests now reason about **effective parallelism**, not requested budget;
- single-core hosts explicitly validate that no parallel pool is created;
- distinct cached pools are required only for distinct effective parallelism > 1.

Fix commit:

- `3c3d4e89386b87105ec5d60e9f5fc9e115c45021`

Rerun commit:

- `4421faa4be2ae134811d3e1e1d27865e2d56b0ca`

## 5. Convergence Smoke status

Latest successful run:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37714643781

Result:

- **SUCCESS**

The earlier failure was only a formatting issue introduced while adding the fresh-revalidation invariant test.

That formatting issue was fixed before the authoritative Foundation Gate.

## 6. Generic research is closed

Canonical decision:

- `docs/UNIVERSAL_GENERIC_HOLDOUT_DECISION_2026-10-08.md`

Final Generic holdout:

- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37706179018

Final conclusion:

> Generic AlpenCat should not be an autonomous universal optimizer.

The portable layer is retained as a conservative substrate.

Do not add additional Generic gates just to improve the closed holdout.

The current architectural split is:

```
Generic passive foundation
    |
    +-- capability/state primitives
    +-- ResourceEpoch
    +-- PublishedBoundary
    +-- FastRoute
    +-- execution history / measurement infrastructure
    |
    v
platform-aware layer
    |
    +-- Linux
    +-- Apple
    +-- Windows
    |
    v
vendor specialization where justified
    |
    +-- Intel
    +-- AMD
```

## 7. Current runtime architecture

The surviving control law is still:

```
cheap signal
-> material opportunity?
-> candidate
-> expected gain > measured adaptation cost?
-> switch
-> verify
-> keep / rollback
```

Platform/vendor specialization may improve signal quality and priors.

It must not replace the economic contract with CPU-model-specific magic constants.

## 8. What has NOT started yet

Do not claim these are implemented:

- Linux PSI-based eligibility;
- cgroup-aware optimization;
- NUMA-aware routing;
- Intel HFI integration;
- AMD CPPC integration;
- Apple ThermalState integration;
- vendor-specific route selection.

These remain the next research layer.

## 9. Next-stage entry point

Canonical checkpoint:

- `docs/LINUX_X86_SPECIALIZATION_CHECKPOINT_2026-10-08.md`

The next recommended phase is **Linux x86 platform-aware signal inventory**, before splitting Intel and AMD.

Start with one batched matrix that inventories:

- `/proc/pressure/cpu`
- `/proc/pressure/memory`
- `/proc/pressure/io`
- cgroup v2 `cpu.max`
- effective cpuset
- `memory.current`
- `memory.max`
- effective CPU count
- `lscpu -J`
- NUMA node topology

CPU vendor/model may be recorded as metadata only, not as policy input.

## 10. Next research question

> Can cheap Linux-native state distinguish meaningful resource transitions from harmless ones materially better than the frozen Generic shell, without route probing?

This is now the highest-value question.

If yes:
- build a Linux platform-aware eligibility layer;
- then test whether Intel/AMD-specific signals add enough incremental value.

If no:
- do not force a Linux generic signal layer;
- move directly to native/vendor physical-server evidence.

## 11. Execution rules for the next section

1. Do not modify the Foundation primitives unless a new failing invariant requires it.
2. Do not reopen the final Generic holdout.
3. Keep specialization research off the hot path initially.
4. Prefer one matrix run that inventories many signals over one Actions run per signal.
5. Every new Actions test must report its URL immediately.
6. Do not run experiments on the user's personal Mac unless explicitly required/approved.
7. Physical-server-only claims must remain clearly marked as unvalidated until physical evidence exists.
8. Keep GitHub Actions noise low; delete only failed/cancelled superseded runs after a successful authoritative replacement exists.

## 12. Immediate next action

Read, in order:

1. `docs/RESEARCH_HANDOFF_2026-10-09_FOUNDATION_PASS.md`
2. `docs/FOUNDATION_STABILITY_GATE_2026-10-08.md`
3. `docs/UNIVERSAL_GENERIC_HOLDOUT_DECISION_2026-10-08.md`
4. `docs/LINUX_X86_SPECIALIZATION_CHECKPOINT_2026-10-08.md`
5. `docs/ARCHITECTURE.md`

Then begin only with the batched Linux x86 signal-inventory design.

Do not start Intel/AMD tuning before the shared Linux x86 evidence is understood.
