use runtime_api::{ResourceNotifier, Runtime};
use std::sync::Arc;

#[test]
fn portable_notifier_advances_epoch_and_diagnostics_track_staleness() {
    let runtime = Arc::new(Runtime::new());
    let notifier = ResourceNotifier::new(Arc::clone(&runtime));
    let initial = runtime.diagnostics();
    assert!(!initial.boundary_stale);
    assert_eq!(initial.boundary.resource_epoch, initial.resource_epoch);

    let new_epoch = notifier.notify_changed();
    let stale = runtime.diagnostics();
    assert_eq!(stale.resource_epoch, new_epoch);
    assert!(stale.boundary_stale);

    runtime.publish_boundary(stale.boundary.profile);
    let recovered = runtime.diagnostics();
    assert!(!recovered.boundary_stale);
    assert_eq!(recovered.boundary.resource_epoch, recovered.resource_epoch);
}

#[test]
fn cloned_notifiers_can_invalidate_concurrently() {
    let runtime = Arc::new(Runtime::new());
    let notifier = ResourceNotifier::new(Arc::clone(&runtime));
    let baseline = runtime.resource_epoch();
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let notifier = notifier.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    notifier.notify_changed();
                }
            })
        })
        .collect();

    for worker in workers {
        worker.join().expect("notification worker");
    }

    assert_eq!(runtime.resource_epoch(), baseline + 400);
    assert!(runtime.diagnostics().boundary_stale);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_observer_start_stop_and_repeated_shutdown() {
    use runtime_api::LinuxAffinityObserver;
    use std::time::Duration;

    let runtime = Arc::new(Runtime::new());
    let baseline = runtime.resource_epoch();
    let mut observer =
        LinuxAffinityObserver::start(Arc::clone(&runtime), Duration::from_millis(10))
            .expect("linux procfs affinity available");
    std::thread::sleep(Duration::from_millis(30));
    observer.stop();
    observer.stop();
    assert_eq!(runtime.resource_epoch(), baseline);
    assert!(LinuxAffinityObserver::start(runtime, Duration::ZERO).is_err());
}
