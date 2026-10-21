use std::time::Instant;
fn main() {
    let repeats = 2_000;
    let start = Instant::now();
    let mut success = 0_u64;
    for _ in 0..repeats {
        let s = alpencat_resource_snapshot::snapshot();
        if matches!(s.allowed_cpus, alpencat_resource_snapshot::Reading::Known(_)) {
            success += 1;
        }
    }
    let duration = start.elapsed();
    println!("snapshots={} allowed_cpus_known={} elapsed_ns={} mean_ns={:.1}",
        repeats, success, duration.as_nanos(), duration.as_nanos() as f64 / repeats as f64);
    println!("NOTE: elapsed includes OS file I/O and is not a zero-cost hot-path operation");
}
