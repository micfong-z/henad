use crate::icons::material_design_icons::{MDI_PAUSE, MDI_PLAY, MDI_RESTART, MDI_SKIP_NEXT, MDI_TRAY_REMOVE};
use crate::state::AppState;

pub fn playback_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let has_thread = app.sim_thread.is_some();

    ui.horizontal(|ui| {
        let icon = if app.sim_running { MDI_PAUSE } else { MDI_PLAY };

        if ui
            .add_enabled(has_thread, egui::Button::new(icon))
            .on_hover_text("Play / Pause")
            .clicked()
            && let Some(thread) = &mut app.sim_thread
        {
            if app.sim_running {
                thread.pause();
            } else {
                thread.play();
            }
            app.sim_running = !app.sim_running;
        }

        if ui
            .add_enabled(has_thread, egui::Button::new(MDI_SKIP_NEXT))
            .on_hover_text("Step")
            .clicked()
            && let Some(thread) = &mut app.sim_thread
        {
            thread.step_once();
        }
    });

    ui.separator();

    let shortfalls = app.selection_shortfalls();

    ui.horizontal(|ui| {
        // Past the device's limits wgpu panics on this very thread, so refuse rather than try.
        let build = ui.add_enabled(shortfalls.is_empty(), egui::Button::new(format!("{MDI_RESTART} Build")));
        let build = if shortfalls.is_empty() {
            build.on_hover_text("Build the selected model from given parameters, replacing any running simulation")
        } else {
            build.on_disabled_hover_text(format!("Too large for this device: {}", shortfalls.join("; ")))
        };
        if build.clicked() {
            app.reset_simulation();
        }
        if ui
            .add_enabled(has_thread, egui::Button::new(format!("{MDI_TRAY_REMOVE} Offload")))
            .on_hover_text("Free simulation memory")
            .clicked()
        {
            app.offload_simulation();
        }
    });
}
