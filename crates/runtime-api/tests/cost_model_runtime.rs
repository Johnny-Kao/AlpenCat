use std::time::Duration;

use runtime_api::{BackendKind, BrokerCapacity, BrokerRequest, Runtime, TaskDefinition};

#[test]
fn runtime_cost_model_learns_without_user_thresholds() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("learned-backend");
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    for _ in 0..8 {
        runtime.record_cost_observation(
            &task,
            BackendKind::Serial,
            4096,
            Duration::from_micros(200),
            true,
        );
        runtime.record_cost_observation(
            &task,
            BackendKind::Cpu,
            4096,
            Duration::from_micros(100),
            true,
        );
    }

    let learned = runtime.cost_model_decision(&task, 4096, capacity, request);
    assert_eq!(learned.backend, BackendKind::Cpu);
    assert!(learned.confidence > 0.5);

    for _ in 0..8 {
        runtime.record_cost_observation(
            &task,
            BackendKind::Cpu,
            4096,
            Duration::from_micros(100),
            false,
        );
    }

    let degraded = runtime.cost_model_decision(&task, 4096, capacity, request);
    assert_eq!(degraded.backend, BackendKind::Serial);
}

#[test]
fn resetting_task_learning_returns_to_bootstrap() {
    let runtime = Runtime::new();
    let task = TaskDefinition::new("reset-learning");
    let capacity = BrokerCapacity::new(4, 0);
    let request = BrokerRequest {
        gpu_range_eligible: false,
    };

    for _ in 0..8 {
        runtime.record_cost_observation(
            &task,
            BackendKind::Cpu,
            4096,
            Duration::from_micros(10),
            true,
        );
    }

    assert!(runtime.reset_task_learning(&task) > 0);
    let decision = runtime.cost_model_decision(&task, 4096, capacity, request);
    assert_eq!(decision.confidence, 0.0);
}
