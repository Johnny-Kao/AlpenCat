# Linux x86 X1/X2 Evidence Audit — 2026-10-09

## Sources
- Batch: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37873866384
- Independent frozen Generic holdout: https://github.com/Johnny-Kao/AlpenCat/actions/runs/37706179018

## Confirmed
- 20/20 probe jobs and aggregate passed; 16 eligible / 4 excluded.
- All four ubuntu-slim x86 jobs were excluded because /proc/pressure/cpu was absent. This does NOT demonstrate that all Linux x86 targets expose PSI.
- Six phases ran on all 16 eligible Ubuntu 22.04/24.04 jobs.
- CPU PSI some total delta / elapsed time, across 16 jobs (median approximate fraction):
  - baseline 0.001; cpu-pressure 0.655; memory-light 0.000; memory-heavy 0.000; combined 0.639; recovery 0.000.
- cgroup CPU PSI moves similarly, and host PSI may share causes with cgroup PSI; do not count them as independent evidence.
- Memory PSI was zero in the memory-light/heavy/combined synthetic phases. These loads are insufficient to validate true reclaim or memory stalls.
- PSI sampled per entire phase (not timestamped before/during an independent route opportunity). The observed change is **not** proof of lead time or useful route-adaptation decisions.

## X3 independent evidence / mismatch
The frozen Generic run includes workload-level static/oracle opportunity labels, but it ran on **different runner allocations**, without contemporaneous PSI samples. Therefore those labels MUST NOT be joined to X1/X2 samples by OS, phase, kernel or apparent CPU model: doing so would invent paired observations.

X3 (signal opportunity discrimination, observation cost, incremental net utility vs F0) remains NOT VALIDATED. The correct next experiment measures native PSI and actual static/oracle route economics together, on each same machine allocation, same time window, with independent holdout splits. No Generic policy retuning.

## Decision
X1 availability: PASS with portability caveat.
X2 CPU contention sensitivity: PASS for synthetic contention, not memory/IO pressure.
X3 native-signal value beyond frozen Generic shell: INCONCLUSIVE / NOT TESTED.
Vendor extension: BLOCKED pending matched native economic evidence.
No modifications to Foundation, Generic F0/F1/F2 or hot path.
