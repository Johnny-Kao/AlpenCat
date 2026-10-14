use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, ExecutionMode, ResourceNotifier,
    Runtime, RuntimeConfig, TaskDefinition, WorkRange,
};
use std::sync::Arc;

#[test]
fn concurrent_reads_notifications_and_revalidation_preserve_output() {
    let runtime = Arc::new(Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(128, usize::MAX),
        ..RuntimeConfig::default()
    }));
    let notifier = ResourceNotifier::new(Arc::clone(&runtime));
    let task = TaskDefinition::new("stress");
    let threads: Vec<_> = (0..4)
        .map(|worker| {
            let runtime = Arc::clone(&runtime);
            let notifier = notifier.clone();
            std::thread::spawn(move || {
                for iteration in 0..250 {
                    let len = 32 + (iteration * 7 + worker * 11) % 512;
                    let range = WorkRange::new(0, len);
                    let result = runtime.wait(
                        runtime
                            .submit_map(&task, range, ExecutionMode::Auto, |i| {
                                (i as u64).wrapping_mul(17).wrapping_add(3)
                            })
                            .expect("auto route must stay available"),
                    );
                    let expected: Vec<_> = (0..len)
                        .map(|i| (i as u64).wrapping_mul(17).wrapping_add(3))
                        .collect();
                    assert_eq!(result.value, expected);
                    if iteration % 25 == 0 {
                        notifier.notify_changed();
                        let _ = runtime.revalidate_serial_cpu(
                            BoundedRevalidationConfig::new(3, 64, 512),
                            |items, backend| {
                                Some(match backend {
                                    BackendKind::Serial => items as u64,
                                    BackendKind::Cpu => 200,
                                    BackendKind::Gpu => unreachable!(),
                                })
                            },
                        );
                    }
                    let _ = runtime.diagnostics();
                }
            })
        })
        .collect();

    for thread in threads {
        thread.join().expect("stress worker");
    }
    assert!(runtime.resource_epoch() >= 40);
}
