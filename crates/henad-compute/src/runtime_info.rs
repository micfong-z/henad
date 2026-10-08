//! Host and adapter facts, gathered once at startup for a host to show and a sweep to record.

/// Host facts, available with or without an adapter.
#[derive(Debug, Clone)]
pub struct HostInfo {
    /// Operating system name as in `std::env::consts::OS`, and `browser` on wasm32, where that constant is empty.
    pub os: &'static str,
    /// CPU architecture as in `std::env::consts::ARCH`.
    pub arch: &'static str,
    /// Number of logical CPUs, `None` where the platform cannot report it.
    pub logical_cpus: Option<usize>,
    /// Size of rayon's pool, the number of workers a CPU model runs on.
    pub worker_threads: Option<usize>,
}

impl HostInfo {
    /// Collects the host's facts.
    ///
    /// Call it once the thread pool exists. Asking rayon first would build the pool with rayon's own
    /// defaults, and in a browser those request threads that the pool cannot spawn.
    pub fn collect() -> Self {
        Self {
            os: os_name(std::env::consts::OS),
            arch: std::env::consts::ARCH,
            logical_cpus: logical_cpus(),
            worker_threads: Some(rayon::current_num_threads()),
        }
    }
}

/// Returns `os`, or `browser` for the empty name that wasm32-unknown-unknown reports. Only a browser runs that target
/// here.
fn os_name(os: &'static str) -> &'static str {
    if os.is_empty() { "browser" } else { os }
}

#[cfg(not(target_arch = "wasm32"))]
fn logical_cpus() -> Option<usize> {
    std::thread::available_parallelism()
        .ok()
        .map(std::num::NonZeroUsize::get)
}

/// Returns the number of logical CPUs that the browser reports, since `available_parallelism` is unsupported on wasm.
#[cfg(target_arch = "wasm32")]
fn logical_cpus() -> Option<usize> {
    let cores = web_sys::window()?.navigator().hardware_concurrency();
    (cores >= 1.0).then_some(cores as usize)
}

/// Host and adapter facts for one device.
#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    /// Facts about the host.
    pub host: HostInfo,
    /// Adapter the device came from.
    pub adapter: wgpu::AdapterInfo,
    /// Limits the device was created with, after `gpu::limits::raise`.
    pub granted: wgpu::Limits,
    /// Limits the adapter would have allowed, so a gap is headroom left unclaimed.
    pub available: wgpu::Limits,
    /// Whether the device granted `TIMESTAMP_QUERY`.
    pub timestamp_query: bool,
    /// Whether a vertex shader can read a storage buffer. Drawing network edges requires this.
    pub vertex_storage: bool,
}

impl RuntimeInfo {
    /// Collects the facts of `adapter`, `device` and the host. Call it once the thread pool exists, as
    /// [`HostInfo::collect`] says.
    pub fn collect(adapter: &wgpu::Adapter, device: &wgpu::Device) -> Self {
        Self {
            host: HostInfo::collect(),
            adapter: adapter.get_info(),
            granted: device.limits(),
            available: adapter.limits(),
            timestamp_query: device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
            vertex_storage: adapter
                .get_downlevel_capabilities()
                .flags
                .contains(wgpu::DownlevelFlags::VERTEX_STORAGE),
        }
    }

    /// Longest side of a display texture on this device, after Henad's own cap.
    pub fn display_cap(&self) -> u32 {
        crate::display_scale::MAX_DISPLAY_DIM.min(self.granted.max_texture_dimension_2d)
    }
}

/// The adapter's fitness for Henad's workload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuVerdict {
    /// A discrete GPU.
    Capable,
    /// Any other GPU, which might be anything from a low-end integrated GPU to something like an M4 Pro.
    Uncertain,
    /// A CPU adapter, with no GPU behind it.
    Absent,
}

/// Returns whether the adapter can run compute shaders. Every backend but `Gl` can, since WebGL2 has no compute stage.
pub fn supports_compute(info: &wgpu::AdapterInfo) -> bool {
    info.backend != wgpu::Backend::Gl
}

/// Returns the adapter's [`GpuVerdict`].
///
/// `DeviceType` describes memory topology and says nothing of speed, so only `DiscreteGpu` counts as
/// [`GpuVerdict::Capable`].
pub fn classify_adapter(info: &wgpu::AdapterInfo) -> GpuVerdict {
    if info.device_type == wgpu::DeviceType::Cpu {
        GpuVerdict::Absent
    } else if info.device_type == wgpu::DeviceType::DiscreteGpu {
        GpuVerdict::Capable
    } else {
        GpuVerdict::Uncertain
    }
}

#[cfg(test)]
mod tests {
    use super::{HostInfo, os_name};

    /// wasm32-unknown-unknown reports an empty name. Left alone, the System tab reads " (wasm32)".
    #[test]
    fn an_unnamed_os_reads_as_browser() {
        assert_eq!(os_name(""), "browser");
        assert_eq!(os_name("macos"), "macos");
        assert!(!HostInfo::collect().os.is_empty());
    }
}
