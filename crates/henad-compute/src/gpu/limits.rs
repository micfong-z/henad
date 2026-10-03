//! Device limits the GPU models need above the WebGPU baseline.
//!
//! Sizes go to the adapter's report, counts to exactly the models' needs, since wgpu's own
//! advice is to request no more than that. `raise` takes the needs rather than knowing them, since
//! henad-compute cannot see the models and a host needs them before it has a device.

/// Device capabilities a set of models needs above the WebGPU baseline, known before any device exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuNeeds {
    storage_buffers: u32,
}

impl GpuNeeds {
    /// Returns needs of `storage_buffers` storage buffers per shader stage.
    pub const fn with_storage_buffers(storage_buffers: u32) -> Self {
        Self { storage_buffers }
    }

    /// Storage buffers the widest pass binds per shader stage.
    pub fn storage_buffers(&self) -> u32 {
        self.storage_buffers
    }

    /// Returns needs covering both `self` and `other`.
    pub fn merge(self, other: Self) -> Self {
        Self {
            storage_buffers: self.storage_buffers.max(other.storage_buffers),
        }
    }
}

/// Raises `base` to the models' `needs`, clamped to the adapter's.
pub fn raise(adapter: &wgpu::Adapter, base: &wgpu::Limits, needs: GpuNeeds) -> wgpu::Limits {
    let storage_buffers = needs.storage_buffers();
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
    use super::{GpuNeeds, raise};

    /// The default adapter, or `None` when this machine has none.
    ///
    /// # Panics
    ///
    /// If `HENAD_REQUIRE_GPU` is set and no adapter is available.
    fn adapter() -> Option<wgpu::Adapter> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
        if let Err(err) = &adapter {
            assert!(
                !crate::gpu::tests::support::gpu_required(),
                "HENAD_REQUIRE_GPU is set but no wgpu adapter is available: {err}"
            );
        }
        adapter.ok()
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
        let raised = raise(&adapter, &wgpu::Limits::default(), GpuNeeds::with_storage_buffers(4096));
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
        let raised = raise(&adapter, &wgpu::Limits::default(), GpuNeeds::default());
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
        let raised = raise(&adapter, &base, GpuNeeds::default());
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
        let raised = raise(&adapter, &base, GpuNeeds::with_storage_buffers(want));
        assert_eq!(raised.max_storage_buffers_per_shader_stage, want);
        assert_eq!(
            raised.max_compute_workgroups_per_dimension, base.max_compute_workgroups_per_dimension,
            "unrelated limits must pass through untouched"
        );
        assert_eq!(raised.max_bind_groups, base.max_bind_groups);
    }
}
