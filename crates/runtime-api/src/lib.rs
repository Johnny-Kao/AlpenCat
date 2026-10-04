//! Public API for the converged AlpenCat runtime.
//!
//! The active control path is intentionally narrow:
//!
//! published Boundary -> FastRoute -> backend
//! native event -> ResourceEpoch++ -> boundary becomes stale
//!
//! Revalidation is explicit and bounded; AlpenCat no longer owns continuous
//! telemetry, an online cost model, a resource broker, or a global planner.

use std::sync::OnceLock;

use runtime_cpu_rayon::{CpuAdapter, CpuExecutionKind};
use runtime_gpu_wgpu::GpuAdapter;
use runtime_selector::select;

pub use runtime_core::{
    BackendKind, BoundaryProfile, BoundarySnapshot, ExecutionBudget, PublishedBoundary, ResourceEpoch, WorkRange,
};
pub use runtime_machine::{GpuDeviceProfile, GpuVendor, HostProfile, MachineProfile};
pub use runtime_selector::{CPU_MAX_ITEMS, SERIAL_MAX_ITEMS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Auto,
    Serial,
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDefinition {
    pub id: &'static str,
}

impl TaskDefinition {
    pub const fn new(id: &'static str) -> Self {
        Self { id }
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
    pub boundary_stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    BackendUnavailable(BackendKind),
    BackendExecutionFailed(BackendKind),
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

type GpuRangeImplementation<'a, T> = dyn for<'gpu> Fn(
        &GpuExecutionContext<'gpu>,
        WorkRange,
    ) -> Result<Vec<T>, RuntimeError>
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
        G: for<'gpu> Fn(
                &GpuExecutionContext<'gpu>,
                WorkRange,
            ) -> Result<Vec<T>, RuntimeError>
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub boundary: BoundaryProfile,
    pub execution_budget: ExecutionBudget,
    pub external_parallelism: ExternalParallelism,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            boundary: BoundaryProfile::default(),
            execution_budget: ExecutionBudget::default(),
            external_parallelism: ExternalParallelism::default(),
        }
    }
}

#[derive(Debug)]
pub struct TaskHandle<T> {
    task_id: &'static str,
    decision: ExecutionDecision,
    value: T,
}

impl<T> TaskHandle<T> {
    pub const fn decision(&self) -> ExecutionDecision {
        self.decision
    }
}

#[derive(Debug, PartialEq)]
pub struct TaskResult<T> {
    pub task_id: &'static str,
    pub decision: ExecutionDecision,
    pub value: T,
}

#[derive(Debug)]
pub struct Runtime {
    cpu: CpuAdapter,
    gpu: OnceLock<GpuAdapter>,
    resource_epoch: ResourceEpoch,
    boundary: PublishedBoundary,
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
        let resource_epoch = ResourceEpoch::new();
        let boundary = PublishedBoundary::new(config.boundary, resource_epoch.current());
        Self {
            cpu: CpuAdapter::default(),
            gpu: OnceLock::new(),
            resource_epoch,
            boundary,
            config,
        }
    }

    pub const fn execution_budget(&self) -> ExecutionBudget {
        self.config.execution_budget
    }

    pub const fn external_parallelism(&self) -> ExternalParallelism {
        self.config.external_parallelism
    }

    pub fn resource_epoch(&self) -> u64 {
        self.resource_epoch.current()
    }

    /// Mark all currently published boundaries stale.
    ///
    /// Native platform adapters should do no more policy work than this.
    pub fn invalidate_resources(&self) -> u64 {
        self.resource_epoch.invalidate()
    }

    #[inline]
    pub fn boundary_snapshot(&self) -> BoundarySnapshot {
        self.boundary.snapshot()
    }

    pub fn boundary_is_stale(&self) -> bool {
        self.resource_epoch
            .is_stale(self.boundary_snapshot().resource_epoch)
    }

    /// Publish a newly validated boundary at the current resource epoch.
    pub fn publish_boundary(&self, profile: BoundaryProfile) -> BoundarySnapshot {
        let snapshot = BoundarySnapshot::new(profile, self.resource_epoch.current());
        self.boundary.publish(snapshot);
        snapshot
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

    pub fn wait<T>(&self, handle: TaskHandle<T>) -> TaskResult<T> {
        TaskResult {
            task_id: handle.task_id,
            decision: handle.decision,
            value: handle.value,
        }
    }

    /// Submit a whole-range operation.
    ///
    /// Whole-range closures are not implicitly parallelized. Use submit_map for
    /// CPU parallel execution or submit_range_task for a registered GPU path.
    pub fn submit<T, F>(
        &self,
        task: &TaskDefinition,
        range: WorkRange,
        mode: ExecutionMode,
        operation: F,
    ) -> Result<TaskHandle<T>, RuntimeError>
    where
        F: FnOnce(WorkRange) -> T,
    {
        match mode {
            ExecutionMode::Auto | ExecutionMode::Serial => Ok(TaskHandle {
                task_id: task.id,
                decision: self.decision(BackendKind::Serial, None, None),
                value: operation(range),
            }),
            ExecutionMode::Cpu => Err(RuntimeError::BackendUnavailable(BackendKind::Cpu)),
            ExecutionMode::Gpu => Err(RuntimeError::BackendUnavailable(BackendKind::Gpu)),
        }
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
        self.submit_range_task(
            task,
            range,
            mode,
            RangeTaskImplementations::new(operation),
        )
    }

    pub fn submit_range_task<'a, T, F>(
        &self,
        task: &TaskDefinition,
        range: WorkRange,
        mode: ExecutionMode,
        implementations: RangeTaskImplementations<'a, T, F>,
    ) -> Result<TaskHandle<Vec<T>>, RuntimeError>
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        let gpu_eligible = implementations.gpu_eligible();
        let requested = match mode {
            ExecutionMode::Auto => {
                let boundary = self.boundary_snapshot().profile;
                select(range.len(), gpu_eligible, boundary)
            }
            ExecutionMode::Serial => BackendKind::Serial,
            ExecutionMode::Cpu => BackendKind::Cpu,
            ExecutionMode::Gpu => BackendKind::Gpu,
        };

        match requested {
            BackendKind::Serial => Ok(TaskHandle {
                task_id: task.id,
                decision: self.decision(BackendKind::Serial, None, None),
                value: (range.begin..range.end)
                    .map(&implementations.element)
                    .collect(),
            }),
            BackendKind::Cpu => {
                let (value, backend, constraint) =
                    self.execute_cpu(range, &implementations.element);
                Ok(TaskHandle {
                    task_id: task.id,
                    decision: self.decision(backend, None, constraint),
                    value,
                })
            }
            BackendKind::Gpu => {
                let Some(gpu_impl) = implementations.gpu.as_ref() else {
                    return if matches!(mode, ExecutionMode::Auto) {
                        let (value, backend, constraint) =
                            self.execute_cpu(range, &implementations.element);
                        Ok(TaskHandle {
                            task_id: task.id,
                            decision: self.decision(
                                backend,
                                Some(BackendKind::Gpu),
                                constraint,
                            ),
                            value,
                        })
                    } else {
                        Err(RuntimeError::BackendUnavailable(BackendKind::Gpu))
                    };
                };

                match self.gpu_adapter() {
                    Ok(adapter) => {
                        let context = GpuExecutionContext::new(adapter);
                        let value = gpu_impl(&context, range)?;
                        Ok(TaskHandle {
                            task_id: task.id,
                            decision: self.decision(BackendKind::Gpu, None, None),
                            value,
                        })
                    }
                    Err(error) if matches!(mode, ExecutionMode::Auto) => {
                        let _ = error;
                        let (value, backend, constraint) =
                            self.execute_cpu(range, &implementations.element);
                        Ok(TaskHandle {
                            task_id: task.id,
                            decision: self.decision(
                                backend,
                                Some(BackendKind::Gpu),
                                constraint,
                            ),
                            value,
                        })
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }

    fn execute_cpu<T, F>(
        &self,
        range: WorkRange,
        operation: &F,
    ) -> (Vec<T>, BackendKind, Option<ExecutionConstraint>)
    where
        T: Send,
        F: Fn(usize) -> T + Sync + Send,
    {
        if self.config.external_parallelism.active {
            return (
                (range.begin..range.end).map(operation).collect(),
                BackendKind::Serial,
                Some(ExecutionConstraint::ExternalParallelism),
            );
        }

        let execution = self.cpu.map(range, self.config.execution_budget, operation);
        match execution.kind {
            CpuExecutionKind::SerialBudget => (
                execution.values,
                BackendKind::Serial,
                Some(ExecutionConstraint::BudgetLimited),
            ),
            CpuExecutionKind::SerialNested => (
                execution.values,
                BackendKind::Serial,
                Some(ExecutionConstraint::NestedParallelism),
            ),
            CpuExecutionKind::SerialResourceLimited => (
                execution.values,
                BackendKind::Serial,
                Some(ExecutionConstraint::ResourceLimited),
            ),
            CpuExecutionKind::Parallel | CpuExecutionKind::ParallelBudgetLimited => {
                (execution.values, BackendKind::Cpu, None)
            }
        }
    }

    fn decision(
        &self,
        backend: BackendKind,
        fallback_from: Option<BackendKind>,
        constraint: Option<ExecutionConstraint>,
    ) -> ExecutionDecision {
        ExecutionDecision {
            backend,
            fallback_from,
            constraint,
            boundary_stale: self.boundary_is_stale(),
        }
    }

    fn gpu_adapter(&self) -> Result<&GpuAdapter, RuntimeError> {
        if let Some(adapter) = self.gpu.get() {
            return Ok(adapter);
        }

        let adapter = GpuAdapter::new()
            .map_err(|_| RuntimeError::BackendUnavailable(BackendKind::Gpu))?;
        let _ = self.gpu.set(adapter);
        self.gpu
            .get()
            .ok_or(RuntimeError::BackendUnavailable(BackendKind::Gpu))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidation_marks_boundary_stale_until_republished() {
        let runtime = Runtime::new();
        assert!(!runtime.boundary_is_stale());

        runtime.invalidate_resources();
        assert!(runtime.boundary_is_stale());

        runtime.publish_boundary(BoundaryProfile::new(64, 4096));
        assert!(!runtime.boundary_is_stale());
    }

    #[test]
    fn stale_boundary_is_still_used_by_fast_route() {
        let runtime = Runtime::with_config(RuntimeConfig {
            boundary: BoundaryProfile::new(8, 32),
            ..RuntimeConfig::default()
        });
        runtime.invalidate_resources();

        let task = TaskDefinition::new("map");
        let handle = runtime
            .submit_map(
                &task,
                WorkRange::new(0, 16),
                ExecutionMode::Auto,
                |index| index,
            )
            .expect("stale boundary remains routable");

        assert!(handle.decision().boundary_stale);
        assert!(matches!(
            handle.decision().backend,
            BackendKind::Cpu | BackendKind::Serial
        ));
    }
}
