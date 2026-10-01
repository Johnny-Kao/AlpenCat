use runtime_api::{
    BackendKind, CalibrationProfile, ExecutionMode, Runtime, RuntimeConfig, TaskDefinition,
    WorkRange,
};

#[test]
fn runtime_uses_supplied_calibration_profile() {
    let runtime = Runtime::with_config(RuntimeConfig {
        calibration: CalibrationProfile {
            serial_max_items: 8,
            cpu_max_items: 32,
        },
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let small = runtime
        .submit_map(&task, WorkRange::new(0, 8), ExecutionMode::Auto, |index| {
            index
        })
        .expect("small calibrated task must execute");
    let medium = runtime
        .submit_map(&task, WorkRange::new(0, 9), ExecutionMode::Auto, |index| {
            index
        })
        .expect("medium calibrated task must execute");

    assert_eq!(small.decision().backend, BackendKind::Serial);
    assert_eq!(medium.decision().backend, BackendKind::Cpu);
}

#[test]
fn runtime_exposes_active_calibration_profile() {
    let profile = CalibrationProfile {
        serial_max_items: 128,
        cpu_max_items: 8192,
    };
    let runtime = Runtime::with_config(RuntimeConfig {
        calibration: profile,
        ..RuntimeConfig::default()
    });

    assert_eq!(runtime.calibration(), profile);
}

#[test]
fn task_specific_calibration_overrides_runtime_default() {
    let runtime = Runtime::with_config(RuntimeConfig {
        calibration: CalibrationProfile {
            serial_max_items: 1_000_000,
            cpu_max_items: 2_000_000,
        },
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map").with_calibration(CalibrationProfile {
        serial_max_items: 4,
        cpu_max_items: 16,
    });

    let handle = runtime
        .submit_map(&task, WorkRange::new(0, 8), ExecutionMode::Auto, |index| {
            index
        })
        .expect("task-specific calibrated task must execute");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    assert_eq!(
        task.calibration(),
        Some(CalibrationProfile {
            serial_max_items: 4,
            cpu_max_items: 16,
        })
    );
}
