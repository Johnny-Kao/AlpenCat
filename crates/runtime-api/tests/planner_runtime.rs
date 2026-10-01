use runtime_api::{
    BackendKind, BrokerCapacity, BrokerRequest, ChunkPolicy, Runtime, WorkQueue, WorkRange,
};

#[test]
fn runtime_plans_balanced_reassignable_work_units() {
    let runtime = Runtime::new();
    let source = WorkRange::new(100, 10_100);
    let plan = runtime.plan_work(source, ChunkPolicy::new(4096, 1024, 8192));
    let units: Vec<_> = plan.iter().collect();

    assert_eq!(plan.total_items(), source.len());
    assert_eq!(units.first().unwrap().range.begin, source.begin);
    assert_eq!(units.last().unwrap().range.end, source.end);

    for pair in units.windows(2) {
        assert_eq!(pair[0].range.end, pair[1].range.begin);
    }

    let mut queue = WorkQueue::from_plan(&plan);
    let first = queue.claim_next().expect("first unit must exist");
    queue.requeue_back(first);
    assert_eq!(queue.pending_count(), plan.unit_count());

    let assignment = runtime
        .claim_next_work(
            &mut queue,
            BrokerCapacity::new(4, 1),
            BrokerRequest {
                gpu_range_eligible: true,
            },
        )
        .expect("broker must claim work when capacity is available");

    assert_eq!(assignment.backend, BackendKind::Cpu);
    assert_eq!(queue.pending_count(), plan.unit_count() - 1);
}
