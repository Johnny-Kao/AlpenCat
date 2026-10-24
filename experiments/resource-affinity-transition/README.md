# Live CPU Affinity Transition: real parallel workload economics

**Research experiment only.** This modifies CPU affinity of a dedicated benchmark subprocess (owned by the workflow), not the host's global CPU, system-wide cgroups or other users' processes.

- The Python controller changes the Rust worker process's **actual Linux main-thread CPU affinity** with `os.sched_setaffinity(pid, mask)` and verifies `os.sched_getaffinity(pid)`.
- One Rust process per strategy performs the three real transitions **1 CPU → 2 CPUs → 1 CPU**. Newly spawned scoped worker threads inherit the current main-thread affinity; worker-side checks verify the observed CPU count before computing.
- Parallelism is real: each phase processes 96 independent deterministic CPU-intensive blocks on either 1 or 2 threads. Threads execute concurrently where CPUs are permitted; checksums must match across every strategy and phase.
- Four policies: `fixed_one` always uses one thread; `fixed_two` always uses two; `direct_event` reads `snapshot()` exactly once per externally reported transition; `alpencat` calls `ResourceState::invalidate()+refresh()` and chooses threads from the returned snapshot.
- This is a **fair efficient event-driven direct API comparator** and two static baselines. No gratuitous per-task polling. Affinity setting is excluded from the worker's phase timer (same controller and operation in all modes); observation, thread creation and actual computation are included.
- Raw phase CPU time is Linux `CLOCK_PROCESS_CPUTIME_ID` measured from inside the process, including aggregate parallel worker CPU consumption. Wall clock is `Instant`. Observation time is also reported. Controller affinity-transition time is separately reported.
- Two CPU-bound workload sizes (`light` and `heavy`), 7 shuffled and paired rounds per size, plus warmups outside measurements, on Ubuntu 22.04 and 24.04.
- Each phase's checksum must agree across all modes; each phase's actual available CPU count must equal 1→2→1; an adaptive mode must select 1→2→1 workers. Violations fail CI.
- A positive **distinct AlpenCat** outcome requires both CPU and wall time to improve by >2% for ≥6/7 paired rounds against **the better of the two fixed strategies** and against the **direct-event baseline**. Report all negative cases unmodified; economics NO-GO is *not* a CI failure.
- These benchmarks modify Linux affinity only, **not** hierarchical `cpu.max` quotas. They use a controlled standalone application, not an existing production workload. CPU hot-plug, long-lived thread pools, memory bandwidth or real application service traffic are out of scope.

The goal is to determine whether the new lifecycle adds compute value beyond reusing the OS API, not to force a favorable benchmark result. `main` and published releases are excluded.
