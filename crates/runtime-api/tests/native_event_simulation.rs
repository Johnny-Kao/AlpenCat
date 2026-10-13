use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode, RevalidationStatus,
    Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};

#[test]
fn event_burst_updates_epoch_and_cannot_make_old_boundary_fresh() {
    let rt = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4096, 262144),
        ..RuntimeConfig::default()
    });
    let snapshot = rt.boundary_snapshot();
    for i in 1..=32 {
        rt.invalidate_resources();
        assert_eq!(rt.resource_epoch(), snapshot.resource_epoch + i);
        assert!(rt.boundary_is_stale());
        assert_eq!(rt.boundary_snapshot(), snapshot);
    }
}

#[test]
fn native_event_simulation_recovery_and_noop_do_not_force_measurements() {
    let rt = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4096, 262144),
        ..RuntimeConfig::default()
    });
    let mut n = 0;
    let fresh = rt.revalidate_serial_cpu(BoundedRevalidationConfig::new(3, 512, 8192), |_, _| {
        n += 1;
        Some(1)
    });
    assert_eq!(fresh.status, RevalidationStatus::NotStale);
    assert_eq!(n, 0);

    rt.invalidate_resources();
    let stale_epoch = rt.resource_epoch();
    let result = rt.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8192),
        |items, backend| {
            Some(match backend {
                BackendKind::Serial => items as u64,
                BackendKind::Cpu => 1500,
                BackendKind::Gpu => unreachable!(),
            })
        },
    );
    assert_eq!(result.status, RevalidationStatus::Published);
    assert!(!rt.boundary_is_stale());
    assert_eq!(rt.boundary_snapshot().resource_epoch, stale_epoch);
    let task = TaskDefinition::new("native-simulation");
    let handle = rt
        .submit_map(&task, WorkRange::new(0, 2000), ExecutionMode::Auto, |i| {
            i + 1
        })
        .unwrap();
    assert_eq!(rt.wait(handle).value, (1..=2000).collect::<Vec<_>>());

    rt.invalidate_resources(); // simulated recovery transition, not an actual native event
    assert!(rt.boundary_is_stale());
    let result = rt.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8192),
        |_, backend| match backend {
            BackendKind::Cpu => None,
            _ => Some(1),
        },
    );
    assert_eq!(
        result.status,
        RevalidationStatus::RouteUnavailable(BackendKind::Cpu)
    );
    assert!(rt.boundary_is_stale());
}

#[test]
fn simultaneous_event_during_measurement_must_not_publish_as_fresh() {
    let rt = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4096, 262144),
        ..RuntimeConfig::default()
    });
    rt.invalidate_resources();
    let prior = rt.boundary_snapshot();
    let mut once = false;
    let out = rt.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8192),
        |items, backend| {
            if !once {
                rt.invalidate_resources();
                once = true;
            }
            Some(match backend {
                BackendKind::Serial => items as u64,
                BackendKind::Cpu => 1500,
                BackendKind::Gpu => unreachable!(),
            })
        },
    );
    assert_eq!(out.status, RevalidationStatus::InvalidatedDuringMeasurement);
    assert!(rt.boundary_is_stale());
    assert_eq!(rt.boundary_snapshot(), prior);
}
