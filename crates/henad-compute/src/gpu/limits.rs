//! Device limits the GPU models need above the WebGPU baseline.
//!
//! Sizes go to the adapter's report, counts to exactly the models' needs, since wgpu's own
//! advice is to request no more than that. `raise` takes the count rather than knowing it, since
//! henad-compute cannot see the models and a host needs the number before it has a device.

/// Raises `base` to the models' requirements, clamped to the adapter's.
///
/// `storage_buffers` comes from `henad_models::registry::gpu_storage_bindings_needed()`.
pub fn raise(adapter: &wgpu::Adapter, base: &wgpu::Limits, storage_buffers: u32) -> wgpu::Limits {
    let available = adapter.limits();
    // Otherwise this only surfaces much later, as a bind group layout failing validation.
    if available.max_storage_buffers_per_shader_stage < storage_buffers {
        log::warn!(
            "adapter '{}' offers {} storage buffers per shader stage, below the {storage_buffers} the models need; the widest ones will fail to build",
            adapter.get_info().name,
            available.max_storage_buffers_per_shader_stage
        );
    }
    wgpu::Limits {
        max_storage_buffers_per_shader_stage: base
            .max_storage_buffers_per_shader_stage
            .max(storage_buffers)
            .min(available.max_storage_buffers_per_shader_stage),
        max_texture_dimension_2d: base.max_texture_dimension_2d.max(available.max_texture_dimension_2d),
        max_storage_buffer_binding_size: base
            .max_storage_buffer_binding_size
            .max(available.max_storage_buffer_binding_size),
        max_buffer_size: base.max_buffer_size.max(available.max_buffer_size),
        ..base.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::raise;

    fn adapter() -> Option<wgpu::Adapter> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()
    }

    /// Over-asking fails `request_device` outright, so this is the safety property.
    #[test]
    fn nothing_is_asked_for_past_what_the_adapter_offers() {
        let Some(adapter) = adapter() else {
            log::warn!("skipping nothing_is_asked_for_past_what_the_adapter_offers: no adapter");
            return;
        };
        let available = adapter.limits();
        // Far past any adapter, so this tests the clamp rather than the input.
        let raised = raise(&adapter, &wgpu::Limits::default(), 4096);
        assert!(
            raised.max_texture_dimension_2d <= available.max_texture_dimension_2d
                && raised.max_storage_buffer_binding_size <= available.max_storage_buffer_binding_size
                && raised.max_buffer_size <= available.max_buffer_size
                && raised.max_storage_buffers_per_shader_stage <= available.max_storage_buffers_per_shader_stage,
            "asked for more than the adapter offers, which would fail request_device"
        );
    }

    /// The three size limits are the whole point, so a baseline request must move them.
    #[test]
    fn size_limits_reach_what_the_adapter_reports() {
        let Some(adapter) = adapter() else {
            log::warn!("skipping size_limits_reach_what_the_adapter_reports: no adapter");
            return;
        };
        let available = adapter.limits();
        let raised = raise(&adapter, &wgpu::Limits::default(), 0);
        assert_eq!(raised.max_texture_dimension_2d, available.max_texture_dimension_2d);
        assert_eq!(
            raised.max_storage_buffer_binding_size,
            available.max_storage_buffer_binding_size
        );
        assert_eq!(raised.max_buffer_size, available.max_buffer_size);
    }

    /// A host with no models must get a device no wider than the one models are tested against.
    #[test]
    fn needing_nothing_leaves_the_count_at_the_baseline() {
        let Some(adapter) = adapter() else {
            log::warn!("skipping needing_nothing_leaves_the_count_at_the_baseline: no adapter");
            return;
        };
        let base = wgpu::Limits::default();
        let raised = raise(&adapter, &base, 0);
        assert_eq!(
            raised.max_storage_buffers_per_shader_stage,
            base.max_storage_buffers_per_shader_stage
        );
    }

    #[test]
    fn a_need_above_the_baseline_raises_only_the_count() {
        let Some(adapter) = adapter() else {
            log::warn!("skipping a_need_above_the_baseline_raises_only_the_count: no adapter");
            return;
        };
        let base = wgpu::Limits::default();
        let want = base.max_storage_buffers_per_shader_stage + 2;
        if adapter.limits().max_storage_buffers_per_shader_stage < want {
            log::warn!("skipping a_need_above_the_baseline_raises_only_the_count: adapter too small");
            return;
        }
        let raised = raise(&adapter, &base, want);
        assert_eq!(raised.max_storage_buffers_per_shader_stage, want);
        assert_eq!(
            raised.max_compute_workgroups_per_dimension, base.max_compute_workgroups_per_dimension,
            "unrelated limits must pass through untouched"
        );
        assert_eq!(raised.max_bind_groups, base.max_bind_groups);
    }
}
