//! Host-project integration and migration safety.
//!
//! This crate stays outside the resource-control core. It helps an existing
//! project decide whether a validated workload enters the runtime or remains
//! on the host implementation during migration.

use runtime_execution_planner::ExecutionPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionOwner {
    Runtime,
    HostExisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationReason {
    RuntimeValidated,
    UnsupportedWork,
    NotYetValidated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationDecision {
    pub owner: ExecutionOwner,
    pub reason: MigrationReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    pub execution_plan: ExecutionPlan,
    pub decision: MigrationDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReadiness {
    pub supported: bool,
    pub validated: bool,
}

impl MigrationReadiness {
    pub const fn validated() -> Self {
        Self {
            supported: true,
            validated: true,
        }
    }

    pub const fn supported_unvalidated() -> Self {
        Self {
            supported: true,
            validated: false,
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            supported: false,
            validated: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationPolicy;

impl IntegrationPolicy {
    pub const fn decide_readiness(self, readiness: MigrationReadiness) -> MigrationDecision {
        if !readiness.supported {
            return MigrationDecision {
                owner: ExecutionOwner::HostExisting,
                reason: MigrationReason::UnsupportedWork,
            };
        }

        if !readiness.validated {
            return MigrationDecision {
                owner: ExecutionOwner::HostExisting,
                reason: MigrationReason::NotYetValidated,
            };
        }

        MigrationDecision {
            owner: ExecutionOwner::Runtime,
            reason: MigrationReason::RuntimeValidated,
        }
    }

    pub const fn decide(
        self,
        readiness: MigrationReadiness,
        _plan: &ExecutionPlan,
    ) -> MigrationDecision {
        self.decide_readiness(readiness)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseState {
    Planned,
    RuntimeCommitted,
    HostCommitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackError {
    AlreadyCommitted,
    RuntimeNotSelected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLease {
    decision: MigrationDecision,
    state: LeaseState,
}

impl ExecutionLease {
    pub const fn new(decision: MigrationDecision) -> Self {
        Self {
            decision,
            state: LeaseState::Planned,
        }
    }

    pub const fn decision(self) -> MigrationDecision {
        self.decision
    }

    pub const fn state(self) -> LeaseState {
        self.state
    }

    pub fn commit_runtime(&mut self) -> Result<(), FallbackError> {
        if self.state != LeaseState::Planned {
            return Err(FallbackError::AlreadyCommitted);
        }
        if self.decision.owner != ExecutionOwner::Runtime {
            return Err(FallbackError::RuntimeNotSelected);
        }
        self.state = LeaseState::RuntimeCommitted;
        Ok(())
    }

    pub fn commit_host(&mut self) -> Result<(), FallbackError> {
        if self.state != LeaseState::Planned {
            return Err(FallbackError::AlreadyCommitted);
        }
        self.state = LeaseState::HostCommitted;
        Ok(())
    }

    pub fn fallback_to_host_before_execution(&mut self) -> Result<(), FallbackError> {
        if self.state != LeaseState::Planned {
            return Err(FallbackError::AlreadyCommitted);
        }
        if self.decision.owner != ExecutionOwner::Runtime {
            return Err(FallbackError::RuntimeNotSelected);
        }

        self.decision = MigrationDecision {
            owner: ExecutionOwner::HostExisting,
            reason: MigrationReason::NotYetValidated,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::BackendKind;
    use runtime_execution_planner::{BackendMix, ResidencyHint};

    fn plan(confidence_milli: u16) -> ExecutionPlan {
        ExecutionPlan {
            primary_backend: BackendKind::Cpu,
            backend_mix: BackendMix::single(BackendKind::Cpu),
            cpu_parallelism: 4,
            chunk_size: 1024,
            max_in_flight: 4,
            memory_budget_bytes: Some(1 << 30),
            gpu_device: None,
            residency_hint: ResidencyHint::None,
            confidence_milli,
        }
    }

    #[test]
    fn validated_work_enters_runtime_even_during_cost_model_cold_start() {
        let decision = IntegrationPolicy.decide(MigrationReadiness::validated(), &plan(0));
        assert_eq!(decision.owner, ExecutionOwner::Runtime);
        assert_eq!(decision.reason, MigrationReason::RuntimeValidated);
    }

    #[test]
    fn readiness_only_decision_matches_plan_aware_compatibility_api() {
        for readiness in [
            MigrationReadiness::validated(),
            MigrationReadiness::supported_unvalidated(),
            MigrationReadiness::unsupported(),
        ] {
            assert_eq!(
                IntegrationPolicy.decide_readiness(readiness),
                IntegrationPolicy.decide(readiness, &plan(0))
            );
        }
    }

    #[test]
    fn unsupported_work_stays_on_host() {
        let decision = IntegrationPolicy.decide(MigrationReadiness::unsupported(), &plan(900));
        assert_eq!(decision.owner, ExecutionOwner::HostExisting);
        assert_eq!(decision.reason, MigrationReason::UnsupportedWork);
    }

    #[test]
    fn supported_but_unvalidated_work_stays_on_host() {
        let decision =
            IntegrationPolicy.decide(MigrationReadiness::supported_unvalidated(), &plan(900));
        assert_eq!(decision.owner, ExecutionOwner::HostExisting);
        assert_eq!(decision.reason, MigrationReason::NotYetValidated);
    }

    #[test]
    fn fallback_is_allowed_only_before_runtime_commit() {
        let decision = IntegrationPolicy.decide(MigrationReadiness::validated(), &plan(0));
        let mut lease = ExecutionLease::new(decision);
        assert!(lease.fallback_to_host_before_execution().is_ok());
        assert_eq!(lease.decision().owner, ExecutionOwner::HostExisting);

        let mut committed = ExecutionLease::new(decision);
        committed.commit_runtime().unwrap();
        assert_eq!(
            committed.fallback_to_host_before_execution(),
            Err(FallbackError::AlreadyCommitted)
        );
    }

    #[test]
    fn host_selected_work_cannot_commit_runtime() {
        let decision = IntegrationPolicy.decide(MigrationReadiness::unsupported(), &plan(900));
        let mut lease = ExecutionLease::new(decision);
        assert_eq!(
            lease.commit_runtime(),
            Err(FallbackError::RuntimeNotSelected)
        );
        assert!(lease.commit_host().is_ok());
    }
}
