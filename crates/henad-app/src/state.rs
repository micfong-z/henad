//! Panel state, split from `HenadApp` so `DockArea::show` can borrow the dock and the
//! `TabViewer` at once.

use egui::TextureHandle;
use henad_compute::sim_thread::{SimCommand, SimThread};
use henad_compute::snapshot::Snapshot;
use henad_core::params::ParamValue;
use henad_core::view::StatsHistory;
use henad_models::registry::{ModelEntry, ModelState, model_registry};

use crate::sim_runner::SimRunner;
use henad_compute::runtime_info::RuntimeInfo;

use henad_compute::gpu::GpuContext;
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::timing::{DEFAULT_BATCH_SIZE, DEFAULT_TARGET_MS};

/// Exponential moving average smoothing factor (0..1, higher = more responsive).
#[cfg(not(target_arch = "wasm32"))]
const EMA_ALPHA: f64 = 0.1;

/// Per-frame timing breakdown, smoothed with EMA.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct FrameTimings {
    pub render_ms: f64,
    pub ui_ms: f64,
    /// Raw viewport cost this frame; folded into the EMAs once the frame is over.
    pub frame_render_ms: f64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FrameTimings {
    pub fn update_ema(smoothed: &mut f64, sample_ms: f64) {
        *smoothed += EMA_ALPHA * (sample_ms - *smoothed);
    }
}

pub struct AppState {
    pub registry: Vec<ModelEntry>,
    pub selected_model: usize,
    pub param_values: Vec<ParamValue>,
    pub loaded_model: Option<usize>,
    pub pending_reload: Vec<bool>,
    pub sim_thread: Option<SimRunner>,
    pub snapshot: Option<Snapshot>,
    pub sim_running: bool,
    pub grid_texture: Option<TextureHandle>,
    pub density_max: f32,
    pub last_rendered_tick: Option<u64>,
    pub rendering_enabled: bool,
    pub target_tps: f64,
    pub uncapped: bool,
    pub ticks_per_snapshot: u32,
    pub stats_history: Option<StatsHistory>,
    pub history_capacity: usize,
    /// Fixed for the life of the process; collected once at startup.
    pub runtime: RuntimeInfo,
    #[cfg(not(target_arch = "wasm32"))]
    pub timings: FrameTimings,
    /// The injected device/queue, kept so a GPU model can be rebuilt on every Reset / model
    /// switch. `None` on the web build.
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_ctx: Option<GpuContext>,
    /// GPU batching controls
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_adaptive: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_target_ms: f64,
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_batch_size: u32,
}

impl AppState {
    /// `gpu_ctx` is `None` on web, which keeps GPU models out of the registry.
    pub fn new(gpu_ctx: Option<GpuContext>, runtime: RuntimeInfo) -> Self {
        let registry = model_registry(gpu_ctx.clone());
        let param_values: Vec<ParamValue> = registry
            .first()
            .map(|m| m.param_descriptors.iter().map(|p| p.kind.default_value()).collect())
            .unwrap_or_default();

        Self {
            pending_reload: vec![false; param_values.len()],
            registry,
            selected_model: 0,
            param_values,
            loaded_model: None,
            sim_thread: None,
            snapshot: None,
            sim_running: false,
            grid_texture: None,
            density_max: 4.0,
            last_rendered_tick: None,
            rendering_enabled: true,
            target_tps: 30.0,
            uncapped: false,
            ticks_per_snapshot: 1,
            stats_history: None,
            history_capacity: 10_000,
            runtime,
            #[cfg(not(target_arch = "wasm32"))]
            timings: FrameTimings::default(),
            #[cfg(not(target_arch = "wasm32"))]
            gpu_ctx,
            #[cfg(not(target_arch = "wasm32"))]
            gpu_adaptive: true,
            #[cfg(not(target_arch = "wasm32"))]
            gpu_target_ms: DEFAULT_TARGET_MS,
            #[cfg(not(target_arch = "wasm32"))]
            gpu_batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    pub fn reset_simulation(&mut self) {
        // Drop existing sim thread. For a GPU model this also releases its buffers/pipelines, but any paint callback
        // still in flight this frame holds its own `Arc` to the display, so tearing down mid-frame cannot pull
        // the texture out from under the renderer.
        self.sim_thread = None;
        self.snapshot = None;
        self.last_rendered_tick = None;
        self.grid_texture = None;
        self.density_max = 4.0;
        self.ticks_per_snapshot = 1;
        self.loaded_model = None;

        let Some(entry) = self.registry.get(self.selected_model) else {
            return;
        };

        let stats_history = StatsHistory::new(entry.stat_descriptors.clone(), self.history_capacity);

        match (entry.create)(&self.param_values) {
            ModelState::Cpu(state) => {
                let mut thread = SimThread::new(state, self.target_tps);
                if self.uncapped {
                    thread.send(SimCommand::SetUncapped(true));
                }
                self.sim_thread = Some(SimRunner::Cpu(thread));
            }
            #[cfg(not(target_arch = "wasm32"))]
            ModelState::Gpu(state) => {
                let Some(ctx) = self.gpu_ctx.clone() else {
                    // Unreachable in practice: without a context the registry never offers a GPU
                    // entry to select in the first place.
                    log::error!("a GPU model was selected but no GPU context is available");
                    return;
                };
                let settings = GpuBatchSettings {
                    adaptive: self.gpu_adaptive,
                    batch_size: self.gpu_batch_size,
                    target_ms: self.gpu_target_ms,
                };
                self.sim_thread = Some(SimRunner::Gpu(GpuSimThread::new(ctx, state, settings)));
            }
            #[cfg(target_arch = "wasm32")]
            ModelState::Gpu(_) => {
                log::error!("GPU models are not available on the web build");
                return;
            }
        }

        self.stats_history = Some(stats_history);
        self.sim_running = false;
        self.loaded_model = Some(self.selected_model);
        self.pending_reload = vec![false; self.param_values.len()];
    }

    pub fn offload_simulation(&mut self) {
        self.sim_thread = None;
        self.snapshot = None;
        self.sim_running = false;
        self.grid_texture = None;
        self.last_rendered_tick = None;
        self.stats_history = None;
        self.loaded_model = None;
    }

    pub fn load_default_params(&mut self) {
        let Some(entry) = self.registry.get(self.selected_model) else {
            return;
        };
        self.param_values = entry.param_descriptors.iter().map(|p| p.kind.default_value()).collect();
        self.pending_reload = vec![false; self.param_values.len()];
    }

    pub fn selection_is_loaded(&self) -> bool {
        self.loaded_model == Some(self.selected_model)
    }

    pub fn is_gpu(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sim_thread.as_ref().is_some_and(|t| t.gpu_stats().is_some())
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }
}
