# AlpenCat Adaptive Execution Guard — v0.1 Alpha contract

**Product direction: GO to develop an opt-in Adaptive Execution Guard for Rust services.** This is not a claim of universal CPU/Wall improvements or a Beta release. The existing `alpencat-resource` resource-observation SDK stays independently useful and read-only.

## What an adopter can actually do

Import `alpencat-guard` (dependency: only `alpencat-resource`, no third-party/runtime dependency), create a `Guard` with an **application-owned** profile, and use its cloneable `AdmissionGate` to bound **concurrent request or task admission**, not total Linux workers or Rayon global threads.

- **Observe (safe default)**: interpret fresh capacity evidence but never recommend or mutate admissions.
- **Recommend**: return a proposed profile and reason; the app implements/accepts its own strategy.
- **Enforce (explicit opt-in)**: update admissions to a configured normal/constrained limit after a caller-owned event. Existing work is not revoked; new permits respect the smaller limit.
- **Unknown/Expired/Stale/membership-race/partial ancestor visibility**: use application-configured fail-safe admission ceiling; never infer quota from a partial snapshot. `Guard.evaluate(&ResourceState)` is **cache only**, performs no hidden OS observation.
- **Cooldown**: prevents oscillatory **upward** changes; downscaling and fail-safe adjustments are immediate. It does not create background events.
- **Feedback + rollback**: the caller may submit `report_comparison(baseline, candidate)`, with p95, CPU Time and Wall Time **measured within the same resource regime**. If any configured regression threshold is exceeded, Guard restores the previous limit **once**, then suppresses reapplying the losing profile until a different resource regime is observed. No fake measurements or automatic per-request sampling.
- **AdmissionGate**: RAII `Permit`, nonblocking `try_acquire`, bounded `acquire_timeout`. A permit is released even during panic unwinding; a limit reduction never interrupts in-flight tasks or frees the permit prematurely. The application owns the actual worker pool and overload behavior.

### Minimal API sketch

```rust
use alpencat_guard::{Guard, GuardConfig, Mode};
use alpencat_resource::ResourceState;
use std::time::Duration;

let config = GuardConfig {
    mode: Mode::Enforce,
    normal_limit: 4,
    constrained_limit: 2,
    fail_safe_limit: 2,
    upward_cooldown: Duration::from_secs(5),
    ..GuardConfig::default()
};
let guard = Guard::new(config).expect("valid application guard config");
let resources = ResourceState::new();
resources.refresh().expect("consistent resource observation"); // explicit blocking OS read
let decision = guard.evaluate(&resources);
let gate = guard.gate();
if let Some(permit) = gate.acquire_timeout(Duration::from_millis(100)) {
    // application runs its own real task, then permit drops
    drop(permit);
}
```

**Note:** the snippet is illustrative. Real operational code must handle refresh/configuration errors, queueing, cancellation, and what stale readings mean for its service.

## Original evidence, without overstating it

The previous independent HTTP reference study under actual 2→0.5→2 Cgroup v2 quota showed conditional p95 reductions in 3/4 preregistered workload/runner cases against fixed8, with one fail. A strong static fixed2 also performed competitively under restriction, and a direct-OS same-policy adapter matched AlpenCat. This motivates an **application-specified guard**, not a universal speedup claim. See the [separate restricted HTTP research results](https://github.com/Johnny-Kao/AlpenCat/blob/research/http-slo-real-cgroup/experiments/http-slo-pilot/RESTRICTED_HOLDOUT_RESULTS.md).

## Product alpha acceptance (must report separately from CI)

| Gate | Required evidence |
|---|---|
| Functional | Modes do what their names say; no default enforcement, stale fallback, cooldown, rollback, safe concurrency limits |
| Integration | Real standalone external Cargo HTTP service admits real HTTP work with eight persistent threads |
| Platform | Guard unit tests and external consumer compile on two Ubuntu runner variants, macOS 15, Windows 2025 |
| Live quota | Dedicated disposable service, actual 2→0.5→2 Cgroup v2, quota readback, real throttling, Observe/Recommend don't silently enforce; Enforce+Direct comparison record |
| Correctness | Equal HTTP output across all strategies; no failed/overloaded requests at tested traffic; test on fixed8 baseline and efficient direct OS equivalent policy |
| Economics | *Separately* report full-phase HTTP p95, CPU, Wall, throughput and instrument cost vs fixed8/direct; do **not** equate functional PASS with performance GO |
| Distribution | An out-of-workspace consumer can depend on SHA-pinned repo and run without internal test paths. Versioned Alpha tag is separate, explicitly approved release step |

Not yet implemented: native Cgroup event subscription, automated SLO feedback sampler, real production framework middleware, admission fairness guarantees, async nonblocking permits, persisted policy learning, safe control-plane authentication, or host-wide auto-thread management. These are **not** promised in this alpha.

## Concurrency, security, and performance boundaries

- `AdmissionGate` is a **synchronous blocking** primitive for worker threads; never call `acquire_timeout` directly on a Tokio async executor core thread. Prefer `try_acquire` or implement async adapter at the framework boundary.
- `ResourceState::Fresh` is relative to caller-observed invalidations and a caller-selected time bound; Cgroup mutations do not automatically notify this crate. On external changes the application must explicitly `invalidate`, `refresh`, `evaluate`.
- `limits_complete` is a visible-scope flag. Hidden ancestors may still limit resources; the source adapter does not guarantee a complete host-wide entitlement.
- Native fractional CPU quota does not intrinsically determine optimal worker count. The 4/2 policy is a **caller-selected candidate**. No guarantee of equal benefit across services or runners.
- Feedback compares **same-regime** outcomes; comparing pre/post-quota samples would confound effects. The app must provide measurement provenance and reliable SLO samples.
- For requests blocked longer than timeout, the app chooses a rejection or retry. Do not advertise successful queuing or fairness until integrated and validated.

## Research-to-product stop rules

The old universal compute-savings thesis failed the preset >1% CPU gross opportunity bar in its two-workload convergence matrix, and exploration-heavy policies regressed. Do not reopen the unbounded multi-worker tuning cycle.

For the new Guard product, engineering acceptance means it works predictably as an opt-in control primitive. An actual **economic product claim** needs a heldout deployment-style workload, a useful p95 SLO, per-application acceptance criteria, a strong tuned-static baseline, the same-policy direct OS comparison, and demonstrated value beyond merely reading OS metrics. False economic claims are not acceptable even when all tests pass.

Candidate branch: `feature/adaptive-execution-guard-alpha`. No automatic upstream PR/merge, tag or crates.io publish.
