//! The Henad GUI application.

henad_compute::include_shaders!();

mod icons;
mod init;
mod sim_runner;
pub mod state;
pub mod ui;

use eframe::egui_wgpu;
use egui_dock::{DockArea, DockState, Style};

use crate::init::{setup_custom_fonts, setup_custom_styles};

pub use crate::init::wgpu_configuration;

use crate::sim_runner::SimRunner;
use crate::state::AppState;
use crate::ui::dock::{Tab, default_dock_state, focus_tab};
use henad_compute::entry::ModelSet;
use henad_compute::fault::{FaultSink, install_panic_hook};
use henad_compute::runner::CAN_SPAWN_THREADS;
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

/// Longest time between two repaints while a sweep runs on a thread of its own.
const SWEEP_REPAINT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

pub struct HenadApp {
    dock: DockState<Tab>,
    state: AppState,
}

impl HenadApp {
    /// Returns the app offering `models`, on the device eframe created from [`wgpu_configuration`].
    ///
    /// Note that the device has to be requested for `models.gpu_needs()`. Otherwise a GPU model that binds more
    /// storage buffers than the WebGPU baseline allows will fail to build.
    pub fn new(cc: &eframe::CreationContext<'_>, models: ModelSet) -> Self {
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
                models,
                render_ctx,
                gpu_ctx,
                RuntimeInfo::collect(&render_state.adapter, &render_state.device),
            ),
        }
    }

    /// Reads the results a sweep wrote to `folder`, and shows them in the Results tab once read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_results(&mut self, folder: std::path::PathBuf) {
        ui::results::open_folder(&mut self.state, folder);
        self.state.focus_request = Some(Tab::Results);
    }
}

impl eframe::App for HenadApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Advances the sim where it has no thread of its own. A no-op where it has.
        let dt = ctx.input(|i| f64::from(i.unstable_dt));
        if let Some(thread) = &mut self.state.sim_thread {
            thread.update(dt);
        }
        ui::sweep::update(&mut self.state, dt);

        // --- Poll snapshot from sim thread ---
        let fresh = self.state.sim_thread.as_mut().and_then(SimRunner::take_snapshot);
        if let Some(snap) = fresh {
            // The loop can be past the target by the time it handles `RunTo`, and then pauses where it is. A second
            // run to the target sees this snapshot's tick, rebuilds, and stops there.
            let passed_target = self
                .state
                .run_to_target
                .filter(|&target| snap.tick > target && self.state.selection_is_loaded());
            if self.state.run_to_target.is_some_and(|target| snap.tick >= target) {
                self.state.run_to_target = None;
            }
            // A publish at the newest row's tick replaces that row. After an action it carries the
            // action's effect, and after a paused layout step the same stats again.
            if let Some(history) = &mut self.state.stats_history {
                history.push_entries(&snap.stats, snap.tick);
            }
            self.state.record(&snap);
            // Handing the outgoing one back lets the sim thread refill it instead of allocating.
            if let Some(previous) = self.state.snapshot.replace(snap)
                && let Some(thread) = &mut self.state.sim_thread
            {
                thread.recycle(previous);
            }
            if let Some(target) = passed_target {
                self.state.run_to(target);
            }
        }

        if let Some(fault) = self.state.render_ctx.faults.take() {
            self.state.report_fault(fault);
        }

        self.state.poll_saves();
        self.state.poll_opens();
        ui::results::poll(ctx, &mut self.state);
        self.state.poll_capture();

        // Request continuous repaint while running. A run to a tick wakes the UI on each publish, and needs the
        // repaint only where the frame steps the sim.
        let frame_drives_sim = !CAN_SPAWN_THREADS;
        if self.state.sim_running || (frame_drives_sim && self.state.run_to_target.is_some()) {
            ctx.request_repaint_after(std::time::Duration::ZERO);
        }
        // A sweep wakes the UI on each finished run. Between runs only the clock and the ticks move.
        if frame_drives_sim && self.state.sweep.is_stepping() {
            ctx.request_repaint_after(std::time::Duration::ZERO);
        } else if self.state.sweep.is_running() {
            ctx.request_repaint_after(SWEEP_REPAINT_INTERVAL);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let frame_start = web_time::Instant::now();
        self.state.timings.frame_render_ms = 0.0;

        ui::menu_bar::menu_bar_panel(ui, &mut self.dock, &mut self.state);
        ui::fault::fault_modal(ui.ctx(), &mut self.state);
        ui::about::about_modal(ui.ctx(), &mut self.state);

        let mut dock_style = Style::from_egui(ui.style());
        // `from_egui` adds 2 to the widget radius for the tab bar's top corners.
        dock_style.tab_bar.corner_radius = egui::CornerRadius::ZERO;
        // `egui_dock` draws an overflowing tab bar's scroll bar as a pill. The wheel still scrolls the bar without it.
        dock_style.tab_bar.show_scroll_bar_on_overflow = false;
        DockArea::new(&mut self.dock)
            .style(dock_style)
            .show_close_buttons(true)
            .show_leaf_close_all_buttons(true)
            .show_inside(ui, &mut self.state);

        // Applied after the dock has drawn. The panels draw while it is borrowed.
        if let Some(tab) = self.state.focus_request.take() {
            focus_tab(&mut self.dock, tab);
            ui.ctx().request_repaint();
        }

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
