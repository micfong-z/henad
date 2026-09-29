//! A GPU device acquired with no window or surface.
//!
//! Native only. `pollster` blocks on the request, and a browser cannot block.

use std::fmt;

use henad_compute::fault::FaultSink;
use henad_compute::gpu::GpuContext;
use henad_compute::runtime_info::RuntimeInfo;

/// Failure to acquire a headless device.
#[derive(Debug)]
pub enum DeviceError {
    /// No adapter suits the request.
    NoAdapter(wgpu::RequestAdapterError),
    /// The adapter refused to create a device.
    NoDevice(wgpu::RequestDeviceError),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(_) => f.write_str("no suitable GPU adapter found"),
            Self::NoDevice(_) => f.write_str("failed to create GPU device"),
        }
    }
}

impl std::error::Error for DeviceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoAdapter(error) => Some(error),
            Self::NoDevice(error) => Some(error),
        }
    }
}

/// Acquire a headless GPU device, the same thing eframe does for henad-app minus any window or
/// surface. `henad-compute` never creates a device, so a non-GUI runner must.
///
/// The adapter is dropped here, so `RuntimeInfo` has to be captured.
///
/// # Errors
///
/// Returns [`DeviceError`] when this machine offers no suitable adapter, or the adapter creates no device.
pub fn acquire_headless() -> Result<(GpuContext, RuntimeInfo), DeviceError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(DeviceError::NoAdapter)?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("henad-explore"),
        required_features: wgpu::Features::empty(),
        required_limits: henad_compute::gpu::limits::raise(
            &adapter,
            &wgpu::Limits::default(),
            henad_models::registry::gpu_storage_bindings_needed(),
        ),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(DeviceError::NoDevice)?;
    let runtime = RuntimeInfo::collect(&adapter, &device);
    // No surface exists, so `target_format` is arbitrary: the models' display texture is an
    // offscreen Rgba8Unorm target, never a swapchain, and a headless run never reads it back.
    Ok((
        GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, FaultSink::new()),
        runtime,
    ))
}
