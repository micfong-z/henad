//! A GPU device acquired with no window or surface.
//!
//! Native only. `pollster` blocks on the request, and a browser cannot block.

use std::fmt;

use henad_compute::fault::FaultSink;
use henad_compute::gpu::{GpuContext, GpuNeeds};
use henad_compute::runtime_info::RuntimeInfo;

/// Failure to acquire a headless device.
#[derive(Debug)]
pub enum DeviceError {
    /// No adapter suits the request.
    NoAdapter(wgpu::RequestAdapterError),
    /// Adapter `adapter` offers less than the WebGPU baseline in limit `limit`, as a GL adapter can.
    BelowBaseline { adapter: String, limit: &'static str },
    /// The adapter refused to create a device.
    NoDevice(wgpu::RequestDeviceError),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(_) => f.write_str("no suitable GPU adapter found"),
            Self::BelowBaseline { adapter, limit } => {
                write!(f, "adapter '{adapter}' offers less than the WebGPU baseline in {limit}")
            }
            Self::NoDevice(_) => f.write_str("failed to create GPU device"),
        }
    }
}

impl std::error::Error for DeviceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoAdapter(error) => Some(error),
            Self::BelowBaseline { .. } => None,
            Self::NoDevice(error) => Some(error),
        }
    }
}

/// Acquires a headless device for `needs`, with its adapter's [`RuntimeInfo`] attached to the context.
///
/// This is the device eframe acquires for henad-app, minus any window or surface. `henad-compute` never creates a
/// device, so a non-GUI runner must.
///
/// The device is requested at the WebGPU baseline on every backend, raised to `needs` by
/// [`henad_compute::gpu::limits::raise`]. Note that an adapter below the baseline, as a GL adapter can be, gets no
/// device here. henad-app takes a lower base for a GL adapter, draws with it and runs no GPU model on it.
///
/// # Errors
///
/// Returns [`DeviceError::NoAdapter`] when this machine offers no suitable adapter,
/// [`DeviceError::BelowBaseline`] for an adapter below the baseline, and [`DeviceError::NoDevice`] when the adapter
/// creates no device.
pub fn acquire_headless(needs: GpuNeeds) -> Result<GpuContext, DeviceError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .map_err(DeviceError::NoAdapter)?;
    let required_limits = device_limits(&adapter.limits(), |baseline| {
        henad_compute::gpu::limits::raise(&adapter, baseline, needs)
    })
    .map_err(|limit| DeviceError::BelowBaseline {
        adapter: adapter.get_info().name,
        limit,
    })?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("henad-explore"),
        required_features: wgpu::Features::empty(),
        required_limits,
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(DeviceError::NoDevice)?;
    let runtime = RuntimeInfo::collect(&adapter, &device);
    // No surface exists, so `target_format` is arbitrary: the models' display texture is an
    // offscreen Rgba8Unorm target, never a swapchain, and a headless run never reads it back.
    Ok(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, FaultSink::new()).with_runtime_info(runtime))
}

/// Returns the limits to request from an adapter offering `available`, `raise` applied to the WebGPU baseline.
///
/// The adapter is checked against the baseline before the raise. The raise clamps the storage buffer count to the
/// adapter's.
///
/// # Errors
///
/// Returns the name of the first limit of the baseline that `available` falls short of.
pub(crate) fn device_limits(
    available: &wgpu::Limits,
    raise: impl FnOnce(&wgpu::Limits) -> wgpu::Limits,
) -> Result<wgpu::Limits, &'static str> {
    let baseline = wgpu::Limits::default();
    match short_limit(&baseline, available) {
        Some(limit) => Err(limit),
        None => Ok(raise(&baseline)),
    }
}

/// Returns the name of the first limit of `required` that `available` falls short of, or `None` when it offers them
/// all.
fn short_limit(required: &wgpu::Limits, available: &wgpu::Limits) -> Option<&'static str> {
    let mut short = None;
    required.check_limits_with_fail_fn(available, true, |name, _, _| short = Some(name));
    short
}

#[cfg(test)]
mod tests {
    use super::{device_limits, short_limit};

    #[test]
    fn a_webgl2_adapter_falls_short_of_the_baseline() {
        let baseline = wgpu::Limits::default();
        assert_eq!(short_limit(&baseline, &baseline), None);
        assert!(short_limit(&baseline, &wgpu::Limits::downlevel_webgl2_defaults()).is_some());
    }

    /// An adapter one storage buffer short of the baseline is refused. The raise clamps the count to the adapter's 7,
    /// and a check of the raised limits would pass it.
    #[test]
    fn an_adapter_short_of_storage_buffers_falls_short_of_the_baseline() {
        let available = wgpu::Limits {
            max_storage_buffers_per_shader_stage: 7,
            ..wgpu::Limits::default()
        };
        let clamp = |base: &wgpu::Limits| wgpu::Limits {
            max_storage_buffers_per_shader_stage: base
                .max_storage_buffers_per_shader_stage
                .min(available.max_storage_buffers_per_shader_stage),
            ..base.clone()
        };
        assert_eq!(
            device_limits(&available, clamp).err(),
            Some("max_storage_buffers_per_shader_stage")
        );
        assert!(device_limits(&wgpu::Limits::default(), clamp).is_ok());
    }
}
