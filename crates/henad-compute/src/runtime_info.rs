//! Host and adapter facts, gathered once at startup. Shared by the GUI and the headless runner.

/// Host facts, available with or without an adapter.
pub struct HostInfo {
    pub os: &'static str,
    pub arch: &'static str,
    /// `None` where the platform cannot report it.
    pub logical_cpus: Option<usize>,
    /// Size of rayon's pool, i.e. how wide the CPU models actually step.
    pub worker_threads: Option<usize>,
}

impl HostInfo {
    /// Call once the thread pool exists. Asking rayon first would build it with rayon's own
    /// defaults, which in a browser means asking for threads the pool has no way to spawn.
    pub fn collect() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            logical_cpus: logical_cpus(),
            worker_threads: Some(rayon::current_num_threads()),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn logical_cpus() -> Option<usize> {
    std::thread::available_parallelism()
        .ok()
        .map(std::num::NonZeroUsize::get)
}

/// `available_parallelism` is unsupported on wasm. The browser reports the same count itself.
#[cfg(target_arch = "wasm32")]
fn logical_cpus() -> Option<usize> {
    let cores = web_sys::window()?.navigator().hardware_concurrency();
    (cores >= 1.0).then_some(cores as usize)
}

pub struct RuntimeInfo {
    pub host: HostInfo,
    pub adapter: wgpu::AdapterInfo,
    /// Limits the device was created with, after `gpu::limits::raise`.
    pub granted: wgpu::Limits,
    /// Limits the adapter would have allowed, so a gap is headroom left unclaimed.
    pub available: wgpu::Limits,
    /// Set when the device granted `TIMESTAMP_QUERY`.
    pub timestamp_query: bool,
}

impl RuntimeInfo {
    pub fn collect(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Self {
        Self {
            host: HostInfo::collect(),
            adapter: adapter.get_info(),
            granted: device.limits(),
            available: adapter.limits(),
            timestamp_query: device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        }
    }

    /// Largest display texture asked for here, after Henad's own cap.
    pub fn display_cap(&self) -> u32 {
        crate::display_scale::MAX_DISPLAY_DIM.min(self.granted.max_texture_dimension_2d)
    }
}

/// The adapter's fitness for Henad's workload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuVerdict {
    /// A real GPU :)
    Capable,
    /// Uncertain whether this "GPU" will run Henad well. It might be anything from a low-end
    /// integrated GPU to something like an M4 Pro.
    Uncertain,
    /// Not a GPU :(
    Absent,
}

/// True for every backend except `Gl`, as WebGL2 has no compute stage.
pub fn supports_compute(info: &wgpu::AdapterInfo) -> bool {
    info.backend != wgpu::Backend::Gl
}

/// `DeviceType` describes memory topology, not speed, so only `DiscreteGpu` is claimed outright.
pub fn classify_adapter(info: &wgpu::AdapterInfo) -> GpuVerdict {
    if info.device_type == wgpu::DeviceType::Cpu {
        GpuVerdict::Absent
    } else if info.device_type == wgpu::DeviceType::DiscreteGpu {
        GpuVerdict::Capable
    } else {
        GpuVerdict::Uncertain
    }
}
