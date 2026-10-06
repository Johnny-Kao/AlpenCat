# W2 Holdout Economics Checkpoint — 2026-10-06

> Phase: E1 / W2
> Status: independent holdout validation complete
> Runtime core change: none
> Authoritative confirmatory run: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37480459630

## Question

Can an economic-gated directional adaptation policy chosen from one warmed/interleaved W2 evidence batch transfer to a completely independent evidence batch on the same cloud runner?

This checkpoint intentionally separates:

- training evidence: chooses candidate direction and estimates break-even;
- holdout evidence: independently prices the action.

The holdout data is not used to choose the candidate.

## Why holdout was required

The first cross-runner simulator used the same evidence to estimate break-even and to price the resulting action. That was useful for mechanism sensitivity but optimistic by construction.

The first independent holdout run showed the exact failure mode we wanted to detect:

- at a 1,000-call horizon, several candidates whose estimated break-even was only slightly below the horizon became negative out of sample;
- the same directional candidates became strongly positive at a 10,000-call horizon.

This established that estimated break-even is uncertain and should not be treated as a hard exact threshold.

## First holdout result

Authoritative first holdout:
- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37479474369

At 1,000 relevant calls:
- 4 candidate actions under the broad gate;
- 1 positive;
- 3 negative;
- mean saving: -0.469%;
- worst case: -3.274%.

At 10,000 relevant calls:
- 6 candidate actions under the broad gate;
- 6 positive;
- 0 negative;
- mean saving: +7.175%;
- available holdout regret captured: 87.1%.

Interpretation:

> Acting immediately when `horizon >= estimated_break_even` is not robust near the estimated break-even point.

The direction can be right while the timing of commitment is too aggressive.

## Break-even safety-factor hypothesis

A follow-up analysis introduced a research-only safety factor:

```text
act only if:
expected_horizon >= safety_factor * estimated_break_even
```

Factors tested:
- 1x;
- 2x;
- 4x.

This is sensitivity analysis, not a production constant.

## Independent confirmatory batch

Authoritative run:
- https://github.com/Johnny-Kao/AlpenCat/actions/runs/37480459630
- conclusion: SUCCESS
- 4 independent Ubuntu runners;
- each runner produced:
  - warmed/interleaved training evidence;
  - p3 sensitivity;
  - training-only escalation candidate;
  - a second independent warmed/interleaved holdout batch.

### 1,000-call horizon

At 1x safety:
- actions: 1;
- positive: 1;
- negative: 0;
- mean saving across all cases: +0.160%;
- holdout regret captured: 1.8%.

At 2x or 4x safety:
- actions: 0;
- positive: 0;
- negative: 0.

Interpretation:
- this confirmatory batch did not reproduce the first batch's negative 1,000-call actions;
- however, the first holdout already proves near-break-even transfer can fail;
- 2x protects against that class but may be unnecessarily conservative.

Therefore **2x is not approved as a universal runtime constant**.

### 10,000-call horizon

Broad gate: 70% consistency, 0% margin:
- actions: 8;
- positive: 8;
- negative: 0;
- mean saving: +8.333%;
- worst case: +4.512%;
- holdout regret captured: 87.7%.

70% consistency, 5% margin:
- actions: 6;
- positive: 6;
- negative: 0;
- mean saving: +5.375%;
- holdout regret captured: 57.8%.

85% consistency, 5% margin:
- actions: 5;
- positive: 5;
- negative: 0;
- mean saving: +4.804%;
- holdout regret captured: 51.3%.

10% margin:
- only 1 action remained;
- positive: 1;
- mean saving: +1.308%;
- holdout regret captured: 14.5%.

The 2x and 4x break-even safety factors did not change the 10,000-call decisions in this batch because qualifying estimated break-even values were already far below the horizon.

## Main conclusion

W2 now supports a stronger statement than the earlier in-sample result:

> **When a resource regime is expected to persist for many relevant calls, a small evidence-driven directional adaptation can produce material positive net value on independent holdout evidence.**

At short horizons, adaptation timing is uncertain and must remain conservative.

This directly supports the original lazy-design intuition, but refines it:

```text
too eager near break-even
    -> risk negative transfer

too lazy at long horizon
    -> leave large repeated regret unrecovered
```

The useful decision variable is therefore not a fixed delay or fixed probe count.

It is the relationship between:
- expected remaining relevant demand;
- uncertain break-even;
- evidence quality;
- candidate direction;
- adaptation/verification cost.

## What is rejected

Do not implement any of the following as universal rules:

- always adapt after a resource event;
- always wait N calls;
- always use 2x break-even;
- always require 5% or 10% route margin;
- resource loss always means Serial;
- one CPU model's behavior applies to all CPUs.

Cross-runner W2 evidence has already falsified these simplifications.

## What is currently supported

Research candidate:

```text
resource transition
-> mark stale

small local probe
-> ordinary crossover found?
     yes -> bounded candidate

-> no crossover / directional evidence
     -> estimate economic exposure
     -> short/uncertain horizon: stay conservative
     -> long/high-exposure horizon: bounded sentinel escalation

candidate
-> post-action verification
-> keep / rollback
```

The holdout evidence supports the economic-gate concept.

It does **not** yet approve production runtime policy.

## Why post-action verification remains useful

The first holdout showed negative near-break-even actions. A fixed safety multiplier can suppress them, but it can also suppress valid early wins.

A smaller long-term mechanism is likely:

```text
candidate action
-> verify a small number of real calls
-> benefit persists?
     yes -> commit / confidence++
     no  -> rollback / confidence--
```

This may dominate a large universal break-even safety factor.

Do not implement it in the hot path until another workload family shows a similar need.

## Decision

W2 recoverable-regret opportunity: **PASS**

W2 cross-runner generalization: **PASS**

Independent holdout transfer at long horizon: **PASS**

Near-break-even hard threshold: **REJECTED**

Universal 2x break-even safety factor: **NOT APPROVED**

Post-action verification: **SUPPORTED AS NEXT SAFETY CANDIDATE**

Runtime-core policy implementation: **DEFER**

## Next gate

Proceed to W3 mixed compute/memory workload using the same evidence contract.

Reason:
- W1 is a low-opportunity compute control;
- W2 demonstrates a real memory-sensitive economic opportunity;
- before moving confidence/hysteresis/verification state into runtime core, verify that the same economic-control pattern exists outside W2.

If W3 also shows:
- stable recoverable regret under at least one regime;
- long-horizon positive holdout economics;
- need for conservative short-horizon behavior;

then the adaptation-policy abstraction has earned implementation work.
