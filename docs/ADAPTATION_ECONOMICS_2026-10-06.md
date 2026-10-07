# AlpenCat Adaptation Economics — Research Note

> Date: 2026-10-06  
> Phase: E1 / W2  
> Status: active design constraint  
> Purpose: decide when adaptation is economically justified before changing runtime policy

## 1. Problem

AlpenCat must avoid two symmetric failures:

~~~
too eager
  -> revalidate often
  -> control cost exceeds stale-route loss

too lazy
  -> keep a stale boundary too long
  -> repeated wrong-route calls compound into large waste
~~~

Because AlpenCat sits below repeated application calls, a small wrong decision can be multiplied many times. The policy therefore cannot be chosen from "3 points vs 5 points" alone.

The decision must be framed as an economic control problem.

## 2. External precedent

### Linux PSI — event thresholding and rate limiting

Linux Pressure Stall Information quantifies CPU, memory, and I/O contention and allows userspace to register threshold triggers over a time window. Notifications are rate-limited to one per tracking window, and the minimum monitoring window exists specifically to avoid excessively frequent monitoring.

Implication for AlpenCat:

- a resource signal should not imply immediate expensive action;
- accumulated impact over a window can justify escalation;
- event generation and adaptation spending should be separately rate-limited.

Reference:
- https://www.kernel.org/doc/html/latest/accounting/psi.html

### Intel HFI — update cadence is processor-model-specific

Intel Hardware Feedback Interface exposes per-CPU performance and energy-efficiency capabilities. Linux documentation explicitly notes that capability update rates differ by processor model: some remain fixed after boot while others can change on the order of tens of milliseconds.

Implication for AlpenCat:

- there cannot be one universal timing constant;
- platform capability should influence adaptation aggressiveness;
- a native signal is evidence that the prior execution boundary may be stale, not proof that immediate recalibration is profitable.

Reference:
- https://www.kernel.org/doc/html/latest/arch/x86/intel-hfi.html

### AMD CPPC / amd-pstate — abstract capability instead of raw frequency

AMD CPPC exposes an abstract continuous performance scale and delivered-performance feedback. AMD preferred-core rankings can change at runtime with workload, thermal, platform, and ageing conditions.

Implication for AlpenCat:

- prefer platform capability semantics over hard-coded MHz thresholds when available;
- resource state is multidimensional and hardware-specific;
- future machine profiles should be keyed by capability/resource class rather than CPU frequency alone.

References:
- https://www.kernel.org/doc/html/latest/admin-guide/pm/amd-pstate.html
- https://www.kernel.org/doc/html/latest/admin-guide/acpi/cppc_sysfs.html

### Selective autotuning — stop paying once confidence is sufficient

Hutter and Solomonik (2021) describe approximate autotuning using confidence intervals and selective kernel execution. Once performance becomes sufficiently predictable, repeated benchmark execution is avoided and prior statistical profiles are reused.

Implication for AlpenCat:

- repeated regimes should become cheaper over time;
- confidence belongs in the long-term profile model;
- a known regime should require confirmation, not full rediscovery.

Reference:
- https://arxiv.org/abs/2103.01304

### Dynamic GPU autotuning — repeated search can become unacceptable

GPU autotuning research notes that retuning overhead can become unacceptable when search spaces are large or tuning must be repeated due to changing data/hardware. Search should therefore be guided by prior information rather than restarted globally.

Implication for AlpenCat:

- preserve old boundary/profile information;
- local search is preferred to global recalibration;
- a resource transition should narrow the question rather than reset knowledge.

References:
- https://arxiv.org/abs/2102.05297
- https://arxiv.org/abs/1910.08498

### SMARTMoE — switching threshold and adaptation frequency

SMARTMoE separates offline candidate construction from lightweight online adaptation. Its runtime policy explicitly considers searching and switching cost, filters plan changes with only minor improvement, and shows the same frequency trade-off AlpenCat faces: searching every iteration adapts quickly but costs too much, while searching too infrequently lets performance drift. In one measured case, a roughly 20 ms switch was repaid by about 8 ms gain per step after roughly three steps.

Implication for AlpenCat:

- compare expected gain against switching/revalidation cost;
- exploit temporal locality instead of globally rediscovering a plan;
- keep the online candidate set/profile deliberately small;
- measure payback in future useful calls rather than treating revalidation cost in isolation.

Reference:
- https://www.usenix.org/conference/atc23/presentation/zhai

### SQL Server automatic tuning — economic gate, verify, revert

SQL Server automatic plan correction identifies plan regressions, applies a prior good plan, verifies the result after action, and reverts changes that do not improve performance. Its recommendations also expose estimated gain; Microsoft documentation uses an estimated CPU gain threshold before automatic forcing.

Implication for AlpenCat:

- action should be separated from verification;
- a candidate boundary should not become permanently trusted immediately;
- a failed adaptation should be rolled back or have confidence reduced;
- expected gain can be a first-class gate.

References:
- https://learn.microsoft.com/en-us/sql/relational-databases/automatic-tuning/automatic-tuning
- https://learn.microsoft.com/sql/relational-databases/system-dynamic-management-views/sys-dm-db-tuning-recommendations-transact-sql

## 3. Core economic model

The simplest useful inequality is:

~~~
expected future stale-route regret > adaptation cost
~~~

More explicitly:

~~~
P(boundary materially wrong)
× expected relevant calls before next regime change
× expected regret per wrong call
>
revalidation cost
~~~

This is not yet a production formula. It is the experiment contract.

For measured evidence, define:

~~~
C_revalidate = measured bounded revalidation cost

L_static = expected cost of keeping the old boundary
L_oracle = expected cost using the best observed route

R_cycle = L_static - L_oracle
~~~

For a declared workload-size distribution with K calls per cycle:

~~~
regret_per_call = R_cycle / K

break_even_calls = C_revalidate / regret_per_call
~~~

If recoverable regret is zero, revalidation has no economic break-even under that demand distribution.

## 4. Policy layers

The intended control loop is:

~~~
L0 FastRoute
   |
L1 Invalidate
   |
L2 Economic Gate
   |
L3 Bounded Revalidation
   |
L4 Candidate Publication
   |
L5 Post-action Verification
   |
keep / rollback / reduce confidence
~~~

Only L0-L3 exist in meaningful form today. L4-L5 remain research targets.

## 5. Required evidence before changing runtime policy

For each W2 regime, measure:

1. old/static boundary;
2. oracle boundary / route choice on the measured grid;
3. stale-route regret per declared demand cycle;
4. measured revalidation cost;
5. break-even calls;
6. route-margin near the crossover;
7. repeated-sample preference consistency;
8. candidate boundary selected by AlpenCat;
9. net value at several demand horizons.

Do not change max_points, hysteresis, or confidence rules until these values are visible.

## 6. W2 hypotheses

### A1 — Local search budget is too narrow

Prediction:
- meaningful stale regret exists;
- nearby evidence points consistently favor a different boundary;
- larger local budgets converge toward oracle with acceptable additional cost.

### A2 — Boundary evidence is noisy

Prediction:
- route winner flips across repeated/interleaved samples near crossover;
- widening search alone does not stabilize the result.

Response:
- confidence / repeated confirmation / hysteresis, not simply more points.

### A3 — A single scalar boundary is insufficient

Prediction:
- route preference is non-monotonic over workload size or depends strongly on regime dimensions not encoded by one crossover.

Response:
- richer but still bounded profile representation.

### A4 — Lazy delay is the dominant problem

Prediction:
- revalidation itself is cheap relative to accumulated stale regret;
- economics improve strongly as action occurs earlier.

Response:
- lower economic gate / faster escalation for that regime class.

### A5 — Revalidation cost dominates

Prediction:
- stale-route regret exists but break-even requires too many calls relative to regime lifetime.

Response:
- remain stale longer, reuse Experience Store, or do not adapt.

## 7. Hysteresis and post-action verification

Future candidate publication should require more than "route B measured slightly faster once."

Candidate rules to evaluate offline:

~~~
switch only if:
  estimated gain > switching margin
  AND evidence consistency >= threshold
  AND expected break-even occurs before likely regime expiry
~~~

After switching:

~~~
observe a small number of real calls
  -> improvement persists: increase confidence
  -> no improvement: rollback / reduce confidence
~~~

Do not add this to the hot path before W2 evidence supports it.

## 8. CPU model dependence

Vendor documentation already implies that runtime capability behavior differs materially by processor model.

Therefore long-term policy constants must not assume universal values such as:

- one fixed revalidation delay;
- one fixed slowdown threshold;
- one fixed HFI cadence;
- one fixed switching margin.

The current convergence experiments should first discover which quantities need machine-specific calibration. A later Experience Store can persist those quantities.

## 9. Immediate experiment

The next W2 step is analysis, not core modification.

Add an offline adaptation-economics analyzer that reports:

~~~
regime
old boundary
oracle boundary
recoverable regret per demand cycle
revalidation cost
break-even calls
near-boundary route margin
preference consistency
net value @ 10 / 100 / 1,000 / 10,000 calls
~~~

Then use those results to choose among A1-A5.

## 10. Decision rule

Only after the analyzer is green:

- A1 supported -> test bounded search policy;
- A2 supported -> add confidence/hysteresis experiment;
- A3 supported -> revisit profile representation;
- A4 supported -> experiment with earlier economic trigger;
- A5 supported -> stay lazy / reuse prior experience.

This keeps AlpenCat evidence-driven and prevents policy complexity from being added merely because it sounds plausible.


## 11. Minimal profile-shape hypothesis

W2 has exposed a concrete representation question.

A single scalar crossover can encode only:

~~~
Serial -> CPU
~~~

A memory-sensitive workload may instead exhibit:

~~~
Serial -> CPU -> Serial
~~~

under CPU restriction or contention.

Before changing Runtime Core, compare three offline representations on the same raw evidence:

1. one scalar boundary;
2. one bounded CPU interval with two boundaries;
3. per-point Oracle.

The interval representation is intentionally the smallest richer candidate:

~~~
Serial, n <= lower
CPU,    lower < n <= upper
Serial, n > upper
~~~

If two boundaries repeatedly recover nearly all of the Oracle gap while one boundary leaves material regret, A3 is supported without requiring a predictor, lookup table, or ML model.

If the interval also fails, do not keep adding boundaries mechanically; revisit the workload key and resource-state representation.


## 12. Generic-first portability constraint

The first production-worthy adaptation policy must be vendor-agnostic.

Generic policy MAY consume only measured or abstract runtime evidence such as:

- observed route execution cost;
- revalidation/probe cost;
- switching and verification cost;
- confidence / preference consistency;
- expected remaining horizon;
- resource-state transitions;
- observed sentinel coverage;
- recoverable regret.

Generic policy MUST NOT branch on:

- CPU vendor;
- CPU model;
- vendor/model whitelists;
- fixed Intel/AMD/Apple-specific thresholds;
- hard-coded per-model probe budgets.

Specialization is a later optimization layer. A platform/vendor adapter may provide better signals, priors, or cheaper observation, but should not replace the economic control law.

The intended ladder is:

~~~
Static
  -> Generic AlpenCat
  -> Platform-aware AlpenCat
  -> Vendor-optimized AlpenCat
  -> Machine-experienced AlpenCat
~~~

Cross-machine validation should therefore ask:

> Can the same vendor-agnostic policy reach economically sensible decisions from different local measurements?

It should not ask for separate Intel/AMD policy constants unless the generic abstraction is first shown to be insufficient.

## 13. Marginal sentinel stopping

A one-sided local miss does not justify unlimited probing.

For sentinel step i -> i+1 define:

~~~
Delta_G = additional recoverable gain per future relevant call
Delta_C = additional probe cost
H       = expected remaining relevant calls
~~~

Continue deeper only when:

~~~
H * Delta_G > Delta_C
~~~

This is a research criterion, not yet an online estimator.

Current W2 evidence shows why the distinction matters:

- one extra sentinel can add cost without adding recoverable value;
- a later sentinel can expose a materially larger safe observed envelope;
- probing beyond full captured opportunity adds no value.

Therefore a fixed p3/p5/p6 budget is not the target policy. The target is an economically stopped, vendor-agnostic sentinel search.

The hindsight upper-bound analyzer is explicitly an offline benchmark envelope. It must not be treated as a deployable decision rule because it uses realized post-hoc marginal gain.
