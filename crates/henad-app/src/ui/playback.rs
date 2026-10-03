use crate::icons::material_design_icons::{
    MDI_FAST_FORWARD, MDI_FLASK_OUTLINE, MDI_PAUSE, MDI_PLAY, MDI_RESTART, MDI_SKIP_NEXT, MDI_TRAY_REMOVE,
};
use crate::state::{AppState, setup_message};
use crate::ui::params::{INVALID_SEED, parse_seed};
use crate::ui::{add_progress_bar, mcs};

/// Reason Build gives with no model selected.
const NO_SELECTED_MODEL: &str = "Select a model in the Model tab";

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
            app.run_to_target = None;
        }

        if ui
            .add_enabled(has_thread, egui::Button::new(MDI_SKIP_NEXT))
            .on_hover_text("Step")
            .clicked()
            && let Some(thread) = &mut app.sim_thread
        {
            thread.step_once();
            app.run_to_target = None;
        }
    });

    let shortfalls = app.selection_shortfalls();
    let build_disabled_reason = build_refusal(app, &shortfalls);

    run_to_row(ui, app, build_disabled_reason.as_deref());

    ui.separator();

    ui.horizontal(|ui| {
        // Past the device's limits wgpu panics on this very thread, so refuse rather than try.
        let build = ui.add_enabled(
            build_disabled_reason.is_none(),
            egui::Button::new(format!("{MDI_RESTART} Build")),
        );
        let build = match &build_disabled_reason {
            None => {
                build.on_hover_text("Build the selected model from given parameters, replacing any running simulation")
            }
            Some(reason) => build.on_disabled_hover_text(reason.as_str()),
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

    opened_run_line(ui, app);
}

/// Returns the reason the selection cannot be built, or `None` when it can.
fn build_refusal(app: &AppState, shortfalls: &[String]) -> Option<String> {
    if app.selected_entry().is_none() {
        return Some(NO_SELECTED_MODEL.to_owned());
    }
    if !shortfalls.is_empty() {
        return Some(format!("Too large for this device: {}", shortfalls.join("; ")));
    }
    if parse_seed(&app.seed_text).is_err() {
        return Some(INVALID_SEED.to_owned());
    }
    app.build_setup()?.err().map(|error| setup_message(&error))
}

/// Draws the Run to tick row, or the progress of a pending run to a tick with its Cancel button.
fn run_to_row(ui: &mut egui::Ui, app: &mut AppState, build_disabled_reason: Option<&str>) {
    let current = app.snapshot.as_ref().map_or(0, |snap| snap.tick);

    if let Some(target) = app.run_to_target {
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        // Right to left, so the bar takes the width Cancel leaves.
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").on_hover_text("Stop at current tick").clicked()
                && let Some(thread) = &mut app.sim_thread
            {
                thread.pause();
                app.run_to_target = None;
            }
            let progress = if target == 0 {
                1.0
            } else {
                (current as f64 / target as f64) as f32
            };
            let bar = egui::ProgressBar::new(progress)
                .text(format!("Running to tick {target}"))
                .corner_radius(0);
            add_progress_bar(ui, bar, mcs::BLUE_700);
        });
        return;
    }

    ui.horizontal(|ui| {
        let label = ui.label("Run to tick");
        ui.add(egui::DragValue::new(&mut app.run_to_input))
            .labelled_by(label.id);
        let run_disabled_reason = run_refusal(app, build_disabled_reason, current);
        let run = ui
            .add_enabled(
                app.sim_thread.is_some() && run_disabled_reason.is_none(),
                egui::Button::new(format!("{MDI_FAST_FORWARD} Run")),
            )
            .on_hover_text(
                "Step as fast as possible to this tick and pause. For a tick behind the current one, the model will \
                 be rebuilt.",
            );
        let run = match run_disabled_reason {
            Some(reason) => run.on_disabled_hover_text(reason),
            None => run,
        };
        if run.clicked() {
            app.run_to(app.run_to_input);
        }
    });
}

/// Returns the reason Run cannot run the loaded model to the tick in the Run to tick field, or `None` when it can.
fn run_refusal(app: &AppState, build_disabled_reason: Option<&str>, current: u64) -> Option<String> {
    // While the sim plays, the loop runs ahead of `current` and might be past the tick already.
    if app.sim_running {
        return Some("Pause to run to a tick".to_owned());
    }
    if app.loaded_model.is_some() && !app.selection_is_loaded() {
        return Some(format!(
            "Selected model not loaded. Press {MDI_RESTART}\u{a0}Build first."
        ));
    }
    // Only a tick behind the current one rebuilds, and only then can the build be refused.
    build_disabled_reason
        .filter(|_| app.run_to_input < current)
        .map(str::to_owned)
}

/// Draws the line naming the sweep run the loaded model replays.
fn opened_run_line(ui: &mut egui::Ui, app: &AppState) {
    let Some(run) = &app.opened_run else {
        return;
    };
    let modified = if run.modified { " (modified)" } else { "" };
    ui.separator();
    ui.label(format!("{MDI_FLASK_OUTLINE} {}{modified}", run.replay.label))
        .on_hover_text(format!(
            "Seed {}. Scheduled actions run on the recorded ticks.",
            run.replay.seed
        ));
}

#[cfg(test)]
mod tests {
    use henad_compute::entry::ModelSet;
    use henad_core::metadata::Backend;

    use super::{NO_SELECTED_MODEL, build_refusal};
    use crate::state::AppState;

    #[test]
    fn build_is_refused_with_no_model_selected() {
        let mut models = ModelSet::new(henad_core::build_info!());
        for entry in henad_models::example_models().iter() {
            if entry.metadata().backend == Backend::Gpu {
                models.insert(entry.clone()).expect("example ids are unique");
            }
        }
        let Some(app) = AppState::headless(models, false) else {
            return;
        };
        assert_eq!(build_refusal(&app, &[]).as_deref(), Some(NO_SELECTED_MODEL));
    }
}
