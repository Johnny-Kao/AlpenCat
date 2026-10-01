//! Lightweight runtime-owned telemetry.
//!
//! This layer measures work the runtime itself performs. It does not require
//! privileged machine counters or vendor-specific monitoring APIs.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use runtime_core::BackendKind;

#[derive(Debug, Clone, Copy, Default)]
struct LatestSample {
    work_items: u64,
    elapsed_nanos: u64,
}

#[derive(Debug, Default)]
struct BackendTelemetry {
    in_flight: AtomicUsize,
    completed: AtomicU64,
    failed: AtomicU64,
    work_items: AtomicU64,
    elapsed_nanos: AtomicU64,
    latest: Mutex<LatestSample>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackendTelemetrySnapshot {
    pub in_flight: usize,
    pub completed: u64,
    pub failed: u64,
    pub work_items: u64,
    pub elapsed_nanos: u64,
    pub last_work_items: u64,
    pub last_elapsed_nanos: u64,
}

impl BackendTelemetrySnapshot {
    pub fn average_nanos_per_item(self) -> Option<f64> {
        if self.work_items == 0 {
            return None;
        }
        Some(self.elapsed_nanos as f64 / self.work_items as f64)
    }

    pub fn average_items_per_second(self) -> Option<f64> {
        if self.elapsed_nanos == 0 {
            return None;
        }
        Some(self.work_items as f64 * 1_000_000_000.0 / self.elapsed_nanos as f64)
    }

    pub fn last_items_per_second(self) -> Option<f64> {
        if self.last_elapsed_nanos == 0 {
            return None;
        }
        Some(self.last_work_items as f64 * 1_000_000_000.0 / self.last_elapsed_nanos as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuntimeTelemetrySnapshot {
    pub serial: BackendTelemetrySnapshot,
    pub cpu: BackendTelemetrySnapshot,
    pub gpu: BackendTelemetrySnapshot,
}

#[derive(Debug, Default)]
pub struct RuntimeTelemetry {
    serial: BackendTelemetry,
    cpu: BackendTelemetry,
    gpu: BackendTelemetry,
}

pub struct ExecutionObservation<'a> {
    telemetry: &'a RuntimeTelemetry,
    backend: BackendKind,
    work_items: usize,
    started: std::time::Instant,
    finished: bool,
}

impl RuntimeTelemetry {
    pub fn begin(&self, backend: BackendKind, work_items: usize) -> ExecutionObservation<'_> {
        self.backend(backend)
            .in_flight
            .fetch_add(1, Ordering::Relaxed);

        ExecutionObservation {
            telemetry: self,
            backend,
            work_items,
            started: std::time::Instant::now(),
            finished: false,
        }
    }

    pub fn snapshot(&self) -> RuntimeTelemetrySnapshot {
        RuntimeTelemetrySnapshot {
            serial: self.snapshot_backend(BackendKind::Serial),
            cpu: self.snapshot_backend(BackendKind::Cpu),
            gpu: self.snapshot_backend(BackendKind::Gpu),
        }
    }

    pub fn record(
        &self,
        backend: BackendKind,
        work_items: usize,
        elapsed: Duration,
        success: bool,
    ) {
        let target = self.backend(backend);
        let elapsed_nanos = (elapsed.as_nanos().min(u128::from(u64::MAX)) as u64).max(1);
        target
            .work_items
            .fetch_add(work_items as u64, Ordering::Relaxed);
        target
            .elapsed_nanos
            .fetch_add(elapsed_nanos, Ordering::Relaxed);
        {
            let mut latest = target
                .latest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *latest = LatestSample {
                work_items: work_items as u64,
                elapsed_nanos,
            };
        }

        if success {
            target.completed.fetch_add(1, Ordering::Relaxed);
        } else {
            target.failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn backend(&self, backend: BackendKind) -> &BackendTelemetry {
        match backend {
            BackendKind::Serial => &self.serial,
            BackendKind::Cpu => &self.cpu,
            BackendKind::Gpu => &self.gpu,
        }
    }

    fn snapshot_backend(&self, backend: BackendKind) -> BackendTelemetrySnapshot {
        let source = self.backend(backend);
        let latest = *source
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        BackendTelemetrySnapshot {
            in_flight: source.in_flight.load(Ordering::Relaxed),
            completed: source.completed.load(Ordering::Relaxed),
            failed: source.failed.load(Ordering::Relaxed),
            work_items: source.work_items.load(Ordering::Relaxed),
            elapsed_nanos: source.elapsed_nanos.load(Ordering::Relaxed),
            last_work_items: latest.work_items,
            last_elapsed_nanos: latest.elapsed_nanos,
        }
    }

    fn finish(&self, backend: BackendKind, work_items: usize, elapsed: Duration, success: bool) {
        let target = self.backend(backend);
        target.in_flight.fetch_sub(1, Ordering::Relaxed);
        let elapsed_nanos = (elapsed.as_nanos().min(u128::from(u64::MAX)) as u64).max(1);
        target
            .work_items
            .fetch_add(work_items as u64, Ordering::Relaxed);
        target
            .elapsed_nanos
            .fetch_add(elapsed_nanos, Ordering::Relaxed);
        {
            let mut latest = target
                .latest
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *latest = LatestSample {
                work_items: work_items as u64,
                elapsed_nanos,
            };
        }

        if success {
            target.completed.fetch_add(1, Ordering::Relaxed);
        } else {
            target.failed.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl ExecutionObservation<'_> {
    pub fn success(mut self) {
        self.finish(true);
    }

    pub fn failure(mut self) {
        self.finish(false);
    }

    fn finish(&mut self, success: bool) {
        if self.finished {
            return;
        }
        self.telemetry.finish(
            self.backend,
            self.work_items,
            self.started.elapsed(),
            success,
        );
        self.finished = true;
    }
}

impl Drop for ExecutionObservation<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_success_and_throughput() {
        let telemetry = RuntimeTelemetry::default();
        let observation = telemetry.begin(BackendKind::Cpu, 100);
        observation.success();

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.cpu.in_flight, 0);
        assert_eq!(snapshot.cpu.completed, 1);
        assert_eq!(snapshot.cpu.failed, 0);
        assert_eq!(snapshot.cpu.work_items, 100);
        assert!(snapshot.cpu.elapsed_nanos > 0);
        assert!(snapshot.cpu.average_items_per_second().is_some());
    }

    #[test]
    fn dropped_observation_records_failure() {
        let telemetry = RuntimeTelemetry::default();
        {
            let _observation = telemetry.begin(BackendKind::Gpu, 64);
        }

        let snapshot = telemetry.snapshot();
        assert_eq!(snapshot.gpu.in_flight, 0);
        assert_eq!(snapshot.gpu.completed, 0);
        assert_eq!(snapshot.gpu.failed, 1);
        assert_eq!(snapshot.gpu.work_items, 64);
    }

    #[test]
    fn in_flight_is_visible_before_completion() {
        let telemetry = RuntimeTelemetry::default();
        let observation = telemetry.begin(BackendKind::Serial, 1);
        assert_eq!(telemetry.snapshot().serial.in_flight, 1);
        observation.success();
        assert_eq!(telemetry.snapshot().serial.in_flight, 0);
    }

    #[test]
    fn latest_sample_remains_pair_consistent_under_concurrency() {
        use std::sync::Arc;
        use std::thread;

        let telemetry = Arc::new(RuntimeTelemetry::default());
        let left = Arc::clone(&telemetry);
        let right = Arc::clone(&telemetry);

        let writer_a = thread::spawn(move || {
            for _ in 0..10_000 {
                left.record(BackendKind::Cpu, 11, Duration::from_nanos(101), true);
            }
        });

        let writer_b = thread::spawn(move || {
            for _ in 0..10_000 {
                right.record(BackendKind::Cpu, 22, Duration::from_nanos(202), true);
            }
        });

        for _ in 0..20_000 {
            let sample = telemetry.snapshot().cpu;
            assert!(
                (sample.last_work_items == 0 && sample.last_elapsed_nanos == 0)
                    || (sample.last_work_items == 11 && sample.last_elapsed_nanos == 101)
                    || (sample.last_work_items == 22 && sample.last_elapsed_nanos == 202)
            );
        }

        writer_a.join().unwrap();
        writer_b.join().unwrap();
    }
}
