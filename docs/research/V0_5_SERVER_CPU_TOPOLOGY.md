# v0.5 Linux Server CPU Cache / NUMA Research Gate

Date: 2026-10-15  
Scope: research branch only. No Runtime core changes.

## Goal

Establish whether the Linux host exposes at least two independently selectable L3 cache domains and NUMA nodes, and whether native multi-thread workloads can measure stable, reproducible effects of actionable CPU placement.

Hardware hypothesis: local vs cross-domain CPU placement may alter memory locality, cache traffic and resource contention. This is workload-dependent and must **not** be assumed beneficial.

## Tooling

- `experiments/v05/server_topology_probe.py` records per-allowed-CPU package, core ID, shared L3 groups and NUMA node information from Linux sysfs.
- Only child process CPU affinity is modified. Parent host, BIOS, global NUMA policy and cgroups are not modified.
- `experiments/v05/server_locality_native.c` runs two native pthreads reading a shared large allocation and records independent wall and process CPU time with checksum verification.
- `.github/workflows/v05-server-topology.yml` exercises syntax and toolchain on Ubuntu 22.04 and 24.04. Raw topology and observations are uploaded as GitHub Actions artifacts.

## Actual first results

[GitHub Actions run #38738319976](https://github.com/Johnny-Kao/AlpenCat/actions/runs/38738319976), code at commit `e3eec1cfb333792edc5879b52ab834e46ac36814`: **2/2 PASS**.

| Runner | Same-L3 CPU pair | Median wall | Median process CPU | Cross-L3 evidence |
|---|---|---:|---:|---|
| Ubuntu 22.04 | [0, 2] | 105,226 ns | 95,277 ns | UNAVAILABLE |
| Ubuntu 24.04 | [0, 2] | 85,796 ns | 138,372 ns | UNAVAILABLE |

No cross-L3 comparison was possible on these runner allocations. No NUMA-locality advantage was proven. Process CPU time can exceed elapsed wall time with parallel workers. These are harness smoke values only, not production performance benefits.

## Experimental limitations and next gate

1. Both jobs expose only one usable L3 grouping; cannot determine the sign or magnitude of cross-CCD / cross-tile locality effects. **Do not fit a policy** to these CI results.
2. Current read-only topology discovery does not set memory policy or ensure first-touch NUMA allocation. Cross-node memory locality is **not measured**.
3. Thread creation is inside the measured window. The current small memory footprint can be dominated by task setup. Next phase must reuse warmed worker threads, measure larger cache/DRAM working sets, and use interleaved, counterbalanced placement order.
4. Reproducibility requires at least one bare-metal or explicitly pinned physical Linux server with two actual distinct L3 domains, known NUMA topology and permission to control CPU affinity. AMD EPYC is one candidate; Intel Xeon P-core is the independent holdout, not an assumed equivalent topology.
5. Treat physical availability as a precondition: when a domain is missing, record `CROSS_L3_UNAVAILABLE`; never fabricate or extrapolate remote-domain measurements.
6. After an opportunity is reproducibly observed, quantify `Static / Periodic / Lazy` net process CPU time and wall time with real resource transitions and calibration cost. The Generic control core remains frozen.

**Decision: infrastructure PASS; scientific claim INSUFFICIENT HARDWARE COVERAGE.** This is a completed implementation gate, not a demonstrated server CPU optimization.
