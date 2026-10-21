use alpencat_resource_snapshot::{snapshot, Reading};

fn main() {
    // Independent Rust application: no Runtime or execution backend required.
    let s = snapshot();
    assert!(s.completed_at.duration_since(s.observed_at).is_ok());
    let cpu = match s.allowed_cpus {
        Reading::Known(ref v) => format!("{} permitted CPU(s)", v.len()),
        Reading::Unsupported => "unsupported".to_string(),
        Reading::Unknown => "unknown".to_string(),
        Reading::Unavailable(ref e) => format!("unavailable: {e}"),
    };
    println!("Resource Snapshot consumer: {cpu}; cgroup_resolved={}", s.cgroup_resolved);
}
