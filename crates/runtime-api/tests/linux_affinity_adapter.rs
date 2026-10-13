#![cfg(target_os = "linux")]
use runtime_api::{
    BackendKind, BoundaryProfile, BoundedRevalidationConfig, RevalidationStatus, Runtime,
    RuntimeConfig,
};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

fn affinity() -> String {
    let status = std::fs::read_to_string("/proc/self/status").expect("proc status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:\t"))
        .expect("Cpus_allowed_list")
        .to_owned()
}

fn available_cpus() -> Vec<usize> {
    affinity()
        .split(',')
        .flat_map(|item| {
            let mut split = item.split('-');
            let first: usize = split.next().unwrap().parse().unwrap();
            let last: usize = split.next().map_or(first, |v| v.parse().unwrap());
            first..=last
        })
        .collect()
}

fn set_affinity(mask: &str) {
    let result = Command::new("taskset")
        .args(["-pc", mask, &std::process::id().to_string()])
        .output()
        .expect("taskset installed");
    assert!(
        result.status.success(),
        "taskset failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

struct WatcherGuard {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for WatcherGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            if !thread::panicking() {
                handle.join().expect("native observation worker");
            } else {
                let _ = handle.join();
            }
        }
    }
}

struct Restore(String);
impl Drop for Restore {
    fn drop(&mut self) {
        set_affinity(&self.0);
    }
}

fn until(deadline: Duration, predicate: impl Fn() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < deadline,
            "OS transition not observed in time"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn actual_linux_affinity_changes_trigger_epoch_and_bounded_revalidation() {
    if Command::new("taskset").arg("--version").output().is_err() {
        eprintln!("UNAVAILABLE: taskset not installed");
        return;
    }
    let cpus = available_cpus();
    if cpus.len() < 2 {
        eprintln!("UNAVAILABLE: insufficient allowed CPUs for affinity transition");
        return;
    }
    let original = affinity();
    let _restore = Restore(original.clone());
    let rt = Arc::new(Runtime::with_config(RuntimeConfig {
        boundary: BoundaryProfile::new(4096, 262144),
        ..RuntimeConfig::default()
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let events = Arc::new(AtomicUsize::new(0));
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let watcher = {
        let rt = Arc::clone(&rt);
        let stop = Arc::clone(&stop);
        let events = Arc::clone(&events);
        thread::spawn(move || {
            let mut last = affinity();
            ready_tx.send(()).expect("watcher ready receiver");
            while !stop.load(Ordering::SeqCst) {
                let now = affinity();
                if now != last {
                    last = now;
                    rt.invalidate_resources();
                    events.fetch_add(1, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(5));
            }
        })
    };
    ready_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("watcher startup");
    let watcher = WatcherGuard {
        stop,
        handle: Some(watcher),
    };
    thread::sleep(Duration::from_millis(30));
    assert_eq!(
        events.load(Ordering::SeqCst),
        0,
        "steady state must not invalidate"
    );
    assert!(!rt.boundary_is_stale());
    set_affinity(&cpus[0].to_string());
    until(Duration::from_secs(3), || {
        events.load(Ordering::SeqCst) >= 1
    });
    assert_eq!(affinity(), cpus[0].to_string());
    assert!(rt.boundary_is_stale());
    let result = rt.revalidate_serial_cpu(
        BoundedRevalidationConfig::new(3, 512, 8192),
        |work_items, backend| {
            Some(match backend {
                BackendKind::Serial => work_items as u64,
                BackendKind::Cpu => 1500,
                BackendKind::Gpu => unreachable!(),
            })
        },
    );
    assert_eq!(result.status, RevalidationStatus::Published);
    assert!(!rt.boundary_is_stale());
    set_affinity(&original);
    until(Duration::from_secs(3), || {
        events.load(Ordering::SeqCst) >= 2
    });
    assert!(rt.boundary_is_stale());
    assert_eq!(affinity(), original);
    drop(watcher);
    let final_epoch = rt.resource_epoch();
    thread::sleep(Duration::from_millis(20));
    assert_eq!(
        rt.resource_epoch(),
        final_epoch,
        "no post-stop invalidation"
    );
    println!(
        "REAL_LINUX_AFFINITY_TO_EPOCH=PASS events={}",
        events.load(Ordering::SeqCst)
    );
}
