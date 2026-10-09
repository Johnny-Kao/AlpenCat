# Linux x86 Specialization Checkpoint — 2026-10-08

> Status: FOUNDATION PASS — ready for Linux x86 research
> Authoritative Foundation Gate: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37869233762
> Begin with shared Linux x86 signal inventory; Intel/AMD tuning remains deferred.

## Scope

Start specialization only after Generic holdout closure.

The first target is a vendor-neutral Linux x86 platform layer shared by Intel and AMD.

## Hypothesis

Generic routing failed because the portable layer lacks enough state to distinguish meaningful resource transitions from harmless ones without paying excessive observation cost.

Linux exposes richer cheap state that may improve eligibility decisions before any route probe:

- PSI;
- cgroup CPU quota / cpuset;
- cgroup memory pressure;
- effective CPU count;
- NUMA/topology;
- scheduler-visible CPU changes.

The platform layer should answer:

> Is this transition likely meaningful enough to justify specialized adaptation?

It should not directly choose the backend.

## Phase X1 — signal inventory

Collect, in one run:

- `/proc/pressure/cpu`
- `/proc/pressure/memory`
- `/proc/pressure/io`
- cgroup v2 `cpu.max`
- cgroup effective cpuset
- memory.current / memory.max when available
- online CPU count
- `lscpu -J`
- NUMA node inventory
- CPU vendor/model only as metadata, not policy input

Record missing/unavailable signals explicitly.

## Phase X2 — transition response

Use the existing pressure regimes and record signal deltas:

- baseline
- CPU pressure
- memory-light
- memory-heavy
- combined
- recovery

Goal:

- determine which native signals actually move under each regime;
- estimate observation cost;
- detect whether signal changes are consistent across x86 runners.

Do not define a routing threshold yet.

## Phase X3 — economic usefulness

For each signal family, ask only:

- did the signal move before or during a measured opportunity?
- did it stay quiet in no-opportunity cases?
- is collection cost small relative to existing revalidation cost?

Use cross-run comparisons, not one-run fitting.

## Phase X4 — vendor extension

Only after X1-X3:

Intel:
- HFI availability/interface
- physical-server event path

AMD:
- CPPC/capability/topology path
- physical-server event path

## Exit criterion

The Linux x86 platform layer advances only if at least one cheap signal family provides materially better opportunity discrimination than the frozen Generic shell without requiring route probing.

Otherwise specialization must move directly to vendor/native physical-server signals.
