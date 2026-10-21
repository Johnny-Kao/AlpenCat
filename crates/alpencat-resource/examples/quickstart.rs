use alpencat_resource::{snapshot, Reading};
fn main() {
    let s = snapshot();
    println!("Resource observation window: {:?} .. {:?}", s.observed_at, s.completed_at);
    match &s.allowed_cpus {
        Reading::Known(cpus) => println!("Process affinity allows {} CPU(s)", cpus.len()),
        Reading::Unknown => println!("CPU affinity: unknown"),
        Reading::Unsupported => println!("CPU affinity: unsupported on this OS"),
        Reading::Unavailable(reason) => println!("CPU affinity: unavailable ({reason})"),
    }
    println!("Cgroup mapped: {}; visible hierarchy complete: {}",
        s.cgroup_resolved, s.limits_complete);
    println!("Effective visible CPU quota: {:?}", s.effective_cpu_capacity);
    println!("Visible hierarchical memory maximum: {:?}", s.memory_max_bytes);
    // No execution Runtime, background polling, automatic calibration, or policy.
}
