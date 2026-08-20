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

pub use crate::init::wgpu_configuration;

use crate::state::AppState;
use crate::ui::dock::{Tab, default_dock_state};
use henad_compute::fault::{FaultSink, install_panic_hook};
use henad_compute::runtime_info::{RuntimeInfo, supports_compute};
/// Re-exported so wasm-bindgen emits the worker glue `wasm_bindgen_rayon` builds its pool from.
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;

/// Pool width asked for by a `?threads=N` query string, clamped to what the host offers.
///
/// `?threads=1` is how the threaded build gets compared against no pool at all, without keeping a
/// second build around to compare against.
pub fn requested_threads(search: &str, available: usize) -> usize {
    let available = available.max(1);
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("threads="))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(available)
        .clamp(1, available)
}

use crate::state::FrameTimings;

pub struct HenadApp {
    dock: DockState<Tab>,
    state: AppState,
}

impl HenadApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_panic_hook();

        let render_state = &cc
            .wgpu_render_state
            .as_ref()
            .expect("wgpu_render_state must exist for wgpu backend");

        let adapter_info = render_state.adapter.get_info();
        log::info!("{}", egui_wgpu::adapter_info_summary(&adapter_info));

        // egui's `RenderState` is the sole authority on device acquisition; `henad-compute` never
        // creates a device, it only ever receives cloned handles. Building the context also takes
        // the device's error handling off wgpu's fatal default.
        let render_ctx = henad_compute::gpu::GpuContext::new(
            render_state.device.clone(),
            render_state.queue.clone(),
            render_state.target_format,
            FaultSink::new(),
        );

        let gpu_ctx = supports_compute(&adapter_info).then(|| render_ctx.clone());

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

        if let Some(fault) = self.state.render_ctx.faults.take() {
            self.state.report_fault(fault);
        }

        // Request continuous repaint while running.
        if self.state.sim_running {
            ctx.request_repaint_after(std::time::Duration::ZERO);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_start = web_time::Instant::now();
        self.state.timings.frame_render_ms = 0.0;

        ui::menu_bar::menu_bar_panel(ui, &mut self.dock);
        ui::fault::fault_modal(ui.ctx(), &mut self.state);

        let dock_style = Style::from_egui(ui.style());
        DockArea::new(&mut self.dock)
            .style(dock_style)
            .show_close_buttons(true)
            .show_leaf_close_all_buttons(true)
            .show_inside(ui, &mut self.state);

        // The viewport tab times itself. Whatever is left over is UI.
        let total_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        let render_ms = self.state.timings.frame_render_ms;
        FrameTimings::update_ema(&mut self.state.timings.render_ms, render_ms);
        FrameTimings::update_ema(&mut self.state.timings.ui_ms, (total_ms - render_ms).max(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::requested_threads;

    #[test]
    fn an_absent_query_asks_for_every_core() {
        assert_eq!(requested_threads("", 14), 14);
        assert_eq!(requested_threads("?debug=1", 14), 14);
    }

    #[test]
    fn threads_one_is_how_a_single_threaded_run_is_asked_for() {
        assert_eq!(requested_threads("?threads=1", 14), 1);
        assert_eq!(requested_threads("?foo=a&threads=4", 14), 4);
    }

    /// More workers than cores only adds contention, and zero would build no pool at all.
    #[test]
    fn a_request_is_clamped_to_the_host() {
        assert_eq!(requested_threads("?threads=99", 14), 14);
        assert_eq!(requested_threads("?threads=0", 14), 1);
        assert_eq!(requested_threads("?threads=nonsense", 14), 14);
    }
}
