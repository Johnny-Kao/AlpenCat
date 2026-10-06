# AlpenCat Research Handoff — 2026-10-07

> Branch: `research/convergence-adaptive-compute-sdk`  
> Current branch head when this handoff was written: `ebdbcb392115c69d62ec23081945120b83b6590d`  
> Research phase: convergence / adaptive-compute economics  
> Status: E0 complete; W1 control established; W2 economic opportunity validated; W3 harness added and awaiting the next decision batch.

## 1. Executive state

The important transition is methodological:

```text
old question:
  how many local calibration points should AlpenCat spend?

current question:
  when is adaptation economically justified,
  how confident are we that the old boundary is materially wrong,
  how should switching avoid oscillation,
  and how should a candidate be verified or rolled back?
```

Do **not** resume by blindly tuning `max_points`.

W2 proved that a real oracle gap can exist under contention and that local bounded revalidation can miss economically relevant opportunity. Subsequent W2 work further showed that the correct control variable is not a universal probe count or delay, but the relationship between expected remaining demand, uncertain break-even, evidence quality, candidate direction, and verification cost.

## 2. E0 — COMPLETE

Canonical checkpoint:

- `docs/E0_EVIDENCE_INFRASTRUCTURE_2026-10-06.md`
- authoritative E0 run: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37456566642
- E0 checkpoint commit: `925f10e` lineage; authoritative documented branch head at E0 completion: `af994384ffbf2e352139874f8f9238197a03f6f2`

E0 established:

- shared JSONL evidence schema;
- one evidence source for Static / Periodic / AlpenCat / Oracle reconstruction;
- five regimes: baseline-full / half / one / contention / recovery;
- bootstrap vs adaptation semantics;
- inherited prior boundary + stale invalidation + bounded local revalidation;
- raw route timings + Auto backend observation + revalidation cost;
- economics reconstruction and artifact validation.

E0 result: **PASS**.

W1 `compute-mix-128` is retained as the low-opportunity compute-bound control.

## 3. W2 integration — COMPLETE

Workload:

`memory-column-transform-u64`

Initial W2 integration checkpoint:

- commit: `a2a1b40d75db6643a108ec0f017db2b8527edc41`
- commit message: `style: apply W2 interleave rustfmt`
- core evidence workload: `crates/runtime-api/examples/evidence_memory_workload.rs`
- five-regime runner: `experiments/convergence/run_w2_evidence_matrix.sh`
- budget-sensitivity runner: `experiments/convergence/run_w2_budget_sensitivity.sh`

The budget-sensitivity sweep explicitly tested `max_points = 3/4/5/6`.

Observed local boundaries included `65536` and `16384`, showing that the W2 crossover is materially more dynamic than W1.

## 4. W2 contention exposed a real oracle gap

The key initial contention observation was approximately:

| Policy | Runtime |
|---|---:|
| Static | 914.0 ms |
| AlpenCat | 919.5 ms |
| Oracle | 865.2 ms |

This is roughly a **5% real Oracle gap** relative to the measured static/AlpenCat result.

Interpretation:

- there is meaningful recoverable regret;
- AlpenCat's current local revalidation did not reliably capture it;
- increasing `max_points` is not automatically the correct remedy.

The initial hypotheses were:

### A1 — local search is too narrow
A larger bounded neighborhood may recover the useful crossover.

### A2 — crossover evidence is noisy / unstable
Near-boundary route preference may flip; more points alone may simply spend more measurement budget on unstable evidence.

### A3 — one scalar boundary is insufficient
A memory-sensitive route can potentially exhibit non-monotonic shape such as:

```text
Serial -> CPU -> Serial
```

rather than a single monotonic:

```text
Serial -> CPU
```

Later W2 analysis added:

### A4 — lazy delay is the dominant loss
Revalidation may be cheap enough that delayed action, not search width, dominates regret.

### A5 — revalidation cost dominates
A real stale-route loss may exist but not live long enough to amortize adaptation.

## 5. Adaptation economics is now the primary research line

Canonical design note:

- `docs/ADAPTATION_ECONOMICS_2026-10-06.md`

Core gate:

```text
P(boundary materially wrong)
× expected relevant calls before next regime change
× expected regret per wrong call
>
revalidation cost
```

Measured form:

```text
C_revalidate = bounded revalidation cost
R_cycle      = L_static - L_oracle
break_even_calls = C_revalidate / regret_per_call
```

The intended control layers are:

```text
L0 FastRoute
L1 Invalidate
L2 Economic Gate
L3 Bounded Revalidation
L4 Candidate Publication
L5 Post-action Verification
   -> keep / rollback / confidence update
```

Only the early layers are mature enough to be treated as implemented runtime behavior. Candidate verification remains a research target.

## 6. Confidence / hysteresis / verification

Do not publish a new boundary merely because another route wins once.

Offline candidate rules to continue testing:

```text
switch only if:
  estimated gain > switching margin
  AND evidence consistency >= threshold
  AND expected break-even occurs before likely regime expiry
```

Then:

```text
candidate action
-> observe a small number of real calls
-> benefit persists?
     yes -> commit / confidence++
     no  -> rollback / confidence--
```

This is preferable to a universal fixed delay or universal safety multiplier if holdout evidence continues to support it.

## 7. W2 holdout result — IMPORTANT UPDATE

Canonical checkpoint:

- `docs/W2_HOLDOUT_ECONOMICS_2026-10-06.md`
- authoritative confirmatory run: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37480459630

The later W2 work materially strengthened the initial conclusion.

At a 1,000-call horizon, the first independent holdout exposed near-break-even transfer failures: candidate direction could be correct while commitment timing was too aggressive.

At a 10,000-call horizon, independent holdout evidence was strongly positive. In the confirmatory batch, the broad gate produced:

- 8 actions;
- 8 positive;
- 0 negative;
- mean saving: **+8.333%**;
- worst case: **+4.512%**;
- holdout regret captured: **87.7%**.

Therefore:

- W2 recoverable-regret opportunity: **PASS**
- W2 cross-runner generalization: **PASS**
- long-horizon independent holdout transfer: **PASS**
- near-break-even hard threshold: **REJECTED**
- universal `2x break-even` safety factor: **NOT APPROVED**
- post-action verification: **SUPPORTED AS NEXT SAFETY CANDIDATE**
- runtime-core policy implementation: **DEFER**

The current supported framing is:

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

## 8. W3 current state

The research branch has already advanced beyond the original W2 checkpoint:

- `c6f76761c69e59688b3b5754176e7f935f5552bb` — `research: add W3 mixed compute-memory workload`
- `ebdbcb392115c69d62ec23081945120b83b6590d` — `style: apply W3 rustfmt output`

Workflow:

- `.github/workflows/w3-mixed.yml`

W3 is the next gate because W1 is low-opportunity and W2 proves a real economic opportunity. Before putting confidence / hysteresis / verification state into runtime core, verify that the same economic-control pattern exists in another workload family.

## 9. Next-session decision sequence

Do this in order:

1. Inspect the W3 workflow/result state at branch head.
2. Preserve W1 as control and W2 as positive economic case.
3. Run / analyze W3 using the same shared-evidence contract.
4. Determine whether W3 shows:
   - stable recoverable regret in at least one regime;
   - positive long-horizon holdout economics;
   - conservative behavior required at short/uncertain horizons.
5. If W3 confirms the pattern, design the **smallest possible** candidate-verification abstraction offline first.
6. Only then evaluate runtime-core implementation.

Do **not**:
- restart global calibration;
- tune `max_points` without an economics hypothesis;
- add universal fixed delays;
- add a universal 2x break-even multiplier;
- assume resource loss implies Serial;
- generalize one runner / CPU model to all hardware;
- add confidence/hysteresis state to the hot path before cross-workload evidence.

## 10. Research question for the next session

The next high-value question is:

> Can W3 independently reproduce the W2 pattern that long-lived resource regimes justify bounded adaptation while short / near-break-even regimes require confidence and verification?

If yes, AlpenCat has evidence for an adaptation-policy abstraction.

If no, the W2 policy may be workload-specific and the correct abstraction boundary must be reconsidered.

## 11. Suggested next-session prompt

```text
接手 AlpenCat 研究分支 research/convergence-adaptive-compute-sdk。

先读：
1. docs/RESEARCH_HANDOFF_2026-10-07.md
2. docs/E0_EVIDENCE_INFRASTRUCTURE_2026-10-06.md
3. docs/ADAPTATION_ECONOMICS_2026-10-06.md
4. docs/W2_HOLDOUT_ECONOMICS_2026-10-06.md

当前关键结论：
- E0 PASS，shared evidence infrastructure 已完成。
- W1 compute-mix-128 是 low-opportunity control。
- W2 memory-column-transform-u64 已证明 contention 下存在真实 oracle gap；早期约 Static 914.0 ms / AlpenCat 919.5 ms / Oracle 865.2 ms。
- 不要继续盲调 max_points。问题已经转成 adaptation economics / confidence / hysteresis / verification。
- W2 后续 independent holdout 已证明：短 horizon 的 near-break-even action 可能负 transfer；长 horizon 则有稳定正经济价值。
- universal 2x break-even、fixed delay、固定 margin 都没有被批准。
- post-action verification 是下一 safety candidate，但 runtime-core implementation 仍 defer。
- 分支已加入 W3 mixed compute-memory harness，当前 head ebdbcb392115c69d62ec23081945120b83b6590d。

你的第一件事：
检查 W3 workflow / evidence 当前状态，并沿用 E0/W2 的 shared-evidence + independent-holdout methodology，判断 W3 是否复现：
1. 至少一个 regime 有 stable recoverable regret；
2. long-horizon holdout economics 为正；
3. short / uncertain horizon 需要 conservative gating。

如果 W3 不支持，不要硬做 policy。
如果 W3 支持，再设计最小 candidate verification / rollback / confidence abstraction，先 offline simulation，再考虑 runtime core。

禁止：
- 无假设地调 max_points；
- 重新做 global calibration；
- 把单一 CPU / runner 结果泛化；
- 直接把 hysteresis/confidence 塞进 hot path。
```
