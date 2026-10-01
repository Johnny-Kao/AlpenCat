use runtime_api::{BrokerCapacity, BrokerRequest, PolicyCacheStatus, Runtime, TaskDefinition};

#[test]
fn runtime_cached_policy_hits_until_resource_state_materially_changes() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("cached-policy-smoke");
    let capacity = BrokerCapacity::new(4, 1);
    let request = BrokerRequest {
        gpu_range_eligible: true,
    };

    let first = runtime
        .cached_execution_policy(&task, 1024, capacity, request)
        .expect("policy");
    let second = runtime
        .cached_execution_policy(&task, 1024, capacity, request)
        .expect("policy");

    assert_eq!(first.status, PolicyCacheStatus::Planned);
    assert_eq!(second.status, PolicyCacheStatus::Hit);
    assert_eq!(first.backend, second.backend);
    assert_eq!(runtime.cached_policy_count(), 1);
}

#[test]
fn runtime_can_explicitly_invalidate_task_policy() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("invalidate-me");
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    let _ = runtime.cached_execution_policy(&task, 128, capacity, request);
    let _ = runtime.cached_execution_policy(&task, 4096, capacity, request);
    assert_eq!(runtime.cached_policy_count(), 2);

    assert_eq!(runtime.invalidate_cached_policy(&task), 2);
    assert_eq!(runtime.cached_policy_count(), 0);
}
