# AlpenCat Next-Sentinel Value Research — 2026-10-07

> Phase: Generic policy research
> Runtime hot-path change: none
> Status: literature-grounded candidate design

## 1. Question

The current generic stopping criterion is easy to evaluate post hoc:

```
continue if H * Delta_G_next > Delta_C_probe_next
```

The runtime problem is that `Delta_G_next` is unknown until the next sentinel has already been measured.

The research question is therefore:

> How can AlpenCat estimate the value of buying the next observation without first paying the full observation cost?

This is an information-acquisition problem, not merely a boundary-search problem.

## 2. Literature interpretation

### Sequential Value of Information

Miller (1975), *The Value of Sequential Information*, shows that when information can be purchased sequentially, the value of one observation depends on both its own decision value and how it changes the decision to buy further observations.

Reference:
- https://doi.org/10.1287/mnsc.22.1.1

AlpenCat implication:
- next-sentinel value cannot be reduced to the most recent local slope;
- an observation has option value because it changes both routing and future search decisions.

### Approximate autotuning with confidence

Hutter and Solomonik (IPDPS 2021) use confidence intervals and selective execution to avoid repeatedly executing kernels once performance is sufficiently predictable.

Reference:
- https://arxiv.org/abs/2103.01304

AlpenCat implication:
- confidence can suppress repeated measurement;
- known/repeated regimes should become cheaper;
- prediction should replace execution only after evidence is sufficiently stable.

### Sequential tests can stop early

Sequential statistical testing literature shows that repeated evaluation can often stop once evidence is sufficient rather than paying a fixed resampling budget.

Relevant example:
- https://doi.org/10.1007/s10182-024-00495-1

AlpenCat implication:
- fixed p3/p5/p6 budgets are structurally inferior to evidence-dependent stopping;
- however, the stopping statistic must price the cost of a wrong routing decision, not merely statistical significance.

### Cost-aware best-arm identification

Cost-aware best-arm work explicitly treats each measurement as having a cost and seeks the best decision with minimum total sampling cost.

References:
- https://arxiv.org/abs/2402.16710
- https://papers.nips.cc/paper_files/paper/2025/hash/b0dfe8236981ba84b4d98a7ed0426f0b-Abstract-Conference.html

AlpenCat implication:
- observation cost and implementation reward should be optimized jointly;
- heterogeneous probe costs matter;
- the objective should be total cost, not a fixed confidence target.

### Information-directed sampling

Russo and Van Roy (2014) choose observations by balancing expected regret against information gain.

Reference:
- https://arxiv.org/abs/1403.5556

AlpenCat implication:
- the next sentinel should be judged by expected decision-relevant information, not geometric distance alone;
- a sentinel that is unlikely to change the action has low value even if cheap.

### Autotuning resource budgets

Recent autotuning work explicitly frames tuning overhead and application performance as one joint optimization problem.

Reference:
- https://doi.org/10.1016/j.parco.2025.103126

AlpenCat implication:
- probe budget is itself part of the optimization target;
- a policy that finds a better route but spends too much finding it is not better.

## 3. Important W2 falsification

The current W2 evidence rules out a naive local-gradient estimator.

Observed pattern:

```
p3 -> p4 : additional captured gain ~= 0
p4 -> p5 : material additional captured gain
p5 -> p6 : additional captured gain ~= 0
```

Therefore this rule is rejected:

```
if previous sentinel added no gain:
    stop
```

A local zero marginal step does not prove the value of the next observation is zero.

## 4. Candidate estimator families

### E1 — Myopic local marginal predictor

Estimate next value from the previous sentinel's measured marginal gain.

Advantages:
- nearly free;
- simple;
- generic.

Failure:
- W2 already contains a counterexample.

Decision:
- retain only as a weak baseline.

### E2 — Expected Value of Sample Information (EVSI) proxy

For a possible next sentinel:

```
EVSI_next ~= P(next observation changes action)
             * value_if_action_changes
             - probe_cost
```

The probability is not initially a production probability model. Offline candidates may estimate it from:
- paired-sample preference consistency;
- unresolved demand mass beyond the observed envelope;
- recurring-regime history;
- uncertainty in the current boundary/profile.

Advantages:
- directly matches the economic question;
- naturally stops when an observation cannot change the decision.

Risk:
- probability estimation can itself become expensive or model-heavy.

### E3 — Confidence-bound decision value

Construct lower/upper bounds on the value of the current candidate and unresolved region.

Possible behavior:

```
if lower_bound(current action value) is already sufficient:
    act / stop probing

elif upper_bound(value of further information) < next probe cost:
    stop probing

else:
    buy one more observation
```

Advantages:
- can remain model-light;
- aligns with selective execution / sequential testing;
- does not require CPU-vendor features.

Risk:
- requires a defensible bound for unresolved region value.

### E4 — Opportunistic evidence acquisition

Do not immediately buy a dedicated next sentinel.

Instead:
- publish only within the observed safe envelope;
- continue serving real calls;
- when real demand naturally reaches the unresolved region, use a small fraction of those calls for alternate-route verification;
- update the profile from those observations.

Advantages:
- converts some dedicated probe cost into naturally useful work;
- particularly attractive for long horizons;
- strongly aligned with AlpenCat's lazy principle.

Risk:
- evidence arrives only if demand reaches the unresolved region;
- alternate-route shadow execution is still not free;
- latency-sensitive calls may prohibit experimentation.

### E5 — Experience-store prior

For a previously seen resource/workload class:
- reuse historical sentinel outcomes as a prior;
- require only lightweight confirmation.

Advantages:
- repeated regimes become progressively cheaper;
- supported by approximate autotuning literature.

Risk:
- cold-start generic behavior still needs a solution;
- stale history must not become a hidden vendor/model policy.

## 5. Current preferred generic architecture

Do not choose one estimator yet.

Test a two-stage design:

```
Stage A: conservative confidence/value bounds
    |
    |-- decisive stay/act -> stop
    |
    '-- unresolved
          |
Stage B: EVSI gate
          |
          |-- dedicated next sentinel if VOI clearly exceeds cost
          |
          '-- otherwise defer to opportunistic evidence
```

Experience-store information may tighten the bounds later but is not required for cold start.

## 6. Offline comparison required

Compare at least:

1. Fixed p3;
2. Fixed p5;
3. myopic last-marginal stopping;
4. confidence-bound stopping;
5. EVSI proxy;
6. opportunistic/deferred observation;
7. hindsight upper bound.

Evaluate on:
- W1 low-opportunity control;
- W2 positive opportunity;
- W3 movement-without-economic-opportunity control.

Metrics:
- total net gain;
- Oracle regret captured;
- dedicated probe cost;
- number of extra observations;
- wrong action rate;
- time/horizon to first profitable action;
- gap to hindsight upper bound.

## 7. Generic-first guardrail

Estimator inputs must remain vendor-agnostic.

Allowed:
- timing samples;
- confidence intervals;
- observed demand;
- horizon;
- probe/switch/verify cost;
- resource-state class;
- prior outcomes from the same abstract regime/workload class.

Not allowed in Generic layer:
- Intel/AMD/Apple branch;
- CPU model IDs;
- per-model thresholds.

## 8. Immediate next experiment

Build an offline estimator benchmark rather than modify Runtime Core.

First falsification targets:

- myopic marginal estimator should fail on the known W2 p3->p4->p5 pattern;
- confidence/EVSI candidates must keep W3 conservative;
- a successful candidate should approach the W2 hindsight stopping envelope without hard-coding p3/p5.

Only after an estimator survives W1/W2/W3 should any online state be considered for Runtime Core.
