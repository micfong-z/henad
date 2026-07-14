//! Henad Engine — GUI application.

mod icons;
mod init;
mod sim_runner;
pub mod ui;

use eframe::egui_wgpu;
use egui::TextureHandle;
use henad_compute::sim_thread::{SimCommand, SimThread};
use henad_compute::snapshot::Snapshot;
use henad_core::params::ParamValue;
use henad_core::view::StatsHistory;
use henad_models::registry::{ModelEntry, ModelState, model_registry};

use crate::init::{setup_custom_fonts, setup_custom_styles};
use crate::sim_runner::SimRunner;

#[cfg(not(target_arch = "wasm32"))]
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
pub struct FrameTimings {
    pub render_ms: f64,
    pub ui_ms: f64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for FrameTimings {
    fn default() -> Self {
        Self {
            render_ms: 0.0,
            ui_ms: 0.0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl FrameTimings {
    fn update_ema(smoothed: &mut f64, sample_ms: f64) {
        *smoothed += EMA_ALPHA * (sample_ms - *smoothed);
    }
}

pub struct HenadApp {
    registry: Vec<ModelEntry>,
    selected_model: usize,
    param_values: Vec<ParamValue>,
    sim_thread: Option<SimRunner>,
    snapshot: Option<Snapshot>,
    sim_running: bool,
    grid_texture: Option<TextureHandle>,
    pixel_buf: Vec<u8>,
    density_max: f32,
    last_rendered_tick: Option<u64>,
    pub rendering_enabled: bool,
    pub target_tps: f64,
    pub uncapped: bool,
    pub ticks_per_snapshot: u32,
    stats_history: Option<StatsHistory>,
    pub history_capacity: usize,
    #[cfg(not(target_arch = "wasm32"))]
    pub timings: FrameTimings,
    /// The injected device/queue, kept so a GPU model can be rebuilt on every Reset / model
    /// switch. `None` on the web build, which is exactly why no GPU model appears in the
    /// dropdown there.
    #[cfg(not(target_arch = "wasm32"))]
    gpu_ctx: Option<GpuContext>,
    /// GPU batching controls. Real fields (not `ctx.data()` temp storage) like every other panel's
    /// state, and the source of truth handed to a freshly spawned GPU thread so its settings
    /// survive a Reset.
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_adaptive: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_target_ms: f64,
    #[cfg(not(target_arch = "wasm32"))]
    pub gpu_batch_size: u32,
}

impl HenadApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = &cc
            .wgpu_render_state
            .as_ref()
            .expect("wgpu_render_state must exist for wgpu backend");

        let adapter_info = render_state.adapter.get_info();
        log::info!("{}", egui_wgpu::adapter_info_summary(&adapter_info));

        // egui's `RenderState` is the sole authority on device acquisition; `henad-compute` never
        // creates a device, it only ever receives cloned handles. On wasm we hand the registry no
        // context at all, so GPU models are simply absent from the web build.
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_ctx = Some(GpuContext::new(
            render_state.device.clone(),
            render_state.queue.clone(),
            render_state.target_format,
        ));
        #[cfg(target_arch = "wasm32")]
        let gpu_ctx: Option<henad_compute::gpu::GpuContext> = None;

        setup_custom_fonts(&cc.egui_ctx);
        setup_custom_styles(&cc.egui_ctx);

        let registry = model_registry(gpu_ctx.clone());
        let param_values: Vec<ParamValue> = registry
            .first()
            .map(|m| m.param_descriptors.iter().map(|p| p.kind.default_value()).collect())
            .unwrap_or_default();

        Self {
            registry,
            selected_model: 0,
            param_values,
            sim_thread: None,
            snapshot: None,
            sim_running: false,
            grid_texture: None,
            pixel_buf: Vec::new(),
            density_max: 4.0,
            last_rendered_tick: None,
            rendering_enabled: true,
            target_tps: 30.0,
            uncapped: false,
            ticks_per_snapshot: 1,
            stats_history: None,
            history_capacity: 10_000,
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

    fn reset_simulation(&mut self) {
        // Drop existing sim thread (sends Shutdown, joins thread). For a GPU model this also
        // releases its buffers/pipelines — but any paint callback still in flight this frame holds
        // its own `Arc` to the display, so tearing down mid-frame cannot pull the texture out from
        // under the renderer.
        self.sim_thread = None;
        self.snapshot = None;
        self.last_rendered_tick = None;
        self.grid_texture = None;
        self.pixel_buf.clear();
        self.density_max = 4.0;
        self.ticks_per_snapshot = 1;

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
    }

    fn offload_simulation(&mut self) {
        self.sim_thread = None;
        self.snapshot = None;
        self.sim_running = false;
        self.grid_texture = None;
        self.pixel_buf = Vec::new();
        self.last_rendered_tick = None;
        self.stats_history = None;
    }
}

impl eframe::App for HenadApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- On WASM: drive simulation synchronously ---
        #[cfg(target_arch = "wasm32")]
        {
            let dt = ctx.input(|i| i.unstable_dt as f64);
            if let Some(thread) = &mut self.sim_thread {
                thread.update(dt);
            }
        }

        // --- Poll snapshot from sim thread ---
        if let Some(thread) = &mut self.sim_thread {
            if let Some(snap) = thread.take_snapshot() {
                if let Some(history) = &mut self.stats_history {
                    let values: Vec<f64> = snap.stats.iter().map(|s| s.value.scalar()).collect();
                    history.push(&values, snap.tick);
                }
                self.snapshot = Some(snap);
            }
        }

        // --- UI ---
        #[cfg(not(target_arch = "wasm32"))]
        let ui_start = std::time::Instant::now();

        ui::toolbar::toolbar_panel(ctx, self);
        ui::sidebar::sidebar_panel(ctx, self);

        #[cfg(not(target_arch = "wasm32"))]
        FrameTimings::update_ema(&mut self.timings.ui_ms, ui_start.elapsed().as_secs_f64() * 1000.0);

        // --- Rendering (pixel conversion + texture upload) ---
        #[cfg(not(target_arch = "wasm32"))]
        let render_start = std::time::Instant::now();

        ui::viewport::viewport_panel(ctx, self);

        #[cfg(not(target_arch = "wasm32"))]
        FrameTimings::update_ema(
            &mut self.timings.render_ms,
            render_start.elapsed().as_secs_f64() * 1000.0,
        );

        // Request continuous repaint while running.
        if self.sim_running {
            ctx.request_repaint_after(std::time::Duration::ZERO);
        }
    }
}
