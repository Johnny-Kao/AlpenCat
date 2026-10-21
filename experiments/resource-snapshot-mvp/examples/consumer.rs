fn main() {
    let s = alpencat_resource_snapshot::snapshot();
    println!("{s:#?}");
    // No Runtime construction, thread, or performance-policy dependency.
}
