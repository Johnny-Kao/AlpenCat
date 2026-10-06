# AlpenCat Convergence 80% Checkpoint — 2026-10-06

## Status

The engineering-spine target for the Convergence Phase is reached subject to the branch-head Convergence Smoke remaining green.

This checkpoint does **not** claim that AlpenCat's economic thesis is validated. It records that the project now has an executable path capable of testing that thesis.

## Completed

### T0 — Phase freeze

- Convergence plan is frozen.
- Native-signal/vendor expansion is deferred.
- Current active question is action + recoverable compute economics.

### T1 — SDK closed loop

The runtime now supports:

```text
ResourceEpoch invalidation
    -> stale published boundary
    -> bounded serial/CPU revalidation
    -> publish only observed local crossover
    -> future Auto call uses updated boundary
    -> real backend executes
```

Safety rules now encoded:

1. local evidence stays local;
2. no local crossover does not imply global serial fallback;
3. a resource change during measurement prevents the result from becoming fresh;
4. route availability is explicit;
5. a fallback execution is not allowed to masquerade as timing evidence for the requested route.

### T2 — First real workload

Current workload:

- `compute-mix-128`;
- deterministic 128-round integer mixing kernel;
- equivalent Serial and CPU implementations through the production runtime API;
- output equivalence checked before measurement;
- actual route timing used by bounded revalidation;
- Auto execution performed after revalidation.

Initial smoke observation on a GitHub-hosted Ubuntu runner:

- 4 effective CPUs:
  - local crossover observed;
  - boundary published;
  - `serial_max_items = 256`;
  - subsequent probe selected CPU;
  - boundary fresh.
- 1 effective CPU:
  - requested CPU route was not actually available;
  - status = `RouteUnavailable(Cpu)`;
  - no false crossover published;
  - actual execution remained Serial;
  - boundary remained stale.

These observations are smoke evidence, not performance claims.

### T3 — Resource-envelope harness

The harness now:

- builds the real workload once;
- records Git SHA and machine/resource manifest;
- runs the workload under full process affinity;
- reruns under single-CPU affinity;
- emits machine-readable JSONL;
- validates required result fields;
- preserves the evidence as a short-retention Actions artifact.

The harness is structured so additional controlled regimes can be batched without redesigning the SDK.

### T4 — Smoke CI

The branch workflow checks:

- rustfmt;
- Clippy with warnings denied;
- runtime-api tests, including convergence regressions;
- real release-mode workload execution;
- full-affinity and single-CPU resource envelopes;
- evidence artifact upload.

## Important finding from the smoke pass

The first workload run exposed a real contract bug:

```text
requested CPU route
-> CPU adapter falls back to Serial because only one CPU is available
-> naive timer labels elapsed time as CPU evidence
-> false crossover can be inferred
```

The corrected contract is:

```text
requested route
-> observe actual executed backend
-> requested backend unavailable?
     yes -> RouteUnavailable
            do not use timing as route evidence
            do not publish a false boundary
     no  -> timing may enter bounded local comparison
```

This is now covered by regression tests.

## What the 80% checkpoint means

Completed:

```text
PLAN FROZEN
+
SDK CLOSED LOOP EXECUTES
+
ONE REAL WORKLOAD EXECUTES
+
RESOURCE-ENVELOPE HARNESS EXECUTES
+
SMOKE CI GREEN
+
NEXT DECISION BATCH DEFINED
```

Not completed:

- final cloud economics;
- W2 memory-sensitive workload;
- W3 mixed workload;
- Static / Periodic / AlpenCat / Oracle comparative economics;
- statistically meaningful replication;
- Intel/AMD/ARM/Android/GPU validation;
- Experience Store;
- fleet layer.

Those are the remaining evidence/expansion work.

## Next decision batch

Do not change architecture first.

### Workloads

- W1: current compute-bound `compute-mix-128`;
- W2: memory-sensitive equivalent-route workload;
- W3: mixed compute/memory equivalent-route workload.

### Resource regimes

Batch on one controlled Linux environment first:

1. full effective CPU set;
2. half effective CPU set;
3. one CPU;
4. external CPU contention;
5. recovery to baseline.

Add CPU quota / NUMA only after the first batch shows a decision-relevant gap.

### Policies

For every workload/regime compare:

1. Static;
2. Periodic/eager recalibration;
3. AlpenCat bounded lazy revalidation;
4. Oracle/best observed route.

### Required evidence

- raw route timings;
- actual executed backend;
- output-equivalence result;
- resource state / environment fingerprint;
- revalidation measurements and cost;
- static regret;
- AlpenCat regret;
- periodic-policy cost;
- oracle cost;
- route changes;
- control overhead;
- recovered fraction of oracle benefit.

### Primary decision metric

```text
net compute avoided per 1,000 compute-hours
after AlpenCat control/revalidation cost
```

### Decision

- <0.1% recoverable compute benefit: pause/rethink target;
- 0.1%-1%: narrow to economically relevant workloads;
- >1% across multiple workloads/regimes: proceed to vendor/cross-architecture validation;
- sustained 3%-5%+ with negligible control cost: treat as a serious product/OSS-platform signal.

These are internal research gates, not public claims.

## Next action

Stop architecture expansion.

The next engineering work is the evidence batch:

```text
W1 + W2 + W3
x
full / half / one / contention / recovery
x
Static / Periodic / AlpenCat / Oracle
```

The purpose is to decide whether AlpenCat earns the remaining 20%.
