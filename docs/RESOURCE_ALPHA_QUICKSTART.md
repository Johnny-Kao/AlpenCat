# Resource Snapshot — Alpha quickstart (candidate)

**Status: experimental source alpha v0.5.0-alpha.** No production certification or crates.io publication.

## Install from candidate source

```toml
[dependencies]
alpencat-resource = { git = "https://github.com/Johnny-Kao/AlpenCat", tag = "v0.5.0-alpha" }
```

The source tag identifies an immutable commit. For particularly sensitive deployments, pin that commit SHA in addition to reviewing the documented limitations.

## Use

```rust
use alpencat_resource::{snapshot, Reading};

fn main() {
    let observation = snapshot();
    match observation.allowed_cpus {
        Reading::Known(cpus) => println!("Process affinity CPU count: {}", cpus.len()),
        Reading::Unknown => eprintln!("CPU affinity unknown"),
        Reading::Unsupported => eprintln!("CPU affinity unsupported"),
        Reading::Unavailable(reason) => eprintln!("Cannot read affinity: {reason}"),
    }
}
```

You **do not** need to construct `runtime_api::Runtime` or run benchmarks. Snapshots are on-demand and synchronous; they do filesystem I/O and should not be called from an extremely hot loop.

### Semantics

- `allowed_cpus`: permitted process affinity list, **not** dedicated CPU throughput.
- `cpu_quota_us`, `cpu_period_us`, `effective_cpu_capacity`: strictest quota/period among **visible** ancestors, and its CPU-equivalent ratio; this does not account for CPU contention, other tenants, hidden ancestors, or actual throughput.
- `memory_max_bytes`: strictest limit visible in Cgroup v2 ancestry; it is not guaranteed to include limits outside an isolated mount.
- `limits_complete`: whether the visible cgroup2 mount maps to the unified root as detected, not a universal host-policy certification.
- `observed_at` / `completed_at`: interval containing individual file reads, **not** an atomic snapshot. `membership_changed_during_read` detects some, not all, races.
- Missing signals are explicitly `Unknown`, `Unsupported` or `Unavailable`; there is no fabricated zero.
- Non-Linux OSes return `Unsupported` for Linux-specific fields.
- The API does not choose execution strategies or alter CPU/GPU power settings.

### Compatibility

The existing `runtime-api` still supports calibration and execution routing unchanged. New consumers may depend only on `alpencat-resource`. Workspace package version is still `0.1.0`; alpha version labels refer to **source releases**, not crates.io publication.

### Limits

Cgroup namespace and mount roots are complex; unavailable or hidden parents cannot be proven safe, and the current adapter is best-effort. Do not use Resource Snapshot as an authoritative enforcement/security control. Performance and user adoption claims require separate evidence.
