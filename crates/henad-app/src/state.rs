//! Panel state, split from `HenadApp` so `DockArea::show` can borrow the dock and the
//! `TabViewer` at once.

use std::sync::Arc;

use egui::TextureHandle;
use henad_compute::cpu::sim_thread::{SimCommand, SimThread, WakeFn};
use henad_compute::fault::{BUILDING, Fault, catching};
use henad_compute::snapshot::Snapshot;
use henad_core::params::ParamValue;
use henad_core::view::StatsHistory;
use henad_models::registry::{ModelEntry, ModelState, model_registry};

use crate::sim_runner::SimRunner;
use crate::ui::agent_layer::AgentLayer;
use henad_compute::runtime_info::RuntimeInfo;

use henad_compute::gpu::GpuContext;
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::fault::catching_on;
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::timing::{DEFAULT_BATCH_SIZE, DEFAULT_TARGET_MS};

/// Exponential moving average smoothing factor (0..1, higher = more responsive).
const EMA_ALPHA: f64 = 0.1;

/// Per-frame timing breakdown, smoothed with EMA.
#[derive(Default)]
pub struct FrameTimings {
    pub render_ms: f64,
    pub ui_ms: f64,
    /// Raw viewport cost this frame, folded into the EMAs once the frame is over.
    pub frame_render_ms: f64,
}

impl FrameTimings {
    pub fn update_ema(smoothed: &mut f64, sample_ms: f64) {
        *smoothed += EMA_ALPHA * (sample_ms - *smoothed);
    }
}

pub struct AppState {
    /// Kept so a freshly built sim thread can be handed a repaint waker.
    egui_ctx: egui::Context,
    pub registry: Vec<ModelEntry>,
    pub selected_model: usize,
    pub param_values: Vec<ParamValue>,
    pub loaded_model: Option<usize>,
    pub pending_reload: Vec<bool>,
    pub sim_thread: Option<SimRunner>,
    pub snapshot: Option<Snapshot>,
    pub sim_running: bool,
    pub grid_texture: Option<TextureHandle>,
    /// Separate from `grid_texture`, so density mode does not overwrite a composite model's field.
    pub density_texture: Option<TextureHandle>,
    pub density_max: f32,
    pub point_render_mode: PointRenderMode,
    /// Built on first use and kept across model switches, the pipeline is not tied to a model.
    pub agent_layer: Option<AgentLayer>,
    pub last_rendered_tick: Option<u64>,
    pub rendering_enabled: bool,
    pub target_tps: f64,
    pub uncapped: bool,
    pub ticks_per_snapshot: u32,
    pub stats_history: Option<StatsHistory>,
    pub history_capacity: usize,
    /// Fixed for the life of the process, collected once at startup.
    pub runtime: RuntimeInfo,
    /// Device and queue for rendering, so present on every target. Not `gpu_ctx` below, which
    /// gates GPU models and stays native-only. Errors are reported into `faults`.
    pub render_ctx: GpuContext,
    /// The fault being shown, cleared when the user dismisses the modal.
    pub fault: Option<Fault>,
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

/// Draw style for an agent population in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointRenderMode {
    #[default]
    Agents,
    /// Cheaper past roughly a million agents, and readable where sprites would overlap into a mass.
    Density,
}

impl AppState {
    /// `render_ctx` always exists, eframe is wgpu-only here. `gpu_ctx` is `None` on web, which
    /// keeps GPU models out of the registry without affecting rendering.
    pub fn new(
        egui_ctx: egui::Context,
        render_ctx: GpuContext,
        gpu_ctx: Option<GpuContext>,
        runtime: RuntimeInfo,
    ) -> Self {
        let registry = model_registry(gpu_ctx.clone());
        let param_values: Vec<ParamValue> = registry
            .first()
            .map(|m| m.param_descriptors.iter().map(|p| p.kind.default_value()).collect())
            .unwrap_or_default();

        Self {
            egui_ctx,
            pending_reload: vec![false; param_values.len()],
            registry,
            selected_model: 0,
            param_values,
            loaded_model: None,
            sim_thread: None,
            snapshot: None,
            sim_running: false,
            grid_texture: None,
            density_texture: None,
            density_max: 4.0,
            point_render_mode: PointRenderMode::default(),
            agent_layer: None,
            last_rendered_tick: None,
            rendering_enabled: true,
            target_tps: 30.0,
            uncapped: false,
            ticks_per_snapshot: 1,
            stats_history: None,
            history_capacity: 10_000,
            runtime,
            render_ctx,
            fault: None,
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
        drop(self.render_ctx.faults.take());
        self.snapshot = None;
        self.last_rendered_tick = None;
        self.grid_texture = None;
        self.density_texture = None;
        self.density_max = 4.0;
        self.ticks_per_snapshot = 1;
        self.loaded_model = None;
        // The next model may have no agents at all.
        if let Some(layer) = &mut self.agent_layer {
            layer.clear();
        }

        let Some(entry) = self.registry.get(self.selected_model) else {
            return;
        };

        let stats_history = StatsHistory::new(entry.stat_descriptors.clone(), self.history_capacity);

        // Nudges the event loop, so a snapshot produced while idle is picked up next frame instead
        // of waiting for whatever input event happens to arrive.
        let wake: WakeFn = {
            let ctx = self.egui_ctx.clone();
            Arc::new(move || ctx.request_repaint())
        };

        match self.build_runner(entry, &wake) {
            Ok(runner) => self.sim_thread = Some(runner),
            Err(fault) => {
                self.report_fault(fault);
                return;
            }
        }

        self.stats_history = Some(stats_history);
        self.sim_running = false;
        self.loaded_model = Some(self.selected_model);
        self.pending_reload = vec![false; self.param_values.len()];
    }

    /// Builds the selected model and the thread that will drive it.
    ///
    /// # Errors
    ///
    /// If the model's kernels panic, or the GPU refuses to build it, or the model is not compatible with this machine.
    fn build_runner(&self, entry: &ModelEntry, wake: &WakeFn) -> Result<SimRunner, Fault> {
        match (entry.create)(&self.param_values, None)? {
            ModelState::Cpu(state) => catching(BUILDING, || {
                let mut thread = SimThread::new(
                    state,
                    self.target_tps,
                    Some(wake.clone()),
                    self.render_ctx.faults.clone(),
                );
                if self.uncapped {
                    thread.send(SimCommand::SetUncapped(true));
                }
                SimRunner::Cpu(thread)
            }),
            #[cfg(not(target_arch = "wasm32"))]
            ModelState::Gpu(state) => {
                // Unreachable in practice. Without a context the registry never offers a GPU
                // entry to select.
                let Some(ctx) = self.gpu_ctx.clone() else {
                    return Err(Fault::refused(BUILDING, "no GPU context is available"));
                };
                let settings = GpuBatchSettings {
                    adaptive: self.gpu_adaptive,
                    batch_size: self.gpu_batch_size,
                    target_ms: self.gpu_target_ms,
                };
                catching_on(&ctx.device.clone(), BUILDING, || {
                    SimRunner::Gpu(GpuSimThread::new(ctx, state, settings, Some(wake.clone())))
                })
            }
            #[cfg(target_arch = "wasm32")]
            ModelState::Gpu(_) => Err(Fault::refused(
                BUILDING,
                "GPU models are not available in the web build",
            )),
        }
    }

    /// Stops everything and hands the fault to the modal.
    pub fn report_fault(&mut self, fault: Fault) {
        log::error!("{fault}");
        self.offload_simulation();
        self.fault = Some(fault);
    }

    pub fn offload_simulation(&mut self) {
        self.sim_thread = None;
        drop(self.render_ctx.faults.take());
        self.snapshot = None;
        self.sim_running = false;
        self.grid_texture = None;
        self.density_texture = None;
        self.last_rendered_tick = None;
        self.stats_history = None;
        self.loaded_model = None;
        if let Some(layer) = &mut self.agent_layer {
            layer.clear();
        }
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

    /// Reasons this machine cannot build the selection. Always empty for a CPU model.
    pub fn selection_shortfalls(&self) -> Vec<String> {
        self.registry.get(self.selected_model).map_or_else(Vec::new, |entry| {
            entry.shortfalls(&self.param_values, &self.runtime.granted)
        })
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
