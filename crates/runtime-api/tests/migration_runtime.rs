use runtime_api::{
    BrokerCapacity, BrokerRequest, ExecutionLease, ExecutionOwner, FallbackError,
    MigrationReadiness, Runtime, TaskDefinition,
};

#[test]
fn unsupported_work_stays_on_host_existing() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("unsupported");
    let plan = runtime.migration_plan(
        &task,
        10_000,
        BrokerCapacity::new(4, 0),
        BrokerRequest {
            gpu_range_eligible: false,
        },
        MigrationReadiness::unsupported(),
    );

    assert_eq!(plan.decision.owner, ExecutionOwner::HostExisting);
}

#[test]
fn execution_lease_blocks_post_commit_fallback() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("validated");
    let migration = runtime.migration_plan(
        &task,
        65_536,
        BrokerCapacity::new(4, 0),
        BrokerRequest {
            gpu_range_eligible: false,
        },
        MigrationReadiness::validated(),
    );

    assert_eq!(migration.decision.owner, ExecutionOwner::Runtime);
    let mut lease = ExecutionLease::new(migration.decision);
    lease.commit_runtime().unwrap();
    assert_eq!(
        lease.fallback_to_host_before_execution(),
        Err(FallbackError::AlreadyCommitted)
    );
}
