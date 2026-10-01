use runtime_api::Runtime;

#[test]
fn cheap_host_profile_reports_basic_machine_capability() {
    let runtime = Runtime::new();
    let profile = runtime.host_profile();

    assert!(profile.logical_cpus >= 1);
    assert!(!profile.os.is_empty());
    assert!(!profile.architecture.is_empty());
}

#[test]
fn explicit_machine_profile_probe_preserves_host_profile() {
    let runtime = Runtime::new();
    let cheap = runtime.host_profile();
    let full = runtime.discover_machine_profile();

    assert_eq!(full.host.os, cheap.os);
    assert_eq!(full.host.architecture, cheap.architecture);
    assert_eq!(full.host.logical_cpus, cheap.logical_cpus);

    if std::env::var_os("RUNTIME_REQUIRE_GPU").is_some() {
        assert!(
            !full.gpus.is_empty(),
            "GPU was required but no GPU was discovered"
        );
        assert!(!full.gpus[0].name.is_empty());
        assert!(!full.gpus[0].backend.is_empty());
        assert!(!full.gpus[0].device_type.is_empty());
    }
}
