//! Headless device for this crate's GPU tests.
//!
//! Kept private, so the engine's public surface stays "inject a `GpuContext`".

use crate::gpu::GpuContext;

/// Set in CI to turn "no GPU here" from a skip into a failure.
const REQUIRE_GPU: &str = "HENAD_REQUIRE_GPU";

/// Empty counts as unset, so a workflow matrix can blank it out on runners without a GPU.
fn gpu_required() -> bool {
    std::env::var_os(REQUIRE_GPU).is_some_and(|v| !v.is_empty() && v != "0")
}

/// A headless device, or `None` when this machine cannot give one.
///
/// # Panics
///
/// If [`REQUIRE_GPU`] is set and no adapter or device could be acquired. Missing
/// `required_features` still returns `None`, since a software rasteriser owes us nothing
/// optional.
#[must_use]
pub fn headless_context(label: &str, required_features: wgpu::Features) -> Option<GpuContext> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    if let Err(err) = &adapter {
        assert!(
            !gpu_required(),
            "{REQUIRE_GPU} is set but no wgpu adapter is available: {err}"
        );
    }
    let adapter = adapter.ok()?;

    if !adapter.features().contains(required_features) {
        return None;
    }

    let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        required_features,
        // Same raise the app applies, so a model that builds here builds there.
        required_limits: crate::gpu::limits::raise(&adapter, &wgpu::Limits::default()),
        ..Default::default()
    }));
    if let Err(err) = &device {
        assert!(
            !gpu_required(),
            "{REQUIRE_GPU} is set but adapter '{}' gave no device: {err}",
            adapter.get_info().name
        );
    }
    let (device, queue) = device.ok()?;

    Some(GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm))
}
