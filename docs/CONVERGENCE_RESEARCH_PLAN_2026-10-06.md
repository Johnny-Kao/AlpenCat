# AlpenCat Convergence Research Plan

> Date: 2026-10-06  
> Phase: Convergence  
> Status: Active research plan  
> Primary target: Adaptive Compute SDK  
> Execution repository: `Johnny-Kao/AlpenCat`

## 1. Phase transition

The exploratory architecture phase is substantially complete.

The previous phase established that:

- calibrated execution boundaries can become materially stale;
- not every resource transition justifies immediate recalibration;
- unbounded crossover search can cost more than stale-route regret;
- localized bounded revalidation avoids pathological search cost;
- local evidence must not be extrapolated globally;
- one universal routed-call slowdown threshold does not generalize;
- `ResourceEpoch + PublishedBoundary + FastRoute` can keep the normal routing path extremely small;
- native platform events are promising invalidation sources, but no single signal should define AlpenCat.

The next phase therefore stops expanding the sensor search and asks a narrower product/systems question:

> **Can AlpenCat take real execution action at near-static routing cost and recover enough compute waste on real workloads to justify adaptation?**

Physical Intel HFI validation remains valuable, but it is deferred until the SDK/workload/cloud thesis produces a clear GO signal.

## 2. Product boundary

AlpenCat is not a scheduler, monitoring suite, online ML system, or workload implementation framework.

The target abstraction is an **Adaptive Compute SDK**:

```text
application provides equivalent execution routes
                    |
                    v
              AlpenCat SDK
                    |
          current valid boundary
                    |
                    v
              choose / execute
                    |
                    v
       existing OS/runtime/backend executes
```

AlpenCat decides **how an already-correct workload should execute now**.

It must not own correctness of the underlying implementations. A wrong AlpenCat decision may be slower; it must not make the workload semantically wrong.

## 3. Convergence thesis

### Scientific thesis

Execution boundaries are state-dependent and can become stale.

### Systems thesis

Resource-state invalidation plus lazy localized revalidation can keep execution decisions aligned with changing resource conditions at close to static-routing cost.

### Economic thesis

At useful workload/fleet scale, compute saved by better execution decisions can materially exceed AlpenCat's observation, routing, and revalidation cost.

The Convergence Phase is primarily about the systems and economic theses.

## 4. Active architecture boundary

The intended long-term shape is:

```text
Signal Adapter
    |
    v
ResourceEpoch
    |
    v
Boundary / Profile
    |
    v
FastRoute
    |
    v
Action Adapter
    |
    v
real execution route
```

Signal sources may later include:

- cgroup/cpuset changes;
- PSI or other Linux resource transitions;
- Intel HFI;
- AMD platform signals;
- Android headroom / platform state;
- GPU/platform signals.

Action adapters may later control:

- serial vs parallel;
- worker/thread count;
- CPU vs GPU;
- batch size;
- equivalent algorithm/backend implementations.

The core must not depend on any one signal source or action type.

## 5. Current implementation reality

The current repository already contains part of the action loop.

`runtime-api` can:

- select Serial / CPU / GPU from a published boundary;
- execute the selected implementation;
- expose the resulting `ExecutionDecision`;
- invalidate resources;
- publish a new boundary.

Therefore Track 1 is **not** a greenfield SDK rewrite.

The missing production loop is primarily:

```text
resource invalidated
    -> stale state becomes relevant
    -> bounded/local revalidation is invoked
    -> validated state is published
    -> future real calls automatically take the updated route
    -> outcome is measurable
```

The public API also remains more backend-specific than the desired long-term "equivalent routes" abstraction. Generalization must be evidence-driven and minimal.

## 6. Active hypotheses

### H1 — Real action

A real workload with equivalent routes can be routed by AlpenCat, and a resource-state change can cause AlpenCat to take a different execution action after bounded revalidation.

**Pass condition:** a reproducible end-to-end workload changes route after an injected/controlled resource transition and preserves output equivalence.

### H2 — Recoverable regret

Static routing produces material execution regret under at least some realistic changing resource regimes.

**Pass condition:** real workloads show measurable, repeatable regret relative to an oracle/best-route reference.

### H3 — Near-static control cost

AlpenCat recovers a useful fraction of oracle benefit without turning the hot path into continuous telemetry/prediction.

**Pass condition:** control overhead remains negligible relative to recovered execution cost and the normal routing path remains close to current measured cost.

### H4 — Cloud relevance

Cloud/container resource variability creates enough recoverable regret for AlpenCat to have practical economic value.

**Pass condition:** controlled cloud experiments produce a stable positive compute-savings signal after AlpenCat overhead.

### H5 — Experience reduces future adaptation cost

Previously observed workload/resource regimes can be reused conservatively so repeated regimes require less revalidation work.

**Pass condition:** repeated regimes reduce adaptation cost without degrading routing quality.

H5 is secondary. It must not block H1-H4.

## 7. Track 0 — Freeze the exploratory phase

Status: **substantially complete**.

The existing mechanism/economics/native-signal research is retained as evidence. Do not restart sensor exploration unless H1-H4 expose a concrete missing capability.

Deferred:

- physical Intel HFI campaign;
- AMD vendor campaign;
- Android/ARM implementation;
- GPU-specific optimization campaign;
- fleet control plane;
- ML/RL;
- broad OS portability.

These are backlog, not active execution.

## 8. Track 1 — Adaptive Compute SDK minimum closed loop

Priority: **P0**

### Required result

Produce the smallest real closed loop:

```text
resource state
    -> ResourceEpoch
    -> stale boundary/profile
    -> bounded revalidation
    -> publish
    -> choose
    -> execute
    -> record decision/outcome
```

### Scope

Keep v1 deliberately narrow:

- Linux first;
- CPU serial vs CPU parallel first;
- existing `Runtime`, `PublishedBoundary`, `ResourceEpoch`, and `FastRoute` reused;
- no continuous telemetry;
- no predictor;
- no global scheduler;
- no ML;
- no mandatory daemon.

### SDK surface

The existing execution API is the starting point. The convergence question is whether a minimal `choose` / `execute` abstraction is sufficient for equivalent routes without hard-coding future policy into the public API.

Do not generalize the API before a real workload requires it.

### M1 exit gate

A real program must demonstrate:

1. equivalent serial and parallel routes;
2. output equivalence;
3. initial calibrated decision;
4. controlled resource transition;
5. stale state;
6. bounded/local revalidation;
7. updated publication;
8. changed real execution route when justified;
9. measurable result.

## 9. Track 2 — Real workload validation

Priority: **P0**

Start with **three workload families only**:

- W1 compute-bound;
- W2 memory-sensitive;
- W3 mixed compute/memory.

Selection rule:

> A workload is eligible only if it naturally has at least two semantically equivalent execution paths with a measurable crossover.

Do not add workload count for breadth alone.

### Required baselines

For every workload compare:

1. Static boundary;
2. Periodic/eager adaptation baseline;
3. AlpenCat;
4. Oracle/best observed route.

A richer model-based baseline may be added only if needed to answer a concrete reviewer/product question.

### Required metrics

- wall time;
- CPU time / compute consumed where measurable;
- routing regret;
- AlpenCat control overhead;
- revalidation cost;
- route changes;
- percentage of oracle benefit recovered;
- raw measurements and environment fingerprint.

### M2 exit gate

Determine whether real workloads contain enough **recoverable execution regret** to justify continuing.

## 10. Track 3 — Cloud variability and economics

Priority: **P0**

Use one Linux cloud environment first. Do not begin with an AWS × Azure × GCP comparison.

The experiment is about resource variability, not provider ranking.

### Controlled resource envelopes

Initial variables:

- effective CPU count;
- CPU quota;
- cpuset/affinity;
- external CPU contention;
- memory pressure;
- memory-bandwidth contention;
- NUMA placement when available.

Physical RAM capacity by itself is not a primary variable unless working-set fit makes it relevant.

### Experiment shape

```text
baseline state A
    -> workload
    -> controlled transition
state B
    -> workload
    -> AlpenCat action
    -> recovery
state A
    -> workload
```

Use interleaved baselines and preserve raw data.

### Economic output

The primary product metric is:

> **compute avoided per 1,000 compute-hours, net of AlpenCat control/revalidation cost**

Secondary metrics:

- wall-time improvement;
- CPU-hours saved;
- regret avoided;
- adaptation latency;
- false/unnecessary revalidation spend.

### M3 exit gate / GO-NO-GO

Use the observed recoverable benefit as a decision gate:

- **Red:** <0.1% recoverable compute benefit -> pause/rethink target;
- **Yellow:** 0.1%-1% -> narrow to workloads where economics are material;
- **Green:** >1% across multiple workloads/regimes -> continue vendor/cross-architecture validation;
- **Exceptional:** sustained 3%-5%+ with negligible control cost -> treat AlpenCat as a serious product/OSS platform candidate.

These are research decision thresholds, not public performance claims.

## 11. Track 4 — Experience Store

Priority: **P1**

Do not implement online ML.

First model:

```text
(workload key, machine/resource class)
    -> known boundary/profile
    -> confidence
    -> sample count
    -> observed cost
    -> last validation
```

Goal:

```text
first encounter
    -> bounded learning

repeated similar regime
    -> retrieve prior profile
    -> conservative confirmation
    -> less adaptation spend
```

### M4 exit gate

Repeated regimes reduce revalidation/adaptation cost while preserving routing quality.

## 12. Deferred Track — Android / ARM

Android/ARM is the preferred second environment after the cloud thesis is established.

Reason:

- heterogeneous CPU cores;
- frequent thermal/power transitions;
- foreground/background state;
- CPU/GPU headroom;
- DVFS;
- strong energy constraints.

The architectural requirement now is only:

> Android must be able to become another Signal Adapter / Action Adapter environment without rewriting AlpenCat Core.

No Android implementation is required for M1-M3.

## 13. Deferred Track — Vendor physical validation

Order after a GO signal:

1. Intel;
2. AMD;
3. ARM/vendor platforms.

Intel campaign purpose:

```text
real hardware transition
    -> native event
    -> latency/semantics
    -> ResourceEpoch
    -> real boundary/action consequence
```

The vendor campaign should eventually be one-command, resumable, self-logging, and low-touch for the partner, but campaign engineering is deferred until M1-M3 justify it.

## 14. Today's 80% convergence target

"80%" means **research infrastructure and executable path**, not final scientific proof.

### Must complete today

1. **T0 — Freeze**
   - this convergence plan committed;
   - previous exploration treated as frozen evidence;
   - no new signal/vendor branch opened without a concrete blocker.

2. **T1 — SDK/action path**
   - identify the minimum delta from the existing `runtime-api`;
   - implement or wire the stale -> bounded revalidation -> publish -> execute loop;
   - preserve current hot-path boundary.

3. **T2 — Workload harness**
   - establish one end-to-end real workload first;
   - define the interfaces needed to add W2/W3 without redesign;
   - include output-equivalence and raw-result capture.

4. **T3 — Cloud/resource-envelope harness**
   - define controlled CPU/resource transitions;
   - define baseline/interleaving/recovery protocol;
   - make the harness batchable for GitHub/cloud execution;
   - emit machine-readable run manifests.

5. **T4 — Smoke validation**
   - smallest credible CI run proving the harness and closed loop execute;
   - do not spend on a wide matrix until the harness passes.

### Explicitly not required today

- statistically final cloud economics;
- three fully validated workloads;
- Intel outreach;
- Intel/AMD/ARM physical results;
- Android implementation;
- GPU optimization;
- Experience Store production implementation;
- fleet daemon/dashboard;
- public product claims.

### End-of-day success state

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
NEXT T1/T2 DECISION BATCH DEFINED
```

At that point the remaining ~20% is evidence accumulation and decision-making, not architecture invention.

## 15. CI / experiment discipline

Follow the existing repository/Toolkit execution rules:

- GitHub/public execution repo for ordinary research compute;
- T0 Probe before expensive matrices;
- batch predictable cases sharing setup;
- randomize/interleave measurements where time drift can confound results;
- preserve raw measurements;
- record exact commit, environment fingerprint, cases, skips, and result;
- classify deterministic failures before paying another CI startup;
- stop escalation once the decision is stable.

Do not use CI width as a substitute for experimental design.

## 16. Stop conditions

Stop the active convergence track and reassess if any of the following is established:

1. real workloads do not exhibit material recoverable route regret;
2. adaptation cost consumes most of the available benefit;
3. equivalent-route integration requires application changes too invasive for an SDK;
4. useful resource changes cannot be detected early/cheaply enough for action;
5. a materially simpler static or periodic policy captures essentially all available benefit.

A negative result is a valid research outcome.

## 17. Decision after M3

Only after M1-M3:

```text
weak economics
    -> narrow or stop

material economics
    -> Experience Store
    -> Intel physical validation
    -> AMD
    -> Android / ARM
    -> CPU/GPU expansion
    -> Shadow Mode / Fleet layer
```

The project should earn each increase in scope with evidence.

## 18. Current next action

Start Track 1 from the existing production API.

Do **not** redesign the whole SDK first.

The immediate engineering question is:

> What is the smallest change that turns the existing `Runtime` from "route using a published boundary" into a complete "invalidate -> locally revalidate -> publish -> take action" loop on one real workload while preserving the current normal-path cost?
