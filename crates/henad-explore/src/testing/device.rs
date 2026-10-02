//! A headless device for tests, at the limits and features a test asks for.

#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::GpuNeeds;

/// Limits and features a test device is asked for.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TestDeviceRequest {
    /// Needs the baseline is raised to, `None` for the baseline itself.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the native headless_test_device alone reads a request")
    )]
    needs: Option<GpuNeeds>,
    features: wgpu::Features,
}

impl TestDeviceRequest {
    /// `wgpu::Limits::default()`, the WebGPU baseline a browser offers.
    pub fn baseline() -> Self {
        Self {
            needs: None,
            features: wgpu::Features::empty(),
        }
    }

    /// The baseline raised by [`henad_compute::gpu::limits::raise`] for `needs`, as a host's device is.
    ///
    /// Note that a browser without those limits refuses a model that needs them.
    pub fn raised(needs: GpuNeeds) -> Self {
        Self {
            needs: Some(needs),
            features: wgpu::Features::empty(),
        }
    }

    /// Asks for `features` as well. A device without them is no device for this request.
    ///
    /// `features` names [`henad_compute::gpu::wgpu::Features`], and needs no `wgpu` dependency of the caller's own.
    pub fn features(mut self, features: wgpu::Features) -> Self {
        self.features |= features;
        self
    }
}

/// Returns a headless device for `request`, or `None` on a machine without one.
///
/// The device carries its adapter's [`henad_compute::runtime_info::RuntimeInfo`]. Returns `None` as well when the
/// adapter lacks an optional feature the request names, even under `HENAD_REQUIRE_GPU`.
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
    // A software adapter owes a test nothing optional.
    if !adapter.features().contains(request.features) {
        return None;
    }
    let baseline = wgpu::Limits::default();
    let required_limits = match request.needs {
        Some(needs) => henad_compute::gpu::limits::raise(&adapter, &baseline, needs),
        None => baseline,
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
/// Empty reads as unset. A workflow matrix can then leave the variable blank on a runner without a GPU.
pub(super) fn gpu_required() -> bool {
    std::env::var_os("HENAD_REQUIRE_GPU").is_some_and(|value| !value.is_empty() && value != "0")
}
