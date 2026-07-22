//! Host and adapter facts, gathered once at startup. Shared by the GUI and the headless runner.

/// Host facts, available with or without an adapter.
pub struct HostInfo {
    pub os: &'static str,
    pub arch: &'static str,
    /// `None` where the platform cannot report it (notably wasm).
    pub logical_cpus: Option<usize>,
    /// Size of rayon's pool, i.e. how wide the CPU models actually step. `None` on wasm, which has
    /// no thread pool at all.
    pub worker_threads: Option<usize>,
}

impl HostInfo {
    pub fn collect() -> Self {
        Self {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            logical_cpus: std::thread::available_parallelism()
                .ok()
                .map(std::num::NonZeroUsize::get),
            #[cfg(not(target_arch = "wasm32"))]
            worker_threads: Some(rayon::current_num_threads()),
            #[cfg(target_arch = "wasm32")]
            worker_threads: None,
        }
    }
}

pub struct RuntimeInfo {
    pub host: HostInfo,
    pub adapter: wgpu::AdapterInfo,
    /// Ceiling on one storage binding — bounds how many cells a GPU model can hold.
    pub max_storage_binding_bytes: u64,
    /// Whether the device granted `TIMESTAMP_QUERY`.
    pub timestamp_query: bool,
}

impl RuntimeInfo {
    pub fn collect(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Self {
        Self {
            host: HostInfo::collect(),
            adapter: adapter.get_info(),
            max_storage_binding_bytes: device.limits().max_storage_buffer_binding_size,
            timestamp_query: device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        }
    }
}

/// How well the adapter suits Henad's workload.
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
