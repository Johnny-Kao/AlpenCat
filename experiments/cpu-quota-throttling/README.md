# CPU Quota / Throttling: real Linux Cgroup v2 experiment

This experiment belongs to `research/resource-cpu-quota-throttling`. It is **not** a shipped AlpenCat automatic policy.

## Real system controls

A dedicated transient systemd service is created per strategy and trial, with `CPUQuota=200%`, then changed using `systemctl set-property --runtime` to `CPUQuota=50%`, then restored to `200%`. The Python controller reads the **actual** dedicated cgroup's `cpu.max` after every change and asserts the requested 2 → 0.5 → 2 CPU-quota transition happened. It also checks the worker process is inside that service's cgroup, and reads `cpu.stat` deltas (`nr_throttled`, `throttled_usec`, `usage_usec`).

The controller uses only a temporary service, resets via `systemctl stop`, and does not change host-wide CPU quotas. The worker and controller communicate over a UNIX socket. Ubuntu GitHub-hosted runners have demonstrated systemd ownership and writable `cpu.max` on a dedicated test service in [quota probe #40096517696](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40096517696). Without this capability the experiment must fail as **environment unsupported**, not silently simulate quota.

## Real workload and policies

- Bounded deterministic CPU-heavy request queue: each request is an actual arithmetic computation, and the requests are split among 1/2/4/8 real Rust worker threads.
- For each phase the worker pool is created inside the service under actual CPU quota constraints; CPU Time comes from `CLOCK_PROCESS_CPUTIME_ID`; Wall Time from Rust `Instant`.
- Requests are enqueued at phase start. The reported **p95 request completion latency** is the 95th percentile of time from that phase's common enqueue instant to each request's completion. This is intentionally a saturated batch queue, not a production end-to-end HTTP p95.
- Strategies: static workers=1, 2, 4, 8; `direct` reads Linux via `snapshot()` after each quota transition and sets workers to `ceil(quota)` (clamped 1..8); `alpencat` uses `ResourceState::invalidate()+refresh()` and **the same** worker decision rule.
- Quota policy uses `ceil(0.5)=1`, `ceil(2)=2`. This is a conservative prototype, not an optimal policy. It does not promise CPU efficiency improvement.
- One warmup per mode, then 5 randomized paired rounds across two workload intensities. All modes and rounds must calculate identical checksums. Real quota must be verified per phase; 0.5 CPU must demonstrate throttling.

## Outcome metric

Collect exact CPU Time, Wall Time, sum of phase p95 completion times, observation cost, per-phase `nr_throttled` deltas and `throttled_usec`; preserve raw CSV and JSON with each workflow run. No microbenchmark-only claim.

Predeclared *distinct AlpenCat* stable positive outcome: p95 improves by >5% versus best fixed baseline in >=4/5 paired trials, without median CPU regression worse than 2%; plus AlpenCat beats **same-policy direct-event OS read** by >2% p95 in >=4/5 paired trials, without CPU regression worse than 2%. Both workload sizes and both Ubuntu runner environments must pass to claim stability.

Failure to show economic gain is a **NO-GO**, not CI tooling failure. Failure to control a real Cgroup quota, obtain deterministic results, or find actual throttling is a CI experiment-integrity FAILURE. Missing permissions must be disclosed. Scope does not include memory pressure, cgroup namespace generality, production HTTP latency, all CPUs/architectures, or cloud bill savings.

AlpenCat's resource core remains strictly read-only and adds no background polling or default policy. Existing `main`, v0.5 tag and releases remain unchanged.
