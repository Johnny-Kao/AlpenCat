use rayon::ThreadPoolBuilder;
use runtime_api::{
    BackendKind, ExecutionBudget, ExecutionConstraint, ExecutionMode, Runtime, RuntimeConfig,
    TaskDefinition, WorkRange,
};

#[test]
fn execution_budget_one_forces_serial_cpu_execution() {
    let runtime = Runtime::with_config(RuntimeConfig {
        execution_budget: ExecutionBudget::serial(),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, 16_384),
            ExecutionMode::Cpu,
            |index| index,
        )
        .expect("budget-limited CPU request must execute safely");

    assert_eq!(handle.decision().backend, BackendKind::Serial);
    assert_eq!(
        handle.decision().constraint,
        Some(ExecutionConstraint::BudgetLimited)
    );
    assert_eq!(runtime.execution_budget().max_parallelism, 1);
}

#[test]
fn nested_rayon_context_forces_serial_cpu_execution() {
    let pool = ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("test Rayon pool must build");

    pool.install(|| {
        let runtime = Runtime::with_config(RuntimeConfig {
            execution_budget: ExecutionBudget::new(4),
            ..RuntimeConfig::default()
        });
        let task = TaskDefinition::new("map");

        let handle = runtime
            .submit_map(
                &task,
                WorkRange::new(0, 16_384),
                ExecutionMode::Cpu,
                |index| index,
            )
            .expect("nested CPU request must execute safely");

        assert_eq!(handle.decision().backend, BackendKind::Serial);
        assert_eq!(
            handle.decision().constraint,
            Some(ExecutionConstraint::NestedParallelism)
        );
    });
}

#[test]
fn non_nested_cpu_execution_uses_parallel_backend_with_budget() {
    let runtime = Runtime::with_config(RuntimeConfig {
        execution_budget: ExecutionBudget::new(2),
        ..RuntimeConfig::default()
    });
    let task = TaskDefinition::new("map");

    let handle = runtime
        .submit_map(
            &task,
            WorkRange::new(0, 16_384),
            ExecutionMode::Cpu,
            |index| index,
        )
        .expect("CPU request must execute");

    assert_eq!(handle.decision().backend, BackendKind::Cpu);
    assert_eq!(handle.decision().constraint, None);
}

#[test]
fn zero_parallelism_budget_normalizes_to_one() {
    let budget = ExecutionBudget::new(0);

    assert_eq!(budget.max_parallelism, 1);
}
