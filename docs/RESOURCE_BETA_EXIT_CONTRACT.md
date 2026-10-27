# AlpenCat Resource Intelligence SDK — Beta Candidate Exit Contract

**Current status: FUNCTIONAL CANDIDATE VERIFIED; NOT RELEASED.** The original general Adaptive Compute CPU-economics research passed experiment integrity but **did not** pass the original 1% economic GO threshold in its bounded [Convergence Gate 1](https://github.com/Johnny-Kao/AlpenCat/tree/research/convergence-gate1-execution-crossover). That scientific NO-GO does **not** imply this separate resource-observation product lacks value; it does prohibit claiming generic workload speedups.

## What we will actually ship

The independently consumable Rust crate `alpencat-resource` provides:
- `snapshot()` for bounded, synchronous best-effort CPU/Memory/Cgroup observations;
- typed `Reading<T>` that distinguishes Known / Unknown / Unsupported / Unavailable instead of claiming inferred truth;
- `ResourceState::new()`, `refresh()`, `cached()`, `cached_with_max_age()`, `invalidate()` with immutable `Arc<ResourceSnapshot>`;
- no implicit polling or background threads and no automatic application Worker tuning.

Applications own their scheduling, SLO and rollback policy. CPU quota capacity does NOT equal guaranteed CPU throughput. `Fresh` means not invalidated inside the **caller's own event model**, not that the OS guarantees unchanged resources. Use age bound and explicit event sources appropriately. Cgroup mount root and namespace scope must be presented honestly.

Existing `runtime-api` and earlier v0.4/v0.5 GitHub source tags remain unchanged. The new source SDK does not depend on `runtime-api`.

## Beta Candidate GO checklist (release blockers, not status decorations)

| Requirement | Exit evidence | Current |
|---|---|---|
| API contract | Public quickstart + invalidation, age, Unknown/Unavailable semantics | Documented in `docs/RESOURCE_LIFECYCLE_EXPERIMENT.md` and resource alpha quickstart; revise for beta |
| Cgroup correctness | Actual cgroup2 leaf quota read and visible mount-root absent `cpu.max`/memory.max fixture, graceful malformed mountinfo | Implemented in this candidate; [4-OS CI PASS](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40526685176) |
| Data lifecycle | No hidden OS reads in `cached()`, stale must never be Fresh, concurrent refresh publication and invalidation safe | Unit tests verified by [4-OS CI](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40526685176) |
| Real Linux integration | 0.5 CPU quota via **actual disposable systemd service** with ResourceState lifecycle checked | Real 0.5 CPU Cgroup PASS in [both Ubuntu jobs](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40526685176) |
| Cross-platform | Resource library tests on Ubuntu 22.04 / 24.04 / macOS 15 / Windows 2025 | **4/4 PASS**, [run #40526685176](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40526685176) |
| Real consumer | Independent Cargo package with no internal workspace dependency on runtime-api exercises published API | Separate Cargo consumer PASS on all four systems, [run #40526685176](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40526685176) |
| Formatting & packaging | Rustfmt check on committed source, public install/upgrade path, stable source API inventory, no accidental crates.io publish | Strict committed-source rustfmt + Clippy PASS; workspace crate `0.1.0`, `publish=false` (Git source only); final Beta version policy pending |
| Independent use | One integrator can install the immutable version/tag and exercise it without repository-specific paths | Independent out-of-workspace **SHA-pinned Git consumer PASS**, [run #40527236202](https://github.com/Johnny-Kao/AlpenCat/actions/runs/40527236202), checked SHA `3fc0e1043776b74b56221da40b7f00f08c03de7e`; final immutable Beta tag not created |
| Release identity | Tag points to the exact final tested commit SHA; docs and release notes match | Not done — release signoff decision required |

**Evidence snapshot:** Source format, live Linux Cgroup, library correctness, runtime compatibility, 4 OS and real external Git dependency all PASS. This is **engineering candidate GO, not released Beta**; formal version/tag and external adopter feedback still pending.

**Only these exit conditions decide SDK Beta release.** No general 1% compute saving is required for a read-only Resource Intelligence SDK. Conversely, even all-green CI does not make it a released product without the last two evidence gates.

## Reasonable release semantics

1. Candidate branch `release-candidate/resource-intelligence-beta` must first satisfy correctness and real-system integration.
2. Promote only the safe resource crate/API changes from the candidate. Keep old runtime routes behind their existing experimental contracts.
3. Normalize file formatting **in the repository**, not only temporarily in the CI checkout. Strict `cargo fmt -p alpencat-resource -- --check` should pass on the committed SHA.
4. Run final independent Cargo consumer from a pinned immutable ref after choosing a release identity, then verify version metadata, docs, supported-platform table and no publication misrepresentation.
5. Create a GitHub source Beta release **only after a separate explicit release signoff**, not because code compiles.

## Scientific STOP / product GO boundary

The original Convergence Gate 1 [results](https://github.com/Johnny-Kao/AlpenCat/blob/research/convergence-gate1-execution-crossover/experiments/convergence-gate1/RESULTS.md) found optimistic CPU savings ≤0.201% across the tested families on two runners (below 1%). The existing `ExecutionMode::Cpu` fell back to Serial under fractional quota, limiting the opportunity test. No claim about arbitrary workloads, vendor native hardware events, or universal speedups is supported.

**Product priority now** is finishing a reliable, independently adoptable resource-state contract, with honest unknown/cgroup scope limitations. The adaptive compute route stays experimental and does not block this release.

