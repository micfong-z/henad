//! A headless device for tests, at the limits and features that a test requests.

#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::GpuNeeds;

/// Limits and features to request for a test device.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TestDeviceRequest {
    /// Needs that the baseline is raised to meet, `None` for the baseline itself.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the native headless_test_device alone reads a request")
    )]
    needs: Option<GpuNeeds>,
    features: wgpu::Features,
}

impl TestDeviceRequest {
    /// Returns a request for `wgpu::Limits::default()`, the WebGPU baseline a browser offers.
    pub fn baseline() -> Self {
        Self {
            needs: None,
            features: wgpu::Features::empty(),
        }
    }

    /// Returns a request for the baseline raised by [`henad_compute::gpu::limits::raise`] for `needs`, as a host's
    /// device is raised.
    ///
    /// Note that a browser without those limits rejects a model that needs them.
    pub fn raised(needs: GpuNeeds) -> Self {
        Self {
            needs: Some(needs),
            features: wgpu::Features::empty(),
        }
    }

    /// Requests `features` as well. `headless_test_device` returns `None` for this request on an adapter without them.
    ///
    /// A caller can name the type of `features` as [`henad_compute::gpu::wgpu::Features`], without a direct `wgpu`
    /// dependency.
    pub fn features(mut self, features: wgpu::Features) -> Self {
        self.features |= features;
        self
    }
}

/// Returns a headless device for `request`, or `None` on a machine without a device.
///
/// The device carries its adapter's [`henad_compute::runtime_info::RuntimeInfo`]. Returns `None` as well when the
/// adapter lacks an optional feature that the request lists, even under `HENAD_REQUIRE_GPU`. An adapter that offers
/// less than the WebGPU baseline yields no device for a baseline or a raised request.
///
/// Note that each test takes its own device. Clones of a context share its fault sink, and a check of the kit
/// attributes every fault it finds there to itself. A device error that another test leaves in a shared sink is then
/// dropped, or reported as that check's failure.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set to anything but empty or `0` and no device is available.
#[cfg(not(target_arch = "wasm32"))]
pub fn headless_test_device(request: &TestDeviceRequest) -> Option<GpuContext> {
    use henad_compute::fault::FaultSink;
    use henad_compute::runtime_info::RuntimeInfo;

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }));
    let adapter = match adapter {
        Ok(adapter) => adapter,
        Err(error) => return unavailable(&format!("no adapter is available: {error}")),
    };
    // A software adapter is not required to offer optional features.
    if !adapter.features().contains(request.features) {
        return None;
    }
    let raise = |baseline: &wgpu::Limits, needs| henad_compute::gpu::limits::raise(&adapter, baseline, needs);
    let required_limits = match requested_limits(request, &adapter.limits(), raise) {
        Ok(limits) => limits,
        Err(limit) => {
            return unavailable(&format!(
                "adapter '{}' offers less than the WebGPU baseline in {limit}",
                adapter.get_info().name
            ));
        }
    };
    let device = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("henad-test"),
        required_features: request.features,
        required_limits,
        ..Default::default()
    }));
    match device {
        Ok((device, queue)) => {
            let runtime = RuntimeInfo::collect(&adapter, &device);
            Some(
                GpuContext::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, FaultSink::new())
                    .with_runtime_info(runtime),
            )
        }
        Err(error) => unavailable(&format!(
            "adapter '{}' gave no device: {error}",
            adapter.get_info().name
        )),
    }
}

/// Returns the limits to request for `request` from an adapter that offers `available`. `raise` raises the baseline
/// to the needs of a raised request.
///
/// The adapter is checked against the baseline before the raise, as [`crate::device::acquire_headless`] checks it.
/// The raise clamps the storage buffer count to the adapter's count. Otherwise a test would get a device below the
/// baseline.
///
/// # Errors
///
/// Returns the name of the first baseline limit that `available` does not meet.
#[cfg(not(target_arch = "wasm32"))]
fn requested_limits(
    request: &TestDeviceRequest,
    available: &wgpu::Limits,
    raise: impl FnOnce(&wgpu::Limits, GpuNeeds) -> wgpu::Limits,
) -> Result<wgpu::Limits, &'static str> {
    crate::device::device_limits(available, |baseline| match request.needs {
        Some(needs) => raise(baseline, needs),
        None => baseline.clone(),
    })
}

/// Returns `None` for a test to skip on, after checking that `HENAD_REQUIRE_GPU` allows it.
///
/// # Panics
///
/// Panics with `reason` when `HENAD_REQUIRE_GPU` is set to anything but empty or `0`.
#[cfg(not(target_arch = "wasm32"))]
fn unavailable(reason: &str) -> Option<GpuContext> {
    assert!(!gpu_required(), "HENAD_REQUIRE_GPU is set, but {reason}");
    None
}

/// Returns whether `HENAD_REQUIRE_GPU` is set to anything but empty or `0`.
///
/// An empty value is treated as unset. A workflow matrix can then leave the variable blank on a runner without a GPU.
pub(super) fn gpu_required() -> bool {
    std::env::var_os("HENAD_REQUIRE_GPU").is_some_and(|value| !value.is_empty() && value != "0")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use henad_compute::gpu::GpuNeeds;

    use super::{TestDeviceRequest, requested_limits};

    /// An adapter one storage buffer short of the baseline yields no device for a baseline or a raised request. The
    /// raise clamps the count to the adapter's 7, and a check of the raised limits would pass it.
    #[test]
    fn an_adapter_short_of_the_baseline_gives_no_raised_device() {
        let available = wgpu::Limits {
            max_storage_buffers_per_shader_stage: 7,
            ..wgpu::Limits::default()
        };
        let clamp = |base: &wgpu::Limits, needs: GpuNeeds| wgpu::Limits {
            max_storage_buffers_per_shader_stage: base
                .max_storage_buffers_per_shader_stage
                .max(needs.storage_buffers())
                .min(available.max_storage_buffers_per_shader_stage),
            ..base.clone()
        };
        let raised = TestDeviceRequest::raised(GpuNeeds::with_storage_buffers(8));
        for request in [TestDeviceRequest::baseline(), raised.clone()] {
            assert_eq!(
                requested_limits(&request, &available, clamp).err(),
                Some("max_storage_buffers_per_shader_stage"),
                "{request:?}"
            );
        }
        assert!(requested_limits(&raised, &wgpu::Limits::default(), clamp).is_ok());
    }
}
