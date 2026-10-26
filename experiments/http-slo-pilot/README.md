# Actual HTTP Queue SLO Pilot (research, not release)

This is an **independent reference HTTP application** built on the OSS `tiny_http` HTTP stack, not an existing production service and not an HTTP simulator. HTTP clients send requests over localhost TCP to a process that runs in an **isolated systemd transient service with true Cgroup v2 CPU Quota** and a persistent thread pool.

## Predeclared objective

Identify whether a small **preselected profile**, without expensive search in the request path, gives a repeatable p95/CPU benefit within a controlled throughput budget, over legitimate fixed worker counts. The profile is only a hypothesis:
- 2 CPU-equivalent quota → 4 active workers.
- 0.5 CPU-equivalent quota → 2 active workers.
- Restore 2 CPU quota → 4 active workers.

The reference application has eight long-lived worker threads, but only the first N call `tiny_http::Server::recv_timeout` in each phase. The control channel changes N without repeatedly constructing or destroying pools. The fixed strategies use the same eight long-lived threads and vary only active N, so they bear equivalent pool construction cost. This is a **policy-specified concurrency experiment**, not a new default AlpenCat API.

Six baselines/controls: `fixed1`, `fixed2`, `fixed4`, `fixed8`, `direct_quota`, `alpencat_quota`. Policy comparison: `direct_guard` and `alpencat_guard`. Direct and AlpenCat paths each receive the same external quota-change event; `direct_guard` and `alpencat_guard` implement **identical** concurrency rules, differing only in direct `snapshot()` versus `ResourceState::invalidate()+refresh()`. No source-code changes to the runtime or release.

## Real request and quota measurements

- Python controller starts one systemd service per arm/round with `CPUQuota=200%` and changes it to `50%`, then restores `200%`. Every phase verifies the dedicated `cpu.max` and active service cgroup membership. At least one throttled CFS period must occur in the middle phase.
- Each phase sends fixed **real HTTP GETs** to `/work/{id}/{iterations}` with 16 concurrent closed-loop clients. Each request performs deterministic CPU arithmetic before returning a 16-hex-byte digest. Results must be identical across arms for every round and phase.
- Scenario `short`: 96 requests × 1,800,000 CPU iterations per request, and example absolute p95 target **250 ms**. Scenario `long`: 144 requests × 2,000,000 CPU iterations, target **700 ms**. Targets are research assumptions, not user traffic SLOs.
- Python client measures **end-to-end** request p95 and p99 from before opening TCP to after reading the entire HTTP body; not queue-enqueue/synthetic durations. Concurrent clients are closed-loop, so this is **not open-loop fixed-arrival benchmarking**.
- Rust server records process-wide CPU time using `CLOCK_PROCESS_CPUTIME_ID`, encompassing work and workers. Python client records total batch Wall Time. The controller records cgroup `cpu.stat` usage, `nr_throttled`, and `throttled_usec` deltas, as well as the observation/decision cost. All measurements and checksums are retained in raw JSON and CSV artifacts.
- One warmup per mode, then **four randomized paired rounds** per scenario, on Ubuntu 22.04 and 24.04.

## GO / NO-GO (before reading the output)

A meaningful tradeoff for `alpencat_guard` relative to **fixed8** requires all of:
1. Relative summed three-phase p95 reduction **above 5% in at least 3/4 paired rounds** per scenario per Ubuntu runner;
2. Median server CPU time regression **no worse than 2%** relative to fixed8;
3. Median external HTTP batch Wall Time regression **no worse than 5%** relative to fixed8;
4. Same-policy `direct_guard` must exhibit comparable results (relative median HTTP p95 and process CPU within **±5%**), because the core library cannot claim policy-independent compute speedups.

Absolute p95 SLO attainment is reported for every phase; it does **not** replace paired comparisons. Positive results only support the **specific reference HTTP service and tested runner conditions**, not generic latency guarantees or direct AlpenCat Core acceleration. If no genuine tradeoff appears, report NO-GO. A negative economics outcome does not fail the *engineering integrity* workflow.

## Boundaries

The profile rule is intentionally simple, based on previous evidence; no always-on worker, active trial-and-error search, global system quota modification or production default policy. The results apply to a real HTTP reference app rather than a deployed third-party workload. Do not merge into main or publish a release as a result of the benchmark alone.

Run: `python3 experiments/http-slo-pilot/run_http_slo.py --binary experiments/http-slo-pilot/target/release/alpencat-http-slo-pilot` on a permitted Linux Cgroup v2 VM with systemd + sudo.
