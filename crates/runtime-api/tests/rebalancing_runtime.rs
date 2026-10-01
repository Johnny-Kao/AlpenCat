use runtime_api::{BrokerCapacity, BrokerRequest, ExecutionPlan, Runtime, TaskDefinition};

#[test]
fn runtime_rebalancing_is_quiet_without_material_change() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("rebalance-stable");
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    let plan = runtime.adaptive_execution_plan(&task, 100_000, capacity, request);
    let mut session = runtime.begin_rebalancing(plan);

    for _ in 0..4 {
        let result = runtime.submit_range_task(
            &task,
            runtime_api::WorkRange::new(0, 1024),
            runtime_api::ExecutionMode::Cpu,
            runtime_api::RangeTaskImplementations::new(|i| i as u64),
        );
        let _ = runtime.wait(result.unwrap());
    }

    assert!(runtime
        .rebalance_if_needed(&mut session, &task, 90_000, capacity, request)
        .is_none());
}

#[test]
fn zero_remaining_work_never_replans() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("rebalance-done");
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };
    let plan: ExecutionPlan = runtime.adaptive_execution_plan(&task, 1_000, capacity, request);
    let mut session = runtime.begin_rebalancing(plan);

    assert!(runtime
        .rebalance_if_needed(&mut session, &task, 0, capacity, request)
        .is_none());
}
