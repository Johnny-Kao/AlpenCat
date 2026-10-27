# AlpenCat Convergence — Gate 1: Is there recoverable execution-route regret?

**STOP the automatic-policy search until this gate is measured.** This experiment is rooted in the original [Convergence Research Plan](../../docs/CONVERGENCE_RESEARCH_PLAN_2026-10-06.md), especially H1–H4 and the 1% cross-workload CPU-economic GO threshold. Gate 1 precedes Gate 2's adaptive closed-loop and Gate 3's SDK packaging.

## Do not confuse these criteria

1. **Experiment-integrity CI PASS**: source compiled, deterministic Serial/Cpu outputs matched, actual Linux Cgroup v2 quota was modified and read back, correct backend was reported, and raw evidence was preserved.
2. **Gate 1 Scientific GO**: measured evidence for *genuine* alternative route crossovers and enough **gross** recoverable process-CPU regret on predefined workloads. The per-regime oracle assumes perfect information and **charges zero decision overhead**, so it is an optimistic upper bound.
3. **Gate 2 Net GO** (only after Gate 1): actual AlpenCat `ResourceEpoch → localized revalidation → publish → auto select → execute` must recover >1% **net** compute cost against reasonable static/periodic alternatives, with measured control cost and bounded regressions. Gate 1 alone does not establish this.
4. **Product Beta GO**: independently consumable API, truthful platform semantics, install/packaging, clean formatting, tests and release process; not established by any research green CI.

## Fixed experimental protocol (before first run)

Two **preselected**, semantically equivalent workloads exercise the **existing** `runtime-api::Runtime::submit_map` Serial and Cpu paths. This does NOT benchmark a separate hand-written dispatcher:

- W1 `compute`: 512-round dependent integer transform, with `n = [256, 2_048, 16_384]`.
- W2 `memory`: 4 irregular reads in a preallocated 1,048,576-u64 input, with `n = [8_192, 65_536, 524_288]`.
- Runtime uses **`ExecutionBudget::new(2)`** and the actual Rayon CPU adapter. If the adapter falls back to Serial under a quota, the experiment labels the *actual* backend as Serial, not as measured parallel compute.
- One long-lived process per repetition experiences **real** `cpu.max` transitions **2 CPU → 1.5 CPU → 0.5 CPU → 2 CPU**, via a dedicated transient systemd Cgroup v2 service. The controller verifies the process is inside its owned Cgroup and reads `cpu.max` after every transition. `cpu.stat.nr_throttled` must increase at least once in the 0.5-CPU phase. No host-level quota is modified.
- Within every phase: both routes are warmed up, followed by 7 alternating AB/BA paired samples per workload/size; outputs checked for equivalence, actual backend recorded. Wall time uses Rust `Instant`; process-wide CPU time uses Linux `CLOCK_PROCESS_CPUTIME_ID`. Record each pair's **median**, not the lucky minimum.
- Run **5 independent repetitions** on each of **Ubuntu 22.04 and 24.04**. Do not retune workloads or acceptance thresholds after inspecting results; a measurement-infrastructure flaw may be fixed and reported separately.

## Gate 1 oracle and STOP/GO criteria

For each workload family and size, compare the Serial and Cpu API paths over **all four resource phases**:

- **Static best**: choose the same requested API mode across phases, picking the better of the two in hindsight for that workload size. A requested Cpu that the runtime correctly falls back to Serial is counted as the actual executed route. This generous per-size static baseline is stronger than a single fixed global boundary.
- **Perfect oracle**: choose the lower process-CPU cost of either executed route *separately for every phase*, assuming free instantaneous knowledge. Its benefit over best-static is **gross maximum recoverable CPU cost**, excluding all revalidation, observation and routing overhead. It is NOT a deployable strategy and susceptible to measurement noise.
- **Robust CPU route crossover** requires a serial winner and a genuine CPU-parallel winner **each by >5% within the same workload/size on distinct phases**, with **≥4/5 independent repetitions** supporting each. A Cpu API call falling back to Serial is not a second valid execution implementation.
- **Gross CPU benefit**: the optimistic per-size best-static vs perfect oracle saves **more than 1% process CPU time** in **≥4/5 independent repetitions**, aggregating the three predefined sizes within each family, using each size once per phase.
- **Gate 1 full GO**: robust *CPU-time* route crossover **and** >1% optimistic gross CPU savings in **both preselected families**, independently on both Ubuntu environments.
- If this fails, record **NO-GO for the original generic compute-savings thesis on this tested matrix**. It does NOT prove every possible workload has zero adaptive benefit; it **ends this bounded campaign**, with no automatic new exploration branches.
- Wall-time route winners and gross Wall regret are reported separately, but **Wall-Time-only benefits do not satisfy the original 'compute saved' Gate 1**, because increased CPU time cannot be relabeled as compute-hour savings.

## Bounds and interpretation

- This is a micro-application using real `runtime-api` data paths, not third-party production software. The workload samples have no GPU route.
- Five repeated measurements on two ephemeral GitHub runners offer directional evidence, not confidence about fleets.
- 2→1.5→0.5→2 is a **real shared CPU-time quota change**, not CPU affinity, isolated CPU pinning, or actual memory-pressure changes. W2 is memory-sensitive by access pattern, *not* an experimentally imposed memory-bandwidth contention regime.
- A quiet cgroup with only one runnable thread may not throttle in every sample; integrity requires actual throttling in the restricted phase across the batch.
- No `main`, v0.4/v0.5 tag, or external production app is modified. An unsupported CI environment must be reported as an environment blocker, not interpreted as a scientific result.

Primary workflow: `.github/workflows/convergence-gate1.yml`, raw run artifacts `convergence-gate1-ubuntu-*.json/csv`.

## Exact release decision

GO → only then implement **one** end-to-end lazy localized revalidation test on the identified, independently repeatable route-crossing workload, measuring net recovery.  
NO-GO → stop adapting compute execution in this campaign; separately finish the Resource Intelligence SDK by its own correctness/ergonomics/independent-consumer release gates.  
BLOCKED → repair test infrastructure once without changing research acceptance; avoid interpreting absent results as failure or success.

