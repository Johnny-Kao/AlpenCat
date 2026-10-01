use std::time::Duration;

use runtime_api::{BackendKind, BrokerCapacity, BrokerRequest, Runtime, TaskDefinition, WorkRange};

#[test]
fn adaptive_plan_requires_no_user_tuning() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("adaptive-plan");
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    let plan = runtime.adaptive_execution_plan(&task, 100_000, capacity, request);

    assert!(plan.cpu_parallelism >= 1);
    assert!(plan.chunk_size >= 1);
    assert!(plan.max_in_flight >= 1);
    assert_eq!(plan.backend_mix.gpu_weight, 0);
    assert_eq!(
        plan.backend_mix.serial_weight + plan.backend_mix.cpu_weight,
        runtime_api::BackendMix::TOTAL_WEIGHT
    );
}

#[test]
fn learned_costs_flow_into_execution_plan() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("learned-plan");
    let capacity = BrokerCapacity::new(8, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    for _ in 0..8 {
        runtime.record_cost_observation(
            &task,
            BackendKind::Serial,
            65_536,
            Duration::from_micros(800),
            true,
        );
        runtime.record_cost_observation(
            &task,
            BackendKind::Cpu,
            65_536,
            Duration::from_micros(200),
            true,
        );
    }

    let plan = runtime.adaptive_execution_plan(&task, 65_536, capacity, request);
    assert_eq!(plan.primary_backend, BackendKind::Cpu);
    assert!(plan.backend_mix.cpu_weight > plan.backend_mix.serial_weight);
    assert!(plan.confidence_milli > 500);
}

#[test]
fn adaptive_work_plan_covers_source_without_manual_chunk_size() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("adaptive-work-plan");
    let source = WorkRange::new(37, 100_037);
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    let (execution_plan, work_plan) = runtime.plan_adaptive_work(&task, source, capacity, request);

    assert_eq!(work_plan.total_items(), source.len());
    assert_eq!(work_plan.unit(0).unwrap().range.begin, source.begin);
    assert_eq!(
        work_plan
            .unit(work_plan.unit_count() - 1)
            .unwrap()
            .range
            .end,
        source.end
    );
    assert!(execution_plan.chunk_size >= 1);
    assert!(work_plan.unit_count() >= 1);
}
