# AlpenCat Resource Snapshot MVP — research prototype (not released)

Synchronous, read-only Linux process/cgroup v2 resource observation.
No background workers, no calibration, no automatic optimization, no hardware control.
No third-party Rust dependencies. Unsupported hosts return typed Unsupported readings.

Run: `cargo test` and `cargo run --example consumer`.

Limitations:
- Membership path assumes cgroup v2 mounted at /sys/fs/cgroup and matching proc path; container mount roots may differ. A failed lookup is Unsupported/Unknown, never zero capacity.
- Snapshot fields are individually read and **not atomic** with respect to resource changes.
- allowed_cpus is a process affinity permission list, **not** guaranteed CPU throughput or quota.
- CPU Pressure avg10 is ten-second stall percentage, **not** work-speed prediction.
- CPU and memory limits may be hierarchical: local cpu.max and memory.max must not be interpreted as the full effective bound if parent cgroups are tighter.
- Unit and availability contract is preliminary. No public release.
