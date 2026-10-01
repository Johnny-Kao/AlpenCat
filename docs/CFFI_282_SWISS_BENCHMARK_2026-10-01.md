# CFFI #282 × Swiss Knife Benchmark — 2026-10-01

## Scope

This record compares the original CFFI callable-cdata path, PR #282, and two Swiss Knife integration strategies on public GitHub-hosted Linux runners.

Pinned CFFI sources:

```text
Legacy base:
a077bdd48919e0b03a1d2db1dd9f60407e80eb1c

PR #282:
d201013d62d85c6a7a28c7ab7b431f7c553d451a
```

Python:

```text
CPython 3.14.7
```

The benchmark C library and all scenario variants were built and measured on the same runner inside each benchmark job.

## Evidence lanes

Two separate integration lanes were tested.

### Lane A — per-call dynamic Rust bridge

GitHub run:

```text
36837291417
workflow: CFFI Swiss Knife Benchmark
conclusion: SUCCESS
```

The Rust bridge is consulted on every logical CFFI call.

Scenarios:

```text
S1 Original legacy
S2 PR #282 native fast + legacy
S3 Swiss per-call replace
S4 Swiss per-call shadow verification
S5 Swiss per-call primary + backup fallback
S6 Swiss per-call bypass to legacy
```

Each scenario was run forward and reverse on one runner. The evidence bundle contains correctness output, per-round benchmark JSONL, process/resource telemetry, CPU/memory/cgroup snapshots, and Swiss route counters.

### Lane B — cached signature-level Swiss gate

GitHub run:

```text
36838256568
workflow: Runtime Framework Benchmark Lab
conclusion: SUCCESS
```

The Rust gate is consulted when CFFI builds signature metadata. The result is cached in `cif_description_t`. The steady-state call path reads only the cached route byte.

The run used:

```text
matrix rounds: 3
internal timing rounds per scenario: 5
loops per timing round: 500,000
```

Runner snapshot:

```text
architecture: X64
visible CPUs: 4
memory total: ~16.8 GB
memory available pre-run: ~15.4 GB
Vulkan: Mesa llvmpipe CPU device
```

The CFFI scalar benchmark is CPU call-path focused; the Vulkan record is environment evidence only and is not a physical-GPU performance claim.

## Baseline validation

A separate same-runner AB/BA baseline run also passed:

```text
run 36836387186
```

Representative PR #282 improvements over legacy in that run:

| case | Legacy ns | PR #282 ns | improvement |
|---|---:|---:|---:|
| ret0 | 226.90 | 202.90 | 10.58% |
| add1 | 277.75 | 225.98 | 18.64% |
| add2 | 304.66 | 243.27 | 20.15% |
| add4 | 368.88 | 292.33 | 20.75% |
| add2d | 325.46 | 262.94 | 19.21% |
| deref_ptr | 275.40 | 233.71 | 15.14% |

This confirms that the public-runner harness reproduces the expected direction of the #282 improvement.

## Lane A result — per-call dynamic routing

Paired medians:

| case | Legacy | PR #282 | Swiss replace | Shadow | Backup | Bypass |
|---|---:|---:|---:|---:|---:|---:|
| ret0 | 175.37 | 156.31 | 163.93 | 161.26 | 157.90 | 157.74 |
| add1 | 215.07 | 175.81 | 182.09 | 178.49 | 174.95 | 186.43 |
| add2 | 238.81 | 196.38 | 198.77 | 200.01 | 196.14 | 211.32 |
| add4 | 287.45 | 222.74 | 230.58 | 226.10 | 228.83 | 265.34 |
| add2d | 241.81 | 204.16 | 199.37 | 210.26 | 203.66 | 216.40 |
| deref_ptr | 208.08 | 179.65 | 179.87 | 183.70 | 180.87 | 178.06 |

Relative to PR #282:

| case | Replace | Shadow | Backup | Bypass |
|---|---:|---:|---:|---:|
| ret0 | -4.87% | -3.17% | -1.02% | -0.91% |
| add1 | -3.57% | -1.52% | +0.49% | -6.04% |
| add2 | -1.22% | -1.85% | +0.12% | -7.61% |
| add4 | -3.52% | -1.51% | -2.74% | -19.13% |
| add2d | +2.34% | -2.99% | +0.24% | -5.99% |
| deref_ptr | -0.12% | -2.26% | -0.68% | +0.89% |

Important counters:

```text
S3 replace:
route calls       66,600,000
fast routes       55,500,000
legacy routes     11,100,000

S4 shadow:
shadow checks     66,600,000
shadow mismatches 0

S5 backup:
backup fallbacks  572,164
```

Interpretation:

- Per-call Rust routing is functionally viable.
- Shadow verification produced zero routing mismatches in the tested surface.
- Backup fallback was exercised materially, not just theoretically.
- The per-call bridge usually costs about 1–5% versus PR #282 on fast scalar cases.
- Therefore consulting the resource-control layer on every nanosecond-scale FFI call is too fine-grained for this workload.

## Lane B result — cached signature-level routing

Final corrected cached-route run:

| case | Legacy | PR #282 | Swiss cached replace | Swiss shadow | Swiss backup | Swiss bypass |
|---|---:|---:|---:|---:|---:|---:|
| ret0 | 230.88 | 208.41 | 198.87 | 195.52 | 199.18 | 207.79 |
| add1 | 293.62 | 235.62 | 243.94 | 247.70 | 245.37 | 297.71 |
| add2 | 323.74 | 261.39 | 264.92 | 268.22 | 264.47 | 327.01 |
| add4 | 398.45 | 292.73 | 306.51 | 305.31 | 302.41 | 395.77 |
| add2d | 327.94 | 259.32 | 259.79 | 271.61 | 262.27 | 338.99 |
| deref_ptr | 274.92 | 245.79 | 238.07 | 243.85 | 240.42 | 275.76 |
| add1 subclass | 304.43 | 278.26 | 257.34 | 260.10 | 262.06 | 307.58 |
| mixed | 242.96 | 180.79 | 190.65 | 190.42 | 192.92 | 243.51 |

Swiss cached replace relative to PR #282:

```text
ret0           +4.57%
add1           -3.53%
add2           -1.35%
add4           -4.71%
add2d          -0.18%
deref_ptr      +3.14%
add1 subclass  +7.52%
mixed          -5.45%
```

The sign indicates faster (+) or slower (-) than PR #282.

Interpretation:

1. Moving the routing decision out of the per-call hot path removes the large artificial overhead.
2. Cached Swiss routing is broadly in the same performance class as #282.
3. It is not uniformly faster than #282; some scalar fast cases remain 1–5% slower.
4. Some fallback/general cases are faster in this run, but those gains should not be generalized from one public-runner sample.
5. S6 is near original legacy for most cases, which supports the claim that a cached junction can be present with small steady-state overhead.
6. The correct integration granularity is therefore **signature/task-level planning with cached execution decisions**, not per-call resource arbitration for sub-microsecond FFI calls.

## Correctness / stability

The cached lane passed:

- exact-source preparation;
- four isolated CFFI environments;
- six-scenario correctness smoke;
- all three paired matrix rounds;
- aggregation;
- pre/post resource capture;
- artifact upload.

The dynamic lane passed:

- Rust bridge build;
- isolated Legacy / PR #282 / Swiss environments;
- six scenarios forward and reverse;
- correctness logs;
- shadow verification;
- backup failure injection;
- complete evidence bundle upload.

No duplicate `ffi_call()` behavior is introduced by the tested fallback designs.

## Architectural conclusion

The benchmark does **not** support the claim that the Swiss Knife should replace a mature nanosecond-scale CFFI fast path with a fresh resource decision on every call.

It **does** support a stronger and more reusable architecture:

```text
observe / classify at task or signature boundary
-> Swiss Knife chooses execution policy
-> cache the decision
-> hot path executes without control-plane re-entry
-> re-evaluate only when workload/resource state materially changes
```

For very small calls, #282-style local specialization remains the right execution primitive.

The Swiss Knife adds value one layer above it:

- selecting/caching execution policy;
- choosing fast vs legacy implementations;
- shadow validation during migration;
- safe fallback;
- future adaptation when resource state or workload scale justifies re-planning.

This is consistent with the broader resource-control design: the control plane should not become the dataplane bottleneck.

## Current project implication

The experiment is strong enough to justify keeping the CFFI adapter as a real integration case for the runtime project.

It is not yet evidence that the runtime should be packaged as a standalone production project solely on CFFI performance.

The next evidence needed for packaging is broader:

- at least one workload where task size is large enough for CPU/GPU/resource decisions to matter;
- cached cost-model decisions across changing resource pressure;
- repeated integration in a second host project;
- stable public API boundary for adapters;
- H-series production-hardening backlog closure.

## Durable references

```text
baseline AB/BA:
36836387186

dynamic per-call bridge:
36837291417

cached signature-level gate:
36838256568
```
