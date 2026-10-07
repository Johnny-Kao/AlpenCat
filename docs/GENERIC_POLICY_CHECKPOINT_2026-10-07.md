# AlpenCat Generic Policy Checkpoint — 2026-10-07

> Branch: `research/convergence-adaptive-compute-sdk`
> Scope: research checkpoint only
> Runtime hot-path change: none
> Status: frozen research baseline for next-stage work

## 1. Current conclusion

AlpenCat's research question has moved beyond "how many calibration points should we use?"

The current generic-control question is:

> When a resource transition makes the current execution boundary potentially stale, when is it economically rational to spend more observation cost, how far should bounded evidence gathering proceed, and when should a candidate be switched, verified, kept, or rolled back?

The target is minimum total execution cost over time, not maximum instantaneous route accuracy.

## 2. Evidence state

### E0

Evidence infrastructure is complete and validated.

### W1

Low-opportunity control workload.

Interpretation:
- unnecessary adaptation can cost more than stale-route regret;
- a generic policy must be able to decide to stay.

### W2

Memory-sensitive workload with real recoverable regret under restricted/contention regimes.

Established:
- economic opportunity exists;
- long-horizon independent holdout adaptation can be strongly positive;
- near-break-even hard thresholds are not robust;
- current bounded revalidation can miss a useful candidate even when recoverable regret is material.

Recent diagnosis:
- p3-p6 can all return `NoLocalCrossover`;
- strong one-sided evidence may exist even when publication is forbidden by the current crossover-only contract;
- adding probe points mechanically is not the target policy.

### W3

Mixed compute/memory workload.

Extended grid exposed a real resource-dependent boundary movement that the original >=4096 grid missed.

However, under the measured demand weighting:
- boundary movement exists;
- static cost can still equal Oracle cost;
- therefore movement alone is not an economic reason to adapt.

W3 is currently a useful negative/control case for economic gating.

## 3. Control decomposition

The problem is now explicitly separated into three layers:

1. Opportunity economics
   - Is there economically recoverable stale-route regret?

2. Candidate quality
   - Does bounded evidence expose a safe candidate that captures the opportunity?

3. Switching economics
   - After a candidate exists, does expected future gain exceed revalidation + switch + verify + expected rollback cost?

These layers must not be collapsed into one "gain" metric.

## 4. One-sided sentinel hypothesis

When local evidence is one-sided and no crossover is observed:

- do not extrapolate globally;
- do not publish beyond observed evidence;
- optionally escalate to an additional sentinel only when economics justify the marginal observation cost;
- a candidate boundary may extend only to the farthest actually observed sentinel.

This keeps the search conservative while allowing economically meaningful directional evidence to be used.

## 5. Marginal stopping criterion

For the next sentinel:

```
Delta_G = expected additional recoverable gain per future relevant call
Delta_C = additional observation/probe cost
H       = expected remaining relevant calls
```

Research criterion:

```
continue only if H * Delta_G > Delta_C
```

Current W2 evidence demonstrates:
- an additional sentinel can add cost with zero additional recoverable gain;
- a later sentinel can expose a materially larger safe observed envelope;
- probing beyond full captured opportunity adds cost without benefit.

Therefore fixed p3/p5/p6 probe budgets are rejected as the target abstraction.

## 6. Generic-first portability constraint

The first production-worthy policy must remain vendor-agnostic.

Generic policy may consume:
- observed route execution cost;
- probe/revalidation cost;
- switching/verification cost;
- confidence / preference consistency;
- expected remaining horizon;
- resource-state transition evidence;
- observed sentinel coverage;
- recoverable regret.

Generic policy must not branch on:
- CPU vendor;
- CPU model;
- vendor/model whitelists;
- fixed Intel/AMD/Apple-specific thresholds;
- hard-coded per-model probe budgets.

Specialization is a later layer:

```
Static
  -> Generic AlpenCat
  -> Platform-aware AlpenCat
  -> Vendor-optimized AlpenCat
  -> Machine-experienced AlpenCat
```

Platform/vendor specialization may provide better signals, priors, or cheaper observation, but should not replace the generic economic control law.

## 7. Hindsight upper bound

`analyze_generic_policy_upper_bound.py` provides a post-hoc benchmark envelope only.

It may answer:
- for a given measured evidence batch and horizon, what stopping point would have maximized net value?

It must not be treated as a deployable runtime policy because it uses realized future marginal gain.

Its purpose is to define the performance envelope that an online estimator should approach.

## 8. Current bottleneck

Offline, `Delta_G` is measurable after the next sentinel has already been paid for.

At runtime, AlpenCat does not know the next sentinel's marginal value before deciding whether to pay for it.

Therefore the next research question is:

> How can AlpenCat estimate next-sentinel marginal value cheaply enough that the estimator itself does not erase the benefit of adaptive execution?

Candidate research families:
- sequential testing / value of information;
- optimal stopping;
- Bayesian or empirical confidence bounds;
- change-point / replay evidence;
- reuse of prior local profiles;
- monotonicity/shape constraints;
- opportunistic sampling from real calls rather than dedicated probes.

## 9. Guardrails for next stage

Do not:
- add CPU-vendor policy branches;
- hard-code p3/p5/p6;
- turn hindsight-optimal stopping into runtime logic;
- increase search radius without an economic hypothesis;
- implement confidence/hysteresis in the hot path before offline evidence supports the estimator.

Do:
- keep experiments offline/research-only;
- distinguish opportunity, candidate, and switching economics;
- measure estimator cost explicitly;
- validate on W2 and W3;
- preserve W1 as a low-opportunity control.
