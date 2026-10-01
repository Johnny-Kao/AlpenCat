use std::time::Duration;

use runtime_api::{
    BackendKind, BrokerCapacity, BrokerRequest, ExecutionMode, ExecutionOwner, MigrationReadiness,
    PolicyCacheStatus, RangeTaskImplementations, Runtime, TaskDefinition, WorkQueue, WorkRange,
};

#[test]
fn one_task_can_manually_traverse_m8_to_m15_components() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("v01-pipeline-audit");
    let range = WorkRange::new(0, 65_536);
    let host = runtime.host_profile();
    assert!(host.logical_cpus >= 1);

    let capacity = BrokerCapacity::new(host.logical_cpus.max(2), 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    // M12: establish deterministic learned economics for this task.
    for _ in 0..8 {
        runtime.record_cost_observation(
            &task,
            BackendKind::Serial,
            range.len(),
            Duration::from_micros(800),
            true,
        );
        runtime.record_cost_observation(
            &task,
            BackendKind::Cpu,
            range.len(),
            Duration::from_micros(200),
            true,
        );
    }

    // M11.5: policy cache should plan then hit.
    let first_policy = runtime
        .cached_execution_policy(&task, range.len(), capacity, request)
        .expect("policy must resolve");
    let second_policy = runtime
        .cached_execution_policy(&task, range.len(), capacity, request)
        .expect("policy must hit");
    assert_eq!(first_policy.status, PolicyCacheStatus::Planned);
    assert_eq!(second_policy.status, PolicyCacheStatus::Hit);

    // M13 + M10: adaptive plan creates a chunked work plan.
    let (plan, work_plan) = runtime.plan_adaptive_work(&task, range, capacity, request);
    assert_eq!(plan.primary_backend, BackendKind::Cpu);
    assert_eq!(work_plan.total_items(), range.len());
    assert!(work_plan.unit_count() >= 1);

    // M11: broker can claim a planned work unit.
    let mut queue = WorkQueue::from_plan(&work_plan);
    let assignment = runtime
        .claim_next_work(&mut queue, capacity, request)
        .expect("broker must claim work");
    assert_eq!(assignment.backend, BackendKind::Cpu);

    // M14: establish a rebalance session on the same plan. Stable work should
    // remain quiet rather than spuriously replanning.
    let mut session = runtime.begin_rebalancing(plan.clone());

    let handle = runtime
        .submit_range_task(
            &task,
            WorkRange::new(0, 4096),
            ExecutionMode::Cpu,
            RangeTaskImplementations::new(|i| i as u64),
        )
        .expect("CPU execution must succeed");
    let result = runtime.wait(handle);
    assert_eq!(result.decision.backend, BackendKind::Cpu);

    let _ = runtime.rebalance_if_needed(
        &mut session,
        &task,
        range.len() - 4096,
        capacity,
        request,
    );

    // M15: validated work can be assigned to runtime ownership.
    let migration = runtime.migration_plan(
        &task,
        range.len(),
        capacity,
        request,
        MigrationReadiness::validated(),
    );
    assert_eq!(migration.decision.owner, ExecutionOwner::Runtime);
}

#[test]
fn canonical_auto_submission_executes_correctly_but_does_not_require_manual_planning() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("v01-canonical-auto");
    let range = WorkRange::new(0, 32_768);

    for _ in 0..8 {
        runtime.record_cost_observation(
            &task,
            BackendKind::Serial,
            range.len(),
            Duration::from_micros(600),
            true,
        );
        runtime.record_cost_observation(
            &task,
            BackendKind::Cpu,
            range.len(),
            Duration::from_micros(150),
            true,
        );
    }

    let handle = runtime
        .submit_range_task(
            &task,
            range,
            ExecutionMode::Auto,
            RangeTaskImplementations::new(|i| i * 2),
        )
        .expect("Auto execution must succeed");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    let result = runtime.wait(handle);
    assert_eq!(result.value.len(), range.len());
    assert_eq!(result.value[1234], 2468);

    // This assertion captures the current architectural fact: the canonical
    // submission path succeeds without materializing M10/M11/M13/M14/M15
    // objects. Those APIs currently require explicit orchestration.
    assert_eq!(runtime.cached_policy_count(), 0);
}
