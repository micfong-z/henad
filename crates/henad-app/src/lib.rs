//! Henad Engine — GUI application.

mod icons;
mod init;
pub mod ui;

use egui::TextureHandle;
use henad_compute::runner::SimRunner;
use henad_core::{params::ParamValue, topology::TopologyHint};
use henad_models::registry::{ModelEntry, model_registry};

use crate::init::{setup_custom_fonts, setup_custom_styles};

/// Exponential moving average smoothing factor (0..1, higher = more responsive).
#[cfg(not(target_arch = "wasm32"))]
const EMA_ALPHA: f64 = 0.1;

/// Per-frame timing breakdown, smoothed with EMA.
#[cfg(not(target_arch = "wasm32"))]
pub struct FrameTimings {
    pub sim_ms: f64,
    pub render_ms: f64,
    pub ui_ms: f64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for FrameTimings {
    fn default() -> Self {
        Self {
            sim_ms: 0.0,
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
    runner: Option<SimRunner>,
    grid_texture: Option<TextureHandle>,
    pixel_buf: Vec<u8>,
    density_max: f32,
    /// Tick at which we last uploaded the grid texture. Used to skip
    /// the expensive pixel conversion + texture upload when nothing changed.
    last_rendered_tick: Option<u64>,
    /// When false, skip pixel conversion and texture upload entirely.
    pub rendering_enabled: bool,
    pub target_tps: f64,
    pub uncapped: bool,
    pub history_capacity: usize,
    #[cfg(not(target_arch = "wasm32"))]
    pub timings: FrameTimings,
}

impl HenadApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        setup_custom_styles(&cc.egui_ctx);

        let registry = model_registry();
        let param_values: Vec<ParamValue> = registry
            .first()
            .map(|m| {
                m.param_descriptors
                    .iter()
                    .map(|p| p.kind.default_value())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            registry,
            selected_model: 0,
            param_values,
            runner: None,
            grid_texture: None,
            pixel_buf: Vec::new(),
            density_max: 4.0,
            last_rendered_tick: None,
            rendering_enabled: true,
            target_tps: 30.0,
            uncapped: false,
            history_capacity: 10_000,
            #[cfg(not(target_arch = "wasm32"))]
            timings: FrameTimings::default(),
        }
    }

    fn reset_simulation(&mut self) {
        if let Some(entry) = self.registry.get(self.selected_model) {
            let state = (entry.create)(&self.param_values);
            let mut runner = SimRunner::new(state, self.target_tps);
            runner.set_uncapped(self.uncapped);
            runner.state_mut().resize_history(self.history_capacity);
            self.runner = Some(runner);
            self.grid_texture = None;
            self.density_max = 4.0;
            self.pixel_buf.clear();
            self.last_rendered_tick = None;
        }
    }

    fn offload_simulation(&mut self) {
        self.runner = None;
        self.grid_texture = None;
        self.pixel_buf = Vec::new();
        self.last_rendered_tick = None;
    }

    pub(crate) fn selected_topology_hint(&self) -> Option<TopologyHint> {
        self.registry
            .get(self.selected_model)
            .map(|entry| entry.topology_hint)
    }
}

impl eframe::App for HenadApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- Simulation ---
        #[cfg(not(target_arch = "wasm32"))]
        let sim_start = std::time::Instant::now();

        let dt = ctx.input(|i| i.unstable_dt as f64);
        if let Some(runner) = &mut self.runner {
            runner.update(dt);
        }

        #[cfg(not(target_arch = "wasm32"))]
        FrameTimings::update_ema(
            &mut self.timings.sim_ms,
            sim_start.elapsed().as_secs_f64() * 1000.0,
        );

        // --- UI ---
        #[cfg(not(target_arch = "wasm32"))]
        let ui_start = std::time::Instant::now();

        ui::toolbar::toolbar_panel(ctx, self);
        ui::sidebar::sidebar_panel(ctx, self);

        #[cfg(not(target_arch = "wasm32"))]
        FrameTimings::update_ema(
            &mut self.timings.ui_ms,
            ui_start.elapsed().as_secs_f64() * 1000.0,
        );

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
        // Checked after UI panels so toolbar Play/Pause toggle is reflected immediately.
        if self.runner.as_ref().is_some_and(SimRunner::is_running) {
            ctx.request_repaint_after(std::time::Duration::ZERO);
        }
    }
}
