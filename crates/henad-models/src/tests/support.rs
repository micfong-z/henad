//! Headless device setup shared by the GPU models' tests.

use henad_compute::gpu::GpuContext;

const REQUIRE_GPU: &str = "HENAD_REQUIRE_GPU";

/// Consider empty as unset, hence a workflow matrix can leave the variable blank on runners that don't have a GPU.
fn gpu_required() -> bool {
    std::env::var_os(REQUIRE_GPU).is_some_and(|v| !v.is_empty() && v != "0")
}

/// A headless device for the GPU tests, or `None` when this machine cannot give one.
///
/// Missing `required_features` still skips under `HENAD_REQUIRE_GPU`, since a software rasteriser
/// owes us nothing optional. A missing adapter does not.
pub(crate) fn headless_context(label: &str, required_features: wgpu::Features) -> Option<GpuContext> {
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
