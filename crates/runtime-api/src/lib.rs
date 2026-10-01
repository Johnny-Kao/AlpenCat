//! Public API boundary for the experimental runtime framework.
//!
//! M7 adds explicit execution budgets and nested-Rayon protection while
//! preserving serial, CPU, GPU, selector, and calibration behavior.

use std::cell::Cell;
use std::sync::OnceLock;

use runtime_broker::ResourceBroker;
use runtime_cost_model::OnlineCostModel;
use runtime_cpu_rayon::{CpuAdapter, CpuExecutionKind};
use runtime_execution_planner::AdaptiveExecutionPlanner;
use runtime_gpu_wgpu::GpuAdapter;
use runtime_integration::IntegrationPolicy;
use runtime_planner::ChunkPlanner;
use runtime_policy_cache::ExecutionPolicyCache;
use runtime_rebalancer::RebalanceAction;
use runtime_telemetry::RuntimeTelemetry;

pub use runtime_broker::{BrokerCapacity, BrokerRequest, WorkAssignment};
pub use runtime_core::{BackendKind, ExecutionBudget, WorkRange};
pub use runtime_cost_model::{
    BackendCostEstimate, CostEstimateSource, CostModelContext, CostModelDecision, CostObservation,
    MachineFingerprint,
};
pub use runtime_execution_planner::{BackendMix, ExecutionPlan, PlannerContext, ResidencyHint};
pub use runtime_integration::{
    ExecutionLease, ExecutionOwner, FallbackError, IntegrationPolicy as MigrationPolicy,
    LeaseState, MigrationDecision, MigrationPlan, MigrationReadiness, MigrationReason,
};
pub use runtime_machine::{GpuDeviceProfile, GpuVendor, HostProfile, MachineProfile};
pub use runtime_planner::{ChunkPolicy, WorkPlan, WorkQueue, WorkUnit};
pub use runtime_policy_cache::{CachedExecutionPolicy, PolicyCacheStatus};
pub use runtime_rebalancer::{RebalancePolicy, RebalanceReason, RebalanceSession};
pub use runtime_selector::{CalibrationProfile, CPU_MAX_ITEMS, SERIAL_MAX_ITEMS};
pub use runtime_telemetry::{BackendTelemetrySnapshot, RuntimeTelemetrySnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Auto,
    Serial,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDefinition {
    pub id: &'static str,
    calibration: Option<CalibrationProfile>,
}

impl TaskDefinition {
    pub const fn new(id: &'static str) -> Self {
        Self {
            id,
            calibration: None,
        }
    }

    pub const fn with_calibration(mut self, calibration: CalibrationProfile) -> Self {
        self.calibration = Some(calibration);
        self
    }

    pub const fn calibration(&self) -> Option<CalibrationProfile> {
        self.calibration
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalParallelism {
    pub active: bool,
}

impl ExternalParallelism {
    pub const fn inactive() -> Self {
        Self { active: false }
    }

    pub const fn active() -> Self {
        Self { active: true }
    }
}

impl Default for ExternalParallelism {
    fn default() -> Self {
        Self::inactive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionConstraint {
    BudgetLimited,
    NestedParallelism,
    ExternalParallelism,
    ResourceLimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionDecision {
    pub backend: BackendKind,
    pub fallback_from: Option<BackendKind>,
    pub constraint: Option<ExecutionConstraint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecutionTrace {
    pub serial_units: u64,
    pub cpu_units: u64,
    pub gpu_units: u64,
    pub gpu_failures: u64,
    pub replans: u32,
}

impl ExecutionTrace {
    pub const fn used_backend(self, backend: BackendKind) -> bool {
        match backend {
            BackendKind::Serial => self.serial_units > 0,
            BackendKind::Cpu => self.cpu_units > 0,
            BackendKind::Gpu => self.gpu_units > 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    BackendUnavailable(BackendKind),
    BackendExecutionFailed(BackendKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebalanceUpdate {
    pub reason: RebalanceReason,
    pub plan: ExecutionPlan,
}

pub struct GpuExecutionContext<'a> {
    adapter: &'a GpuAdapter,
}

impl<'a> GpuExecutionContext<'a> {
    fn new(adapter: &'a GpuAdapter) -> Self {
        Self { adapter }
    }

    pub fn dispatch_f32(
        &self,
        shader_source: &str,
        input: &[f32],
        params: &[f32],
        workgroup_size: u32,
    ) -> Result<Vec<f32>, RuntimeError> {
        self.adapter
            .dispatch_f32(shader_source, input, params, workgroup_size)
            .map_err(|_| RuntimeError::BackendExecutionFailed(BackendKind::Gpu))
    }
}

type GpuRangeImplementation<'a, T> = dyn for<'gpu> Fn(&GpuExecutionContext<'gpu>, WorkRange) -> Result<Vec<T>, RuntimeError>
    + Send
    + Sync
    + 'a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuWorkGranularity {
    WholeTask,
    RangeAware,
}

pub struct RangeTaskImplementations<'a, T, F> {
    element: F,
    gpu: Option<Box<GpuRangeImplementation<'a, T>>>,
    gpu_granularity: Option<GpuWorkGranularity>,
}

impl<'a, T, F> RangeTaskImplementations<'a, T, F> {
    pub fn new(element: F) -> Self {
        Self {
            element,
            gpu: None,
            gpu_granularity: None,
        }
    }

    pub fn with_gpu<G>(mut self, gpu: G) -> Self
    where
        G: for<'gpu> Fn(&GpuExecutionContext<'gpu>) -> Result<Vec<T>, RuntimeError>
            + Send
            + Sync
            + 'a,
    {
        self.gpu = Some(Box::new(move |context, _range| gpu(context)));
        self.gpu_granularity = Some(GpuWorkGranularity::WholeTask);
        self
    }

    pub fn with_gpu_range<G>(mut self, gpu: G) -> Self
    where
        G: for<'gpu> Fn(&GpuExecutionContext<'gpu>, WorkRange) -> Result<Vec<T>, RuntimeError>
            + Send
            + Sync
            + 'a,
    {
        self.gpu = Some(Box::new(gpu));
        self.gpu_granularity = Some(GpuWorkGranularity::RangeAware);
        self
    }

    pub fn gpu_eligible(&self) -> bool {
        self.gpu.is_some()
    }

    pub const fn gpu_granularity(&self) -> Option<GpuWorkGranularity> {
        self.gpu_granularity
    }

    pub const fn gpu_range_eligible(&self) -> bool {
        matches!(self.gpu_granularity, Some(GpuWorkGranularity::RangeAware))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeConfig {
    pub calibration: CalibrationProfile,
    pub execution_budget: ExecutionBudget,
    pub external_parallelism: ExternalParallelism,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceSnapshot {
    pub host: HostProfile,
    pub telemetry: RuntimeTelemetrySnapshot,
}

#[derive(Debug)]
pub struct Runtime {
    cpu: CpuAdapter,
    gpu: OnceLock<GpuAdapter>,
    telemetry: RuntimeTelemetry,
    cost_model: OnlineCostModel,
    policy_cache: ExecutionPolicyCache,
    config: RuntimeConfig,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::with_config(RuntimeConfig::default())
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: RuntimeConfig) -> Self {
        Self {
            cpu: CpuAdapter::default(),
            gpu: OnceLock::new(),
            telemetry: RuntimeTelemetry::default(),
            cost_model: OnlineCostModel::default(),
            policy_cache: ExecutionPolicyCache::default(),
            config,
        }
    }

    pub const fn config(&self) -> RuntimeConfig {
        self.config
    }

    pub const fn calibration(&self) -> CalibrationProfile {
        self.config.calibration
    }

    pub const fn execution_budget(&self) -> ExecutionBudget {
        self.config.execution_budget
    }

    pub const fn external_parallelism(&self) -> ExternalParallelism {
        self.config.external_parallelism
    }

    pub fn host_profile(&self) -> HostProfile {
        HostProfile::discover()
    }

    pub fn discover_machine_profile(&self) -> MachineProfile {
        let mut profile = MachineProfile::host_only();

        if let Ok(gpu) = self.gpu_adapter() {
            let info = gpu.device_info();
            profile.gpus.push(GpuDeviceProfile {
                name: info.name.clone(),
                backend: info.backend.clone(),
                device_type: info.device_type.clone(),
                vendor_id: Some(info.vendor_id),
                device_id: Some(info.device_id),
                dedicated_memory_bytes: None,
            });
        }

        profile
    }

    pub fn telemetry_snapshot(&self) -> RuntimeTelemetrySnapshot {
        self.telemetry.snapshot()
    }

    pub fn resource_snapshot(&self) -> ResourceSnapshot {
        ResourceSnapshot {
            host: HostProfile::discover(),
            telemetry: self.telemetry.snapshot(),
        }
    }

    pub fn plan_work(&self, range: WorkRange, policy: ChunkPolicy) -> WorkPlan {
        ChunkPlanner.plan(range, policy)
    }

    pub fn claim_next_work(
        &self,
        queue: &mut WorkQueue,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Option<WorkAssignment> {
        ResourceBroker.claim_next(queue, self.telemetry.snapshot(), capacity, request)
    }

    pub fn adaptive_execution_plan(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> ExecutionPlan {
        self.adaptive_execution_plan_with_preference(task, work_items, capacity, request, None)
    }

    fn adaptive_execution_plan_with_preference(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
        preferred_backend: Option<BackendKind>,
    ) -> ExecutionPlan {
        let telemetry = self.telemetry.snapshot();
        let gpu_eligible = request.gpu_range_eligible && capacity.gpu_slots > 0;
        let machine = if gpu_eligible {
            self.discover_machine_profile()
        } else {
            MachineProfile::host_only()
        };
        let cost = self.cost_model.decide_with_preference(
            task.id,
            work_items,
            CostModelContext {
                cpu_eligible: capacity.cpu_slots > 0,
                gpu_eligible,
                machine: &machine,
                telemetry,
                bootstrap: self.calibration_for(task),
            },
            preferred_backend,
        );

        AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items,
            cost,
            machine: &machine,
            telemetry,
            gpu_range_eligible: gpu_eligible,
        })
    }

    fn cached_adaptive_execution_plan(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> ExecutionPlan {
        let telemetry = self.telemetry.snapshot();
        let gpu_eligible = request.gpu_range_eligible && capacity.gpu_slots > 0;
        let machine = if gpu_eligible {
            self.discover_machine_profile()
        } else {
            MachineProfile::host_only()
        };

        let mut miss_plan = None;
        let cached = self.policy_cache.resolve_with(
            task.id,
            work_items,
            telemetry,
            capacity,
            request,
            || {
                let cost = self.cost_model.decide(
                    task.id,
                    work_items,
                    CostModelContext {
                        cpu_eligible: capacity.cpu_slots > 0,
                        gpu_eligible,
                        machine: &machine,
                        telemetry,
                        bootstrap: self.calibration_for(task),
                    },
                );
                let plan = AdaptiveExecutionPlanner.plan(PlannerContext {
                    work_items,
                    cost,
                    machine: &machine,
                    telemetry,
                    gpu_range_eligible: gpu_eligible,
                });
                let backend = plan.primary_backend;
                miss_plan = Some(plan);
                Some(backend)
            },
        );

        if let Some(plan) = miss_plan {
            return plan;
        }

        let backend = cached
            .map(|policy| policy.backend)
            .unwrap_or(BackendKind::Serial);
        AdaptiveExecutionPlanner.plan(PlannerContext {
            work_items,
            cost: CostModelDecision {
                backend,
                confidence: 0.0,
                serial: None,
                cpu: None,
                gpu: None,
            },
            machine: &machine,
            telemetry,
            gpu_range_eligible: gpu_eligible,
        })
    }

    pub fn migration_plan(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
        readiness: MigrationReadiness,
    ) -> MigrationPlan {
        let execution_plan = self.adaptive_execution_plan(task, work_items, capacity, request);
        let decision = IntegrationPolicy.decide(readiness, &execution_plan);

        MigrationPlan {
            execution_plan,
            decision,
        }
    }

    pub fn begin_rebalancing(&self, plan: ExecutionPlan) -> RebalanceSession {
        RebalanceSession::new(plan, self.telemetry.snapshot())
    }

    pub fn rebalance_if_needed(
        &self,
        session: &mut RebalanceSession,
        task: &TaskDefinition,
        remaining_work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Option<RebalanceUpdate> {
        if remaining_work_items == 0 {
            return None;
        }

        let snapshot = self.telemetry.snapshot();
        match session.consider(snapshot, capacity.cpu_slots, capacity.gpu_slots) {
            RebalanceAction::Keep => None,
            RebalanceAction::Replan(reason) => {
                self.policy_cache.invalidate_task(task.id);
                let preferred_backend = Some(session.plan().primary_backend);
                let plan = self.adaptive_execution_plan_with_preference(
                    task,
                    remaining_work_items,
                    capacity,
                    request,
                    preferred_backend,
                );
                session.replace_plan(plan.clone(), snapshot);
                Some(RebalanceUpdate { reason, plan })
            }
        }
    }

    pub fn plan_adaptive_work(
        &self,
        task: &TaskDefinition,
        range: WorkRange,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> (ExecutionPlan, WorkPlan) {
        let execution_plan = self.adaptive_execution_plan(task, range.len(), capacity, request);
        let work_plan = self.plan_work(range, ChunkPolicy::fixed(execution_plan.chunk_size.max(1)));
        (execution_plan, work_plan)
    }

    pub fn cost_model_decision(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> CostModelDecision {
        let telemetry = self.telemetry.snapshot();
        let gpu_eligible = request.gpu_range_eligible && capacity.gpu_slots > 0;
        let machine = if gpu_eligible {
            self.discover_machine_profile()
        } else {
            MachineProfile::host_only()
        };

        self.cost_model.decide(
            task.id,
            work_items,
            CostModelContext {
                cpu_eligible: capacity.cpu_slots > 0,
                gpu_eligible,
                machine: &machine,
                telemetry,
                bootstrap: self.calibration_for(task),
            },
        )
    }

    pub fn cached_execution_policy(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        capacity: BrokerCapacity,
        request: BrokerRequest,
    ) -> Option<CachedExecutionPolicy> {
        let telemetry = self.telemetry.snapshot();
        let gpu_eligible = request.gpu_range_eligible && capacity.gpu_slots > 0;
        let machine = if gpu_eligible {
            self.discover_machine_profile()
        } else {
            MachineProfile::host_only()
        };

        self.policy_cache
            .resolve_with(task.id, work_items, telemetry, capacity, request, || {
                Some(
                    self.cost_model
                        .decide(
                            task.id,
                            work_items,
                            CostModelContext {
                                cpu_eligible: capacity.cpu_slots > 0,
                                gpu_eligible,
                                machine: &machine,
                                telemetry,
                                bootstrap: self.calibration_for(task),
                            },
                        )
                        .backend,
                )
            })
    }

    pub fn record_cost_observation(
        &self,
        task: &TaskDefinition,
        backend: BackendKind,
        work_items: usize,
        elapsed: std::time::Duration,
        success: bool,
    ) {
        self.cost_model
            .observe(task.id, backend, work_items, elapsed, success);
    }

    pub fn record_detailed_cost_observation(
        &self,
        task: &TaskDefinition,
        backend: BackendKind,
        machine: &MachineProfile,
        observation: CostObservation,
    ) {
        self.cost_model.observe_detailed(
            task.id,
            backend,
            MachineFingerprint::from_machine(machine),
            observation,
        );
    }

    pub fn invalidate_cached_policy(&self, task: &TaskDefinition) -> usize {
        self.policy_cache.invalidate_task(task.id)
    }

    pub fn reset_task_learning(&self, task: &TaskDefinition) -> usize {
        let removed = self.cost_model.clear_task(task.id);
        self.policy_cache.invalidate_task(task.id);
        removed
    }

    pub fn clear_cached_policies(&self) {
        self.policy_cache.clear();
    }

    pub fn cached_policy_count(&self) -> usize {
        self.policy_cache.len()
    }

    fn calibration_for(&self, task: &TaskDefinition) -> CalibrationProfile {
        task.calibration().unwrap_or(self.config.calibration)
    }

    fn automatic_backend_with_machine(
        &self,
        task: &TaskDefinition,
        work_items: usize,
        gpu_eligible: bool,
        machine: &MachineProfile,
    ) -> BackendKind {
        let telemetry = self.telemetry.snapshot();

        self.cost_model
            .decide(
                task.id,
                work_items,
                CostModelContext {
                    cpu_eligible: machine.host.logical_cpus > 1,
                    gpu_eligible,
                    machine,
                    telemetry,
                    bootstrap: self.calibration_for(task),
                },
            )
            .backend
    }

    fn execute_serial_map<T, F>(&self, range: WorkRange, operation: F) -> Vec<T>
    where
        F: Fn(usize) -> T,
    {
        let observation = self.telemetry.begin(BackendKind::Serial, range.len());
        let values = (range.begin..range.end).map(operation).collect();
        observation.success();
        values
    }

    fn execute_cpu<T, F>(
        &self,
        range: WorkRange,
        operation: F,
    ) -> (BackendKind, Option<ExecutionConstraint>, Vec<T>)
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        self.execute_cpu_with_budget(range, self.config.execution_budget, operation)
    }

    fn execute_cpu_with_budget<T, F>(
        &self,
        range: WorkRange,
        budget: ExecutionBudget,
        operation: F,
    ) -> (BackendKind, Option<ExecutionConstraint>, Vec<T>)
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        if self.config.external_parallelism.active {
            return (
                BackendKind::Serial,
                Some(ExecutionConstraint::ExternalParallelism),
                self.execute_serial_map(range, operation),
            );
        }

        let expected_backend = if self.cpu.effective_parallelism(budget) <= 1
            || self.cpu.in_rayon_parallel_context()
        {
            BackendKind::Serial
        } else {
            BackendKind::Cpu
        };
        let observation = self.telemetry.begin(expected_backend, range.len());
        let execution = self.cpu.map(range, budget, operation);
        observation.success();

        match execution.kind {
            CpuExecutionKind::Parallel => (BackendKind::Cpu, None, execution.values),
            CpuExecutionKind::ParallelBudgetLimited => (
                BackendKind::Cpu,
                Some(ExecutionConstraint::BudgetLimited),
                execution.values,
            ),
            CpuExecutionKind::SerialBudget => (
                BackendKind::Serial,
                Some(ExecutionConstraint::BudgetLimited),
                execution.values,
            ),
            CpuExecutionKind::SerialNested => (
                BackendKind::Serial,
                Some(ExecutionConstraint::NestedParallelism),
                execution.values,
            ),
            CpuExecutionKind::SerialResourceLimited => (
                BackendKind::Serial,
                Some(ExecutionConstraint::ResourceLimited),
                execution.values,
            ),
        }
    }

    pub fn submit<T, F>(
        &self,
        task: &TaskDefinition,
        range: WorkRange,
        mode: ExecutionMode,
        serial_impl: F,
    ) -> Result<TaskHandle<T>, RuntimeError>
    where
        F: FnOnce(WorkRange) -> T,
    {
        let backend = match mode {
            ExecutionMode::Auto | ExecutionMode::Serial => BackendKind::Serial,
            ExecutionMode::Cpu => return Err(RuntimeError::BackendUnavailable(BackendKind::Cpu)),
            ExecutionMode::Gpu => return Err(RuntimeError::BackendUnavailable(BackendKind::Gpu)),
        };

        let observation = self.telemetry.begin(BackendKind::Serial, range.len());
        let result = serial_impl(range);
        observation.success();

        Ok(TaskHandle {
            task_id: task.id,
            decision: ExecutionDecision {
                backend,
                fallback_from: None,
                constraint: None,
            },
            trace: ExecutionTrace {
                serial_units: 1,
                ..ExecutionTrace::default()
            },
            result,
        })
    }

    pub fn submit_range_task<'a, T, F>(
        &self,
        task: &TaskDefinition,
        range: WorkRange,
        mode: ExecutionMode,
        implementations: RangeTaskImplementations<'a, T, F>,
    ) -> Result<TaskHandle<Vec<T>>, RuntimeError>
    where
        T: Send + 'a,
        F: Fn(usize) -> T + Sync + Send + 'a,
    {
        let gpu_eligible = implementations.gpu_eligible();
        let element = &implementations.element;
        let observed_machine = if gpu_eligible {
            self.discover_machine_profile()
        } else {
            MachineProfile::host_only()
        };
        let machine_fingerprint = MachineFingerprint::from_machine(&observed_machine);
        let serial_units = Cell::new(0u64);
        let cpu_units = Cell::new(0u64);
        let gpu_units = Cell::new(0u64);
        let gpu_failures = Cell::new(0u64);
        let replans = Cell::new(0u32);

        let execute_serial = |work: WorkRange| {
            serial_units.set(serial_units.get().saturating_add(1));
            let started = std::time::Instant::now();
            let values = self.execute_serial_map(work, element);
            self.cost_model.observe_detailed(
                task.id,
                BackendKind::Serial,
                machine_fingerprint,
                CostObservation::execution(work.len(), started.elapsed(), true),
            );
            values
        };

        let execute_cpu = |work: WorkRange, budget: ExecutionBudget| {
            let started = std::time::Instant::now();
            let (backend, constraint, values) = self.execute_cpu_with_budget(work, budget, element);
            match backend {
                BackendKind::Serial => {
                    serial_units.set(serial_units.get().saturating_add(1));
                }
                BackendKind::Cpu => {
                    cpu_units.set(cpu_units.get().saturating_add(1));
                }
                BackendKind::Gpu => unreachable!("CPU adapter cannot return GPU execution"),
            }
            self.cost_model.observe_detailed(
                task.id,
                backend,
                machine_fingerprint,
                CostObservation::execution(work.len(), started.elapsed(), true),
            );
            (backend, constraint, values)
        };

        let execute_gpu = |work: WorkRange| -> Result<Vec<T>, RuntimeError> {
            gpu_units.set(gpu_units.get().saturating_add(1));
            let started = std::time::Instant::now();
            let observation = self.telemetry.begin(BackendKind::Gpu, work.len());
            let result = (|| {
                let gpu_impl = implementations
                    .gpu
                    .as_ref()
                    .ok_or(RuntimeError::BackendUnavailable(BackendKind::Gpu))?;
                let adapter = self.gpu_adapter()?;
                let context = GpuExecutionContext::new(adapter);
                gpu_impl(&context, work)
            })();

            let success = result.is_ok();
            if success {
                observation.success();
            } else {
                gpu_failures.set(gpu_failures.get().saturating_add(1));
                observation.failure();
            }
            self.cost_model.observe_detailed(
                task.id,
                BackendKind::Gpu,
                machine_fingerprint,
                CostObservation::execution(work.len(), started.elapsed(), success),
            );
            result
        };

        let (backend, fallback_from, constraint, result) = match mode {
            ExecutionMode::Auto
                if implementations.gpu_eligible() && !implementations.gpu_range_eligible() =>
            {
                // Whole-task-only GPU registrations cannot participate in M10/M11
                // chunk reassignment. Preserve the pre-integration Auto path.
                match self.automatic_backend_with_machine(
                    task,
                    range.len(),
                    gpu_eligible,
                    &observed_machine,
                ) {
                    BackendKind::Serial => (BackendKind::Serial, None, None, execute_serial(range)),
                    BackendKind::Cpu => {
                        let (backend, constraint, values) =
                            execute_cpu(range, self.config.execution_budget);
                        (backend, None, constraint, values)
                    }
                    BackendKind::Gpu => match execute_gpu(range) {
                        Ok(values) => (BackendKind::Gpu, None, None, values),
                        Err(_) => {
                            let (backend, constraint, values) =
                                execute_cpu(range, self.config.execution_budget);
                            (backend, Some(BackendKind::Gpu), constraint, values)
                        }
                    },
                }
            }
            ExecutionMode::Auto if !implementations.gpu_range_eligible() => {
                // CPU/serial-only work has no second range-aware backend to
                // rebalance toward. Run M11.5/M12/M13/M15, but avoid splitting
                // one Rayon operation into many sequential Rayon invocations.
                let request = BrokerRequest {
                    gpu_range_eligible: false,
                };
                let capacity = BrokerCapacity::new(
                    self.cpu.effective_parallelism(self.config.execution_budget),
                    0,
                );
                let plan =
                    self.cached_adaptive_execution_plan(task, range.len(), capacity, request);

                match plan.primary_backend {
                    BackendKind::Serial => {
                        (BackendKind::Serial, None, None, execute_serial(range))
                    }
                    BackendKind::Cpu => {
                        let planned_parallelism = plan
                            .cpu_parallelism
                            .min(self.config.execution_budget.max_parallelism)
                            .max(1);
                        let (backend, constraint, values) =
                            execute_cpu(range, ExecutionBudget::new(planned_parallelism));
                        (backend, None, constraint, values)
                    }
                    BackendKind::Gpu => unreachable!(
                        "GPU cannot be selected when no range-aware GPU implementation is registered"
                    ),
                }
            }
            ExecutionMode::Auto => {
                let request = BrokerRequest {
                    gpu_range_eligible: implementations.gpu_range_eligible(),
                };
                let base_capacity = BrokerCapacity::new(
                    self.cpu.effective_parallelism(self.config.execution_budget),
                    usize::from(request.gpu_range_eligible && !observed_machine.gpus.is_empty()),
                );

                // M11.5 supplies a stable preferred route. M12/M13 still produce
                // the complete execution plan; M14 invalidates this cache when a
                // material event requires replanning.
                let mut plan =
                    self.cached_adaptive_execution_plan(task, range.len(), base_capacity, request);

                if plan.primary_backend == BackendKind::Serial {
                    (BackendKind::Serial, None, None, execute_serial(range))
                } else {
                    let work_plan =
                        self.plan_work(range, ChunkPolicy::fixed(plan.chunk_size.max(1)));
                    let mut queue = WorkQueue::from_plan(&work_plan);
                    let mut rebalance = self.begin_rebalancing(plan.clone());
                    let mut values = Vec::with_capacity(range.len());
                    let mut completed_items = 0usize;
                    let mut last_backend = plan.primary_backend;
                    let mut fallback_from = None;
                    let mut constraint = None;

                    while !queue.is_empty() {
                        // M13 owns the chosen route and M11 owns pending-work
                        // claiming. Until execution becomes asynchronous, expose
                        // only the current primary backend to the broker.
                        let broker_capacity = match plan.primary_backend {
                            BackendKind::Serial => BrokerCapacity::new(0, 0),
                            BackendKind::Cpu => BrokerCapacity::new(
                                plan.cpu_parallelism
                                    .min(plan.max_in_flight)
                                    .min(base_capacity.cpu_slots)
                                    .max(1),
                                0,
                            ),
                            BackendKind::Gpu => BrokerCapacity::new(
                                0,
                                base_capacity.gpu_slots.min(plan.max_in_flight),
                            ),
                        };

                        let assignment = self.claim_next_work(&mut queue, broker_capacity, request);

                        let unit = if let Some(assignment) = assignment {
                            let unit = assignment.unit;
                            match assignment.backend {
                                BackendKind::Cpu => {
                                    let planned_parallelism = plan
                                        .cpu_parallelism
                                        .min(plan.max_in_flight)
                                        .min(self.config.execution_budget.max_parallelism)
                                        .max(1);
                                    let (actual_backend, current_constraint, chunk) = execute_cpu(
                                        unit.range,
                                        ExecutionBudget::new(planned_parallelism),
                                    );
                                    last_backend = actual_backend;
                                    if constraint.is_none() {
                                        constraint = current_constraint;
                                    }
                                    values.extend(chunk);
                                }
                                BackendKind::Gpu => match execute_gpu(unit.range) {
                                    Ok(chunk) => {
                                        last_backend = BackendKind::Gpu;
                                        values.extend(chunk);
                                    }
                                    Err(_) => {
                                        fallback_from = Some(BackendKind::Gpu);
                                        let planned_parallelism = plan
                                            .cpu_parallelism
                                            .min(plan.max_in_flight)
                                            .min(self.config.execution_budget.max_parallelism)
                                            .max(1);
                                        let (actual_backend, current_constraint, chunk) =
                                            execute_cpu(
                                                unit.range,
                                                ExecutionBudget::new(planned_parallelism),
                                            );
                                        last_backend = actual_backend;
                                        if constraint.is_none() {
                                            constraint = current_constraint;
                                        }
                                        values.extend(chunk);
                                    }
                                },
                                BackendKind::Serial => unreachable!(
                                    "M11 broker only assigns CPU/GPU work in the adaptive path"
                                ),
                            }
                            unit
                        } else {
                            // No currently admissible broker capacity: preserve
                            // correctness with the serial runtime-owned fallback.
                            let unit = queue
                                .claim_next()
                                .expect("non-empty adaptive queue must yield a WorkUnit");
                            values.extend(execute_serial(unit.range));
                            last_backend = BackendKind::Serial;
                            if constraint.is_none() {
                                constraint = Some(ExecutionConstraint::ResourceLimited);
                            }
                            unit
                        };

                        completed_items = completed_items.saturating_add(unit.len());
                        let remaining = range.len().saturating_sub(completed_items);
                        if let Some(update) = self.rebalance_if_needed(
                            &mut rebalance,
                            task,
                            remaining,
                            base_capacity,
                            request,
                        ) {
                            replans.set(replans.get().saturating_add(1));
                            plan = update.plan;
                        }
                    }

                    (last_backend, fallback_from, constraint, values)
                }
            }
            ExecutionMode::Serial => (BackendKind::Serial, None, None, execute_serial(range)),
            ExecutionMode::Cpu => {
                let (backend, constraint, values) =
                    execute_cpu(range, self.config.execution_budget);
                (backend, None, constraint, values)
            }
            ExecutionMode::Gpu => (BackendKind::Gpu, None, None, execute_gpu(range)?),
        };

        Ok(TaskHandle {
            task_id: task.id,
            decision: ExecutionDecision {
                backend,
                fallback_from,
                constraint,
            },
            trace: ExecutionTrace {
                serial_units: serial_units.get(),
                cpu_units: cpu_units.get(),
                gpu_units: gpu_units.get(),
                gpu_failures: gpu_failures.get(),
                replans: replans.get(),
            },
            result,
        })
    }

    pub fn submit_map<T, F>(
        &self,
        task: &TaskDefinition,
        range: WorkRange,
        mode: ExecutionMode,
        operation: F,
    ) -> Result<TaskHandle<Vec<T>>, RuntimeError>
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        self.submit_range_task(task, range, mode, RangeTaskImplementations::new(operation))
    }

    fn gpu_adapter(&self) -> Result<&GpuAdapter, RuntimeError> {
        if let Some(gpu) = self.gpu.get() {
            return Ok(gpu);
        }

        let gpu =
            GpuAdapter::new().map_err(|_| RuntimeError::BackendUnavailable(BackendKind::Gpu))?;

        let _ = self.gpu.set(gpu);

        self.gpu
            .get()
            .ok_or(RuntimeError::BackendUnavailable(BackendKind::Gpu))
    }

    pub fn wait<T>(&self, handle: TaskHandle<T>) -> TaskResult<T> {
        TaskResult {
            task_id: handle.task_id,
            decision: handle.decision,
            trace: handle.trace,
            value: handle.result,
        }
    }
}

#[derive(Debug)]
pub struct TaskHandle<T> {
    task_id: &'static str,
    decision: ExecutionDecision,
    trace: ExecutionTrace,
    result: T,
}

impl<T> TaskHandle<T> {
    pub const fn decision(&self) -> ExecutionDecision {
        self.decision
    }

    pub const fn trace(&self) -> ExecutionTrace {
        self.trace
    }
}

#[derive(Debug)]
pub struct TaskResult<T> {
    pub task_id: &'static str,
    pub decision: ExecutionDecision,
    pub trace: ExecutionTrace,
    pub value: T,
}
