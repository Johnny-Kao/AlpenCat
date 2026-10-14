use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, RevalidationStatus, Runtime,
    RuntimeConfig,
};

#[test]
fn initial_calibration_publishes_only_on_local_crossover() {
    let runtime = Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(1024, 262144),
        ..RuntimeConfig::default()
    });
    let outcome = runtime.calibrate_serial_cpu(
        BoundedRevalidationConfig::new(3, 256, 8192),
        |items, backend| match backend {
            BackendKind::Serial => Some(items as u64),
            BackendKind::Cpu => Some(1500),
            BackendKind::Gpu => None,
        },
    );
    assert_eq!(outcome.status, RevalidationStatus::Published);
    assert_eq!(
        outcome.published_boundary,
        Some(runtime.boundary_snapshot())
    );
    assert!(!runtime.boundary_is_stale());
}

#[test]
fn unavailable_backend_must_not_publish_initial_calibration() {
    let runtime = Runtime::new();
    let previous = runtime.boundary_snapshot();
    let outcome = runtime.calibrate_serial_cpu(
        BoundedRevalidationConfig::new(3, 256, 8192),
        |_items, backend| match backend {
            BackendKind::Cpu => None,
            BackendKind::Serial => Some(100),
            BackendKind::Gpu => None,
        },
    );
    assert_eq!(
        outcome.status,
        RevalidationStatus::RouteUnavailable(BackendKind::Cpu)
    );
    assert_eq!(runtime.boundary_snapshot(), previous);
}

#[test]
fn concurrent_invalidation_disallows_calibration_publication() {
    let runtime = Runtime::new();
    let outcome = runtime.calibrate_serial_cpu(
        BoundedRevalidationConfig::new(3, 256, 8192),
        |items, backend| {
            if backend == BackendKind::Serial {
                runtime.invalidate_resources();
            }
            Some(match backend {
                BackendKind::Serial => items as u64,
                BackendKind::Cpu => 1500,
                BackendKind::Gpu => unreachable!(),
            })
        },
    );
    assert_eq!(
        outcome.status,
        RevalidationStatus::InvalidatedDuringMeasurement
    );
    assert!(outcome.published_boundary.is_none());
}
