//! wgpu-backed GPU adapter.
//!
//! M4 validates one explicit GPU kernel implementation: vector_scale.
//! This crate owns all wgpu-specific types and command submission details.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};

use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    AdapterUnavailable,
    DeviceUnavailable,
    PollFailed,
    MapFailed,
    ChannelClosed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct F32KernelKey {
    shader_source: String,
    workgroup_size: u32,
}

#[derive(Debug)]
pub struct PreparedF32Kernel {
    pipeline: wgpu::ComputePipeline,
    workgroup_size: u32,
}

pub struct ResidentF32Job {
    data_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    readback: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    byte_len: wgpu::BufferAddress,
    params_byte_len: wgpu::BufferAddress,
    len: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ResidentF32Timings {
    pub upload: std::time::Duration,
    pub compute_wait: std::time::Duration,
    pub readback_map: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDeviceInfo {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub vendor_id: u32,
    pub device_id: u32,
}

#[derive(Debug)]
pub struct GpuAdapter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: GpuDeviceInfo,
    f32_kernels: Mutex<HashMap<F32KernelKey, Arc<PreparedF32Kernel>>>,
}

impl GpuAdapter {
    pub fn new() -> Result<Self, GpuError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|_| GpuError::AdapterUnavailable)?;

        let adapter_info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|_| GpuError::DeviceUnavailable)?;

        Ok(Self {
            device,
            queue,
            info: GpuDeviceInfo {
                name: adapter_info.name,
                backend: format!("{:?}", adapter_info.backend),
                device_type: format!("{:?}", adapter_info.device_type),
                vendor_id: adapter_info.vendor,
                device_id: adapter_info.device,
            },
            f32_kernels: Mutex::new(HashMap::new()),
        })
    }

    pub fn device_info(&self) -> &GpuDeviceInfo {
        &self.info
    }

    pub fn round_trip_f32(&self, input: &[f32]) -> Result<Vec<f32>, GpuError> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("round-trip-source"),
                contents: cast_slice(input),
                usage: wgpu::BufferUsages::COPY_SRC,
            });

        let byte_len = std::mem::size_of_val(input) as wgpu::BufferAddress;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("round-trip-readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("round-trip-encoder"),
            });
        encoder.copy_buffer_to_buffer(&source, 0, &readback, 0, byte_len);

        let submission = self.queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|_| GpuError::PollFailed)?;

        receiver
            .recv()
            .map_err(|_| GpuError::ChannelClosed)?
            .map_err(|_| GpuError::MapFailed)?;

        let mapped = slice.get_mapped_range().map_err(|_| GpuError::MapFailed)?;
        let output = cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        readback.unmap();

        Ok(output)
    }

    pub fn prepare_f32_kernel(
        &self,
        shader_source: &str,
        workgroup_size: u32,
    ) -> PreparedF32Kernel {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("registered-f32-shader"),
                source: wgpu::ShaderSource::Wgsl(shader_source.into()),
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("registered-f32-pipeline"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

        PreparedF32Kernel {
            pipeline,
            workgroup_size: workgroup_size.max(1),
        }
    }

    pub fn prepare_resident_f32(
        &self,
        kernel: &PreparedF32Kernel,
        input_len: usize,
        params_len: usize,
    ) -> ResidentF32Job {
        let byte_len = (input_len * std::mem::size_of::<f32>()) as wgpu::BufferAddress;
        let params_byte_len =
            (params_len.max(1) * std::mem::size_of::<f32>()) as wgpu::BufferAddress;

        let data_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-f32-data"),
            size: byte_len.max(4),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let params_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-f32-params"),
            size: params_byte_len,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident-f32-readback"),
            size: byte_len.max(4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let bind_group_layout = kernel.pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-f32-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: data_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        ResidentF32Job {
            data_buffer,
            params_buffer,
            readback,
            bind_group,
            byte_len,
            params_byte_len,
            len: input_len,
        }
    }

    pub fn dispatch_resident_f32(
        &self,
        kernel: &PreparedF32Kernel,
        job: &ResidentF32Job,
        input: &[f32],
        params: &[f32],
    ) -> Result<Vec<f32>, GpuError> {
        assert_eq!(input.len(), job.len);
        assert_eq!(
            (params.len().max(1) * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
            job.params_byte_len
        );

        if input.is_empty() {
            return Ok(Vec::new());
        }

        self.queue
            .write_buffer(&job.data_buffer, 0, cast_slice(input));
        if !params.is_empty() {
            self.queue
                .write_buffer(&job.params_buffer, 0, cast_slice(params));
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resident-f32-encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resident-f32-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&kernel.pipeline);
            pass.set_bind_group(0, &job.bind_group, &[]);
            let total_workgroups = (job.len as u32).div_ceil(kernel.workgroup_size);
            let workgroups_x = total_workgroups.min(65_535);
            let workgroups_y = total_workgroups.div_ceil(workgroups_x);
            pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }
        encoder.copy_buffer_to_buffer(&job.data_buffer, 0, &job.readback, 0, job.byte_len);
        let submission = self.queue.submit(Some(encoder.finish()));

        let slice = job.readback.slice(..job.byte_len);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|_| GpuError::PollFailed)?;

        receiver
            .recv()
            .map_err(|_| GpuError::ChannelClosed)?
            .map_err(|_| GpuError::MapFailed)?;

        let mapped = slice.get_mapped_range().map_err(|_| GpuError::MapFailed)?;
        let output = cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        job.readback.unmap();

        Ok(output)
    }

    pub fn run_resident_f32_profiled(
        &self,
        kernel: &PreparedF32Kernel,
        job: &ResidentF32Job,
        input: &[f32],
        params: &[f32],
    ) -> Result<(Vec<f32>, ResidentF32Timings), GpuError> {
        assert_eq!(input.len(), job.len);
        assert_eq!(
            (params.len().max(1) * std::mem::size_of::<f32>()) as wgpu::BufferAddress,
            job.params_byte_len
        );

        let upload_started = std::time::Instant::now();
        self.queue
            .write_buffer(&job.data_buffer, 0, cast_slice(input));
        if !params.is_empty() {
            self.queue
                .write_buffer(&job.params_buffer, 0, cast_slice(params));
        }
        let upload_submission = self.queue.submit(std::iter::empty());
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(upload_submission),
                timeout: None,
            })
            .map_err(|_| GpuError::PollFailed)?;
        let upload = upload_started.elapsed();

        let compute_started = std::time::Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resident-f32-compute-encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resident-f32-compute-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&kernel.pipeline);
            pass.set_bind_group(0, &job.bind_group, &[]);
            let total_workgroups = (job.len as u32).div_ceil(kernel.workgroup_size);
            let workgroups_x = total_workgroups.min(65_535);
            let workgroups_y = total_workgroups.div_ceil(workgroups_x);
            pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }
        let compute_submission = self.queue.submit(Some(encoder.finish()));
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(compute_submission),
                timeout: None,
            })
            .map_err(|_| GpuError::PollFailed)?;
        let compute_wait = compute_started.elapsed();

        let readback_started = std::time::Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("resident-f32-readback-encoder"),
            });
        encoder.copy_buffer_to_buffer(&job.data_buffer, 0, &job.readback, 0, job.byte_len);
        let submission = self.queue.submit(Some(encoder.finish()));

        let slice = job.readback.slice(..job.byte_len);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|_| GpuError::PollFailed)?;

        receiver
            .recv()
            .map_err(|_| GpuError::ChannelClosed)?
            .map_err(|_| GpuError::MapFailed)?;

        let mapped = slice.get_mapped_range().map_err(|_| GpuError::MapFailed)?;
        let output = cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        job.readback.unmap();
        let readback_map = readback_started.elapsed();

        Ok((
            output,
            ResidentF32Timings {
                upload,
                compute_wait,
                readback_map,
            },
        ))
    }

    pub fn dispatch_prepared_f32(
        &self,
        kernel: &PreparedF32Kernel,
        input: &[f32],
        params: &[f32],
    ) -> Result<Vec<f32>, GpuError> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let data_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("registered-f32-data"),
                contents: cast_slice(input),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });

        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("registered-f32-params"),
                contents: cast_slice(params),
                usage: wgpu::BufferUsages::STORAGE,
            });

        let byte_len = std::mem::size_of_val(input) as wgpu::BufferAddress;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("registered-f32-readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let bind_group_layout = kernel.pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("registered-f32-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: data_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("registered-f32-encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("registered-f32-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&kernel.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let total_workgroups = (input.len() as u32).div_ceil(kernel.workgroup_size);
            let workgroups_x = total_workgroups.min(65_535);
            let workgroups_y = total_workgroups.div_ceil(workgroups_x);
            pass.dispatch_workgroups(workgroups_x, workgroups_y, 1);
        }

        encoder.copy_buffer_to_buffer(&data_buffer, 0, &readback, 0, byte_len);
        let submission = self.queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });

        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|_| GpuError::PollFailed)?;

        receiver
            .recv()
            .map_err(|_| GpuError::ChannelClosed)?
            .map_err(|_| GpuError::MapFailed)?;

        let mapped = slice.get_mapped_range().map_err(|_| GpuError::MapFailed)?;
        let output = cast_slice::<u8, f32>(&mapped).to_vec();
        drop(mapped);
        readback.unmap();

        Ok(output)
    }

    fn cached_f32_kernel(
        &self,
        shader_source: &str,
        workgroup_size: u32,
    ) -> Arc<PreparedF32Kernel> {
        let key = F32KernelKey {
            shader_source: shader_source.to_owned(),
            workgroup_size: workgroup_size.max(1),
        };

        let mut kernels = self
            .f32_kernels
            .lock()
            .expect("GPU kernel cache mutex must not be poisoned");

        if let Some(kernel) = kernels.get(&key) {
            return Arc::clone(kernel);
        }

        let kernel = Arc::new(self.prepare_f32_kernel(shader_source, workgroup_size));
        kernels.insert(key, Arc::clone(&kernel));
        kernel
    }

    pub fn cached_f32_kernel_count(&self) -> usize {
        self.f32_kernels
            .lock()
            .expect("GPU kernel cache mutex must not be poisoned")
            .len()
    }

    pub fn dispatch_f32(
        &self,
        shader_source: &str,
        input: &[f32],
        params: &[f32],
        workgroup_size: u32,
    ) -> Result<Vec<f32>, GpuError> {
        let kernel = self.cached_f32_kernel(shader_source, workgroup_size);
        self.dispatch_prepared_f32(&kernel, input, params)
    }

    pub fn vector_scale(&self, input: &[f32], alpha: f32) -> Result<Vec<f32>, GpuError> {
        const SHADER: &str = r#"
@group(0) @binding(0)
var<storage, read_write> data: array<f32>;

@group(0) @binding(1)
var<storage, read> params: array<f32>;

@compute @workgroup_size(64)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let row_width = groups.x * 64u;
    let i = gid.x + gid.y * row_width;
    if (i < arrayLength(&data)) {
        data[i] = data[i] * params[0];
    }
}
"#;

        self.dispatch_f32(SHADER, input, &[alpha], 64)
    }
}
