//! Dynamic resource-broker primitives.
//!
//! The broker chooses where the next pending WorkUnit should go based on
//! current runtime-owned pressure. It does not yet model execution cost.

use runtime_core::BackendKind;
use runtime_planner::{WorkQueue, WorkUnit};
use runtime_telemetry::RuntimeTelemetrySnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerCapacity {
    pub cpu_slots: usize,
    pub gpu_slots: usize,
}

impl BrokerCapacity {
    pub const fn new(cpu_slots: usize, gpu_slots: usize) -> Self {
        Self {
            cpu_slots,
            gpu_slots,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerRequest {
    pub gpu_range_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkAssignment {
    pub unit: WorkUnit,
    pub backend: BackendKind,
}

#[derive(Debug, Default)]
pub struct ResourceBroker;

impl ResourceBroker {
    pub fn claim_next(
        &self,
        queue: &mut WorkQueue,
        telemetry: RuntimeTelemetrySnapshot,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Option<WorkAssignment> {
        let backend = self.select_backend(telemetry, capacity, request)?;
        let unit = queue.claim_next()?;
        Some(WorkAssignment { unit, backend })
    }

    pub fn return_unit(&self, queue: &mut WorkQueue, assignment: WorkAssignment) {
        queue.requeue_front(assignment.unit);
    }

    pub fn select_backend(
        &self,
        telemetry: RuntimeTelemetrySnapshot,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Option<BackendKind> {
        let cpu_available = capacity.cpu_slots > telemetry.cpu.in_flight;
        let gpu_available =
            request.gpu_range_eligible && capacity.gpu_slots > telemetry.gpu.in_flight;

        match (cpu_available, gpu_available) {
            (false, false) => None,
            (true, false) => Some(BackendKind::Cpu),
            (false, true) => Some(BackendKind::Gpu),
            (true, true) => {
                let cpu_pressure = telemetry.cpu.in_flight as f64 / capacity.cpu_slots as f64;
                let gpu_pressure = telemetry.gpu.in_flight as f64 / capacity.gpu_slots as f64;

                if gpu_pressure < cpu_pressure {
                    Some(BackendKind::Gpu)
                } else {
                    Some(BackendKind::Cpu)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::WorkRange;
    use runtime_planner::{ChunkPlanner, ChunkPolicy};
    use runtime_telemetry::BackendTelemetrySnapshot;

    fn backend(in_flight: usize, completed: u64, failed: u64) -> BackendTelemetrySnapshot {
        BackendTelemetrySnapshot {
            in_flight,
            completed,
            failed,
            work_items: 0,
            elapsed_nanos: 0,
            last_work_items: 0,
            last_elapsed_nanos: 0,
        }
    }

    fn telemetry(cpu_in_flight: usize, gpu_in_flight: usize) -> RuntimeTelemetrySnapshot {
        RuntimeTelemetrySnapshot {
            serial: backend(0, 0, 0),
            cpu: backend(cpu_in_flight, 0, 0),
            gpu: backend(gpu_in_flight, 0, 0),
        }
    }

    fn queue() -> WorkQueue {
        let plan = ChunkPlanner.plan(WorkRange::new(0, 4096), ChunkPolicy::fixed(1024));
        WorkQueue::from_plan(&plan)
    }

    #[test]
    fn cpu_is_conservative_default_when_pressure_is_equal() {
        let broker = ResourceBroker;
        let mut queue = queue();

        let assignment = broker
            .claim_next(
                &mut queue,
                telemetry(0, 0),
                BrokerCapacity::new(4, 1),
                BrokerRequest {
                    gpu_range_eligible: true,
                },
            )
            .unwrap();

        assert_eq!(assignment.backend, BackendKind::Cpu);
    }

    #[test]
    fn gpu_can_take_work_when_cpu_is_more_loaded() {
        let broker = ResourceBroker;
        let mut queue = queue();

        let assignment = broker
            .claim_next(
                &mut queue,
                telemetry(3, 0),
                BrokerCapacity::new(4, 1),
                BrokerRequest {
                    gpu_range_eligible: true,
                },
            )
            .unwrap();

        assert_eq!(assignment.backend, BackendKind::Gpu);
    }

    #[test]
    fn cpu_takes_work_when_gpu_is_full() {
        let broker = ResourceBroker;
        let mut queue = queue();

        let assignment = broker
            .claim_next(
                &mut queue,
                telemetry(0, 1),
                BrokerCapacity::new(4, 1),
                BrokerRequest {
                    gpu_range_eligible: true,
                },
            )
            .unwrap();

        assert_eq!(assignment.backend, BackendKind::Cpu);
    }

    #[test]
    fn broker_does_not_claim_when_all_eligible_resources_are_full() {
        let broker = ResourceBroker;
        let mut queue = queue();
        let before = queue.pending_count();

        let assignment = broker.claim_next(
            &mut queue,
            telemetry(4, 1),
            BrokerCapacity::new(4, 1),
            BrokerRequest {
                gpu_range_eligible: true,
            },
        );

        assert!(assignment.is_none());
        assert_eq!(queue.pending_count(), before);
    }

    #[test]
    fn gpu_ineligible_work_never_uses_gpu() {
        let broker = ResourceBroker;
        let mut queue = queue();

        let assignment = broker
            .claim_next(
                &mut queue,
                telemetry(3, 0),
                BrokerCapacity::new(4, 1),
                BrokerRequest {
                    gpu_range_eligible: false,
                },
            )
            .unwrap();

        assert_eq!(assignment.backend, BackendKind::Cpu);
    }

    #[test]
    fn returned_unit_is_available_again() {
        let broker = ResourceBroker;
        let mut queue = queue();

        let first = broker
            .claim_next(
                &mut queue,
                telemetry(0, 0),
                BrokerCapacity::new(4, 1),
                BrokerRequest {
                    gpu_range_eligible: true,
                },
            )
            .unwrap();

        broker.return_unit(&mut queue, first);

        let next = broker
            .claim_next(
                &mut queue,
                telemetry(0, 0),
                BrokerCapacity::new(4, 1),
                BrokerRequest {
                    gpu_range_eligible: true,
                },
            )
            .unwrap();

        assert_eq!(next.unit.id, first.unit.id);
    }
}
