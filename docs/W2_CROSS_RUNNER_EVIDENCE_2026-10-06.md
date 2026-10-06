# W2 Cross-Runner Evidence — 2026-10-06

> Phase: E1 / W2  
> Status: cross-runner evidence checkpoint  
> Authoritative run: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37474606539

## Question

Does the W2 stale-boundary opportunity and a gated sentinel escalation policy generalize across independent cloud runners and CPU models?

The experiment used four independent Ubuntu 24.04 GitHub-hosted runners with:

- 2 paired warm-up rounds per workload size;
- 7 measured interleaved Serial/CPU pairs;
- the same W2 memory-sensitive workload;
- the same half-CPU and contention regimes;
- a 3-point local revalidation cost measurement;
- one high-end sentinel at 4,194,304 items;
- no runtime-core policy change.

## Results

| Runner | CPU | Regime | Static over Oracle | Sentinel | Sentinel margin | Consistency | All-Serial capture | Break-even |
|---|---|---|---:|---|---:|---:|---:|---:|
| 1 | AMD EPYC 7763 | half | 18.1% | Serial | 16.3% | 100.0% | 100% | 734 calls |
| 1 | AMD EPYC 7763 | contention | 16.5% | Serial | 13.2% | 85.7% | 100% | 800 calls |
| 2 | AMD EPYC 9V45 | half | 1.0% | CPU | 19.5% | 100.0% | 0% | n/a |
| 2 | AMD EPYC 9V45 | contention | 2.6% | CPU | 14.9% | 100.0% | 0% | n/a |
| 3 | AMD EPYC 7763 | half | 10.6% | Serial | 7.8% | 100.0% | 100% | 1,194 calls |
| 3 | AMD EPYC 7763 | contention | 11.6% | Serial | 7.5% | 100.0% | 100% | 1,099 calls |
| 4 | Intel Xeon 6973P-C | half | 4.3% | CPU | 1.5% | 71.4% | 0% | n/a |
| 4 | Intel Xeon 6973P-C | contention | 11.7% | Serial | 6.0% | 85.7% | 100% | 879 calls |

"Static over Oracle" is computed from the same warmed shared route evidence and means the execution cost penalty of retaining the baseline static boundary relative to the best measured route per point.

## Main conclusions

### 1. The opportunity is materially real on some machines

The stale-route loss is not a sub-0.1% artifact.

On the two EPYC 7763 runners, constrained regimes showed roughly 10.6%-18.1% excess execution cost relative to Oracle on the measured demand grid.

The Intel 6973P-C contention regime showed roughly 11.7%.

This is large enough to justify continued adaptation research.

### 2. One universal reaction would be wrong

The 4M sentinel produced opposite recommendations across machines:

- EPYC 7763: Serial strongly preferred under half/contention.
- EPYC 9V45: CPU still strongly preferred at the high end.
- Intel 6973P-C:
  - half: CPU still preferred, weak-to-moderate evidence;
  - contention: Serial preferred.

Therefore a rule such as:

```text
resource loss -> disable parallelism
```

would be incorrect.

The decision must remain evidence-driven and machine/regime specific.

### 3. The current lazy policy is too conservative in one identifiable case

Current behavior:

```text
3-point local search
-> all local points favor Serial
-> no crossover observed
-> keep old boundary stale
-> old CPU route remains eligible
```

On EPYC 7763 and Intel contention, this leaves material stale-route regret unrecovered.

A gated high-end sentinel can distinguish:

```text
CPU route still valuable at large work
    -> do not collapse to all-Serial

CPU route also loses at large work
    -> all-Serial candidate becomes plausible
```

### 4. Economic gating is necessary

Even when the sentinel supports all-Serial, payback is not immediate.

Observed break-even estimates ranged from roughly 734 to 1,194 relevant calls.

Therefore "signal -> revalidate -> publish" is still too eager as a universal rule.

A production policy must account for expected regime lifetime / future relevant demand.

### 5. A2 is supported; A3 is not currently supported

Earlier 3-repeat W2 runs suggested non-monotonic Serial->CPU->Serial profiles.

After:

- paired interleaving;
- explicit warm-up;
- 7 measured pairs;
- size-order variation;
- 6 independent stability blocks per constrained regime;

the profile became monotonic and stable.

The richer interval-profile hypothesis is therefore deferred.

Do not add a two-boundary production profile based on the earlier noisy evidence.

## Candidate policy — research only

The smallest supported policy is now:

```text
resource transition
-> stale

3-point local probe
-> crossover found
     -> normal bounded publication

-> no crossover, all evidence points toward Serial
     -> economic gate
        expected relevant calls >= estimated break-even?
           no  -> remain stale / wait
           yes -> high-end sentinel

sentinel says CPU still wins
     -> do not collapse route space

sentinel says Serial wins with sufficient confidence/margin
     -> candidate conservative all-Serial state

candidate
-> post-action verification
-> keep / rollback
```

This is not yet a runtime-core change.

## What is still missing before production implementation

1. A runtime estimate of future relevant call horizon / regime lifetime.
2. A confidence representation richer than one median cost.
3. A post-action verification / rollback contract.
4. Replication on additional physical/cloud machines.
5. A second workload family showing the same economic control pattern.

## Decision

W2 opportunity gate: **PASS**

Universal threshold / universal reaction hypothesis: **REJECTED**

Single scalar boundary representation: **still sufficient for current warmed W2 evidence**

Economic-gated directional escalation: **SUPPORTED AS NEXT CANDIDATE**

Production implementation: **NOT YET APPROVED**
