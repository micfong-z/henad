//! Device limits the GPU models need above the WebGPU baseline.
//!
//! `wgpu::Limits::default()` is the spec baseline, not what hardware offers. It caps
//! `max_storage_buffers_per_shader_stage` at 8 where an M4 Pro reports 31, and a device only gets
//! more if it asks. Every host that creates one routes its descriptor through [`raise`].

/// Storage buffers per compute stage the engine asks for.
///
/// Fixed rather than `adapter.limits()`, so which models run cannot silently depend on the
/// machine.
pub const STORAGE_BUFFERS_PER_STAGE: u32 = 16;

/// Raises `base` to what the engine needs, clamped to what the adapter offers.
///
/// `request_device` fails outright if over-asked. A shortfall is logged, since it otherwise only
/// shows up much later as a bind group layout failing validation.
#[must_use]
pub fn raise(adapter: &wgpu::Adapter, base: &wgpu::Limits) -> wgpu::Limits {
    let available = adapter.limits().max_storage_buffers_per_shader_stage;
    if available < STORAGE_BUFFERS_PER_STAGE {
        log::warn!(
            "adapter '{}' offers {available} storage buffers per shader stage, below the {STORAGE_BUFFERS_PER_STAGE} Henad asks for; GPU models needing more will fail to build",
            adapter.get_info().name
        );
    }
    wgpu::Limits {
        max_storage_buffers_per_shader_stage: base
            .max_storage_buffers_per_shader_stage
            .max(STORAGE_BUFFERS_PER_STAGE)
            .min(available),
        ..base.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{STORAGE_BUFFERS_PER_STAGE, raise};
    use crate::gpu::headless_context;
    use crate::gpu::primitives::pipeline::storage_entry;

    /// A layout with more than the baseline 8 storage buffers must build.
    #[test]
    fn layout_past_the_webgpu_baseline_builds() {
        let Some(ctx) = headless_context("gpu_limits_test", wgpu::Features::empty()) else {
            log::warn!("skipping layout_past_the_webgpu_baseline_builds: no adapter");
            return;
        };

        let wanted = ctx.device.limits().max_storage_buffers_per_shader_stage;
        assert!(
            wanted >= STORAGE_BUFFERS_PER_STAGE,
            "the test device was created without the raised limit: {wanted}"
        );

        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..12).map(|i| storage_entry(i, i % 2 == 0)).collect();
        let _layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("twelve_storage_buffers"),
            entries: &entries,
        });
    }

    /// An adapter offering less must still yield a device, not an error.
    #[test]
    fn request_is_clamped_to_what_the_adapter_offers() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            log::warn!("skipping request_is_clamped_to_what_the_adapter_offers: no adapter");
            return;
        };

        let available = adapter.limits().max_storage_buffers_per_shader_stage;
        let raised = raise(&adapter, &wgpu::Limits::default());
        assert!(
            raised.max_storage_buffers_per_shader_stage <= available,
            "asked for more than the adapter offers, which would fail request_device"
        );
        assert_eq!(
            raised.max_texture_dimension_2d,
            wgpu::Limits::default().max_texture_dimension_2d,
            "unrelated limits must pass through untouched"
        );
    }
}
