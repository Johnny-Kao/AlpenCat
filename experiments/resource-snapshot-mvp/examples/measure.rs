use std::{fs, hint::black_box, time::Instant};
fn measured<F: FnMut()>(n: usize, mut f: F) -> (u128, u128) {
    let start = Instant::now();
    for _ in 0..n { f(); }
    let elapsed = start.elapsed().as_nanos();
    (elapsed, elapsed / n as u128)
}
fn main() {
    let n = 2000;
    let (_, snapshot_ns) = measured(n, || black_box(alpencat_resource_snapshot::snapshot()));
    let (_, direct_ns) = measured(n, || {
        black_box(fs::read_to_string("/proc/self/status").ok());
        black_box(fs::read_to_string("/proc/self/cgroup").ok());
        black_box(fs::read_to_string("/proc/self/mountinfo").ok());
    });
    println!("samples={n} snapshot_mean_ns={snapshot_ns} direct_three_reads_mean_ns={direct_ns}");
    println!("Direct three reads is a partial lower-level baseline, NOT semantic equivalence.");
}
