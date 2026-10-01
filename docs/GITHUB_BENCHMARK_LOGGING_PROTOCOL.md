# GitHub Benchmark Logging Protocol

This protocol is for comparative CFFI / runtime-framework benchmarks on standard public GitHub-hosted Linux runners.

## Why paired runs

`ubuntu-latest` is reproducible at the image level but not a fixed physical machine. Absolute latency can vary between runs.

Therefore performance claims must come from **paired scenarios inside one job**:

- same runner;
- same CPU allocation;
- same memory allocation;
- same kernel/image;
- same compiler/Python toolchain;
- interleaved scenario order;
- repeated rounds.

Cross-run absolute numbers are retained for history but are not treated as the primary comparison.

## Log layout

```text
benchmark-logs/
├── manifest.json
├── pre/
│   ├── environment.json
│   ├── system.txt
│   └── vulkan.txt
├── post/
│   ├── environment.json
│   ├── system.txt
│   └── vulkan.txt
└── scenarios/
    └── <scenario>/
        ├── result.json
        ├── time.txt
        ├── vmstat.log
        ├── stdout.log
        └── stderr.log
```

## Environment data retained

- GitHub run ID / attempt / commit / ref;
- runner name / OS / architecture;
- kernel;
- CPU topology and model;
- visible CPU count;
- total/free memory;
- cgroup CPU, memory, and PID limits where exposed;
- process limits;
- filesystem state;
- load average;
- top process snapshot;
- Vulkan implementation/device summary.

## Per-scenario resource data

Each scenario is wrapped with:

`/usr/bin/time -v`

and a parallel `vmstat 1` sampler.

This retains, among other fields:

- elapsed wall time;
- user CPU time;
- system CPU time;
- CPU percentage;
- maximum resident set size;
- major/minor page faults;
- context switches;
- filesystem I/O;
- exit status;
- time-series runnable processes, memory, swap, CPU idle/wait.

Future S1-S6 benchmark harnesses should additionally emit structured JSONL/CSV with per-round latency, throughput, fallback rate, selector overhead, and allocation counts.

## GPU scope

Standard `ubuntu-latest` does **not** provide a stable physical GPU contract.

The free benchmark lane therefore records and validates **Mesa software Vulkan only**.

Physical Metal / CUDA / hardware Vulkan runs must be separate benchmark classes and must not be mixed into the standard-Linux comparison. Paid/specialized runners require explicit user approval before execution.

## Comparison rule

The primary statistic is the paired delta within a single run, not an absolute number from different GitHub runs.

Recommended execution order per round:

```text
round 1: S1 S2 S3 S4 S5 S6
round 2: S6 S5 S4 S3 S2 S1
round 3: rotated order
...
```

This reduces systematic warm-up/order bias.
