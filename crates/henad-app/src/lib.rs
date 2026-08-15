//! The Henad GUI application.

/// Rust generated from this crate's WGSL by `wgsl_bindgen`, in `build.rs`.
///
/// Generated code is not held to the workspace's lints, hence the group allows. `unsafe_code` is
/// the one that matters. The generator writes `unsafe impl bytemuck::Pod` and an
/// `unsafe fn from_raw`, so the workspace deny is lifted here and nowhere else.
#[allow(
    unsafe_code,
    dead_code,
    elided_lifetimes_in_paths,
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery
)]
mod shader_bindings {
    include!(concat!(env!("OUT_DIR"), "/shader_bindings.rs"));
}

mod icons;
mod init;
mod sim_runner;
pub mod state;
pub mod ui;

use eframe::egui_wgpu;
use egui_dock::{DockArea, DockState, Style};

use crate::init::{setup_custom_fonts, setup_custom_styles};
use crate::state::AppState;
use crate::ui::dock::{Tab, default_dock_state};
use henad_compute::runtime_info::RuntimeInfo;

#[cfg(not(target_arch = "wasm32"))]
use crate::state::FrameTimings;

pub struct HenadApp {
    dock: DockState<Tab>,
    state: AppState,
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
        // creates a device, it only ever receives cloned handles.
        let render_ctx = henad_compute::gpu::GpuContext::new(
            render_state.device.clone(),
            render_state.queue.clone(),
            render_state.target_format,
        );

        // Rendering always has a device, but GPU models stay native-only. On wasm we hand the
        // registry no context, so they are simply absent from the web build.
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_ctx = Some(render_ctx.clone());
        #[cfg(target_arch = "wasm32")]
        let gpu_ctx: Option<henad_compute::gpu::GpuContext> = None;

        setup_custom_fonts(&cc.egui_ctx);
        setup_custom_styles(&cc.egui_ctx);

        Self {
            dock: default_dock_state(),
            state: AppState::new(
                cc.egui_ctx.clone(),
                render_ctx,
                gpu_ctx,
                RuntimeInfo::collect(&render_state.adapter, &render_state.device),
            ),
        }
    }
}

impl eframe::App for HenadApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // --- On WASM: drive simulation synchronously ---
        #[cfg(target_arch = "wasm32")]
        {
            let dt = ctx.input(|i| i.unstable_dt as f64);
            if let Some(thread) = &mut self.state.sim_thread {
                thread.update(dt);
            }
        }

        // --- Poll snapshot from sim thread ---
        if let Some(thread) = &mut self.state.sim_thread
            && let Some(snap) = thread.take_snapshot()
        {
            if let Some(history) = &mut self.state.stats_history {
                let values: Vec<f64> = snap.stats.iter().map(|s| s.value.scalar()).collect();
                history.push(&values, snap.tick);
            }
            // Handing the outgoing one back lets the sim thread refill it instead of allocating.
            if let Some(previous) = self.state.snapshot.replace(snap) {
                thread.recycle(previous);
            }
        }

        // Request continuous repaint while running.
        if self.state.sim_running {
            ctx.request_repaint_after(std::time::Duration::ZERO);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(not(target_arch = "wasm32"))]
        let frame_start = std::time::Instant::now();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.state.timings.frame_render_ms = 0.0;
        }

        ui::menu_bar::menu_bar_panel(ui, &mut self.dock);

        let dock_style = Style::from_egui(ui.style());
        DockArea::new(&mut self.dock)
            .style(dock_style)
            .show_close_buttons(true)
            .show_leaf_close_all_buttons(true)
            .show_inside(ui, &mut self.state);

        // The viewport tab times itself. Whatever is left over is UI.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let total_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
            let render_ms = self.state.timings.frame_render_ms;
            FrameTimings::update_ema(&mut self.state.timings.render_ms, render_ms);
            FrameTimings::update_ema(&mut self.state.timings.ui_ms, (total_ms - render_ms).max(0.0));
        }
    }
}
