use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_FAST_FORWARD, MDI_FLASK_OUTLINE, MDI_GRAPH_OUTLINE, MDI_INFORMATION, MDI_PAUSE, MDI_PLAY,
    MDI_RESTART, MDI_SKIP_NEXT, MDI_TRAY_REMOVE,
};
use crate::state::{AppState, BUILDING_MODEL, DID_NOT_STOP};
use crate::ui::editor::preview::rebuild_preview;
use crate::ui::results::table::INEXACT_REPLAY;
use crate::ui::time_axis::counter_text;
use crate::ui::{add_progress_bar, mcs};

/// Sets `name` as the accessible name of the control `id`, an icon button or a box beside its label.
fn icon_name(ui: &egui::Ui, id: egui::Id, name: &str) {
    ui.ctx().accesskit_node_builder(id, |node| node.set_label(name));
}

pub fn playback_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let has_thread = app.sim_thread.is_some();

    ui.horizontal(|ui| {
        let (icon, name) = if app.sim_running {
            (MDI_PAUSE, "Pause")
        } else {
            (MDI_PLAY, "Play")
        };

        let play = ui.add_enabled(has_thread, egui::Button::new(icon));
        // The button shows an icon alone, so the screen reader takes its name from here.
        icon_name(ui, play.id, name);
        if play.on_hover_text("Play / Pause").clicked() {
            if app.sim_running {
                app.pause_simulation();
            } else {
                app.play();
            }
        }

        let step = ui.add_enabled(has_thread, egui::Button::new(MDI_SKIP_NEXT));
        icon_name(ui, step.id, "Step");
        if step.on_hover_text("Step").clicked() {
            app.step_once();
        }

        if let Some(counter) = tick_counter(app) {
            ui.label(counter);
        }
    });

    // Build replays the start of the Editor's sheet while its preview runs and the selection is unchanged. The
    // selected model's refusal is not Build's reason then.
    let replays_sheet = app.build_replays_preview();
    let build_disabled_reason = if app.is_building() {
        Some(BUILD_RUNNING.to_owned())
    } else if replays_sheet {
        None
    } else {
        app.build_refusal()
    };

    if app.is_building() {
        building_row(ui, app);
    } else {
        run_to_row(ui, app, build_disabled_reason.as_deref());
    }

    ui.separator();

    ui.horizontal(|ui| {
        // A model past the device's limits would fail its build. Build is disabled instead, with the reason as its
        // tooltip.
        let build = ui.add_enabled(
            build_disabled_reason.is_none(),
            egui::Button::new(format!("{MDI_RESTART} Build")),
        );
        let build = match &build_disabled_reason {
            None if replays_sheet => build.on_hover_text(BUILD_SHEET_TOOLTIP),
            None => build.on_hover_text("Build selected model from given parameters, replacing any running simulation"),
            Some(reason) => build.on_disabled_hover_text(reason.as_str()),
        };
        if build.clicked() {
            press_build(app);
        }
        if ui
            .add_enabled(has_thread, egui::Button::new(format!("{MDI_TRAY_REMOVE} Offload")))
            .on_hover_text("Free simulation memory")
            .clicked()
        {
            app.offload_simulation();
        }
    });

    stopping_line(ui, app);
    opened_run_line(ui, app);
    preview_line(ui, app);
}

/// Label of the box of the tick that Run steps to, and the box's accessible name.
const RUN_TO_TICK: &str = "Run to tick";

/// Tooltip of Build while it replays the start of the Editor's sheet.
pub const BUILD_SHEET_TOOLTIP: &str = "Start world again from tick 0, as it started before";

/// Builds the selected model, as Build does. While the preview of the Editor's sheet runs and the selection has not
/// changed since it loaded, builds that preview again at the same seed. The world then starts as it started before.
pub fn press_build(app: &mut AppState) {
    if app.build_replays_preview() {
        rebuild_preview(app);
    } else {
        app.reset_simulation();
    }
}

/// Returns the text of the tick counter beside Play and Step, `None` before the loaded model's first snapshot.
///
/// The counter reads "Tick 48", or "Tick 48 (2 days)" for a model with a time scale, the time in the unit that reads
/// most naturally for it.
pub(crate) fn tick_counter(app: &AppState) -> Option<String> {
    let snapshot = app.snapshot.as_ref()?;
    Some(counter_text(snapshot.tick, app.loaded_time()))
}

/// Hover text of the preview line while the Editor holds the previewed graph or sheet and the preview runs smaller than
/// the model's own size.
pub const PREVIEW_APPLY: &str = "Press Apply in the Editor to run the model at its own size.";

/// Hover text of the preview line while the Editor holds the previewed graph or sheet and the preview runs at the
/// model's own size.
pub const PREVIEW_APPLY_FULL_SIZE: &str = "Press Apply in the Editor to run the model.";

/// Hover text of the preview line once the Editor holds a grid rule or nothing.
pub const PREVIEW_ORPHANED: &str = "Another model is open in the Editor. Press Offload to stop the preview.";

/// Line under the preview of the Editor's sheet while it waits at tick 0.
pub const PREVIEW_WAITS_FOR_PLAY: &str = "Press Play to run the preview.";

/// Draws the line that names the Editor's preview while it runs, and says how to start a sheet's preview that waits.
fn preview_line(ui: &mut egui::Ui, app: &AppState) {
    let (Some(run), Some(entry)) = (app.preview(), app.loaded_entry()) else {
        return;
    };
    ui.separator();
    ui.label(format!(
        "{MDI_GRAPH_OUTLINE} Preview of {} at {}",
        entry.name(),
        run.size_text()
    ))
    .on_hover_text(preview_hover(app));
    if app.previews_editor_sheet() && !app.sim_running && app.current_tick() == 0 {
        ui.weak(format!("{MDI_INFORMATION} {PREVIEW_WAITS_FOR_PLAY}"));
    }
}

/// Returns the hover text of the preview line. Apply runs the graph or the sheet only while the Editor holds one, at
/// its own size where the preview runs smaller.
pub fn preview_hover(app: &AppState) -> &'static str {
    if !app.editor.model.holds_preview_model() {
        PREVIEW_ORPHANED
    } else if app.preview().is_some_and(|run| !run.is_scaled()) {
        PREVIEW_APPLY_FULL_SIZE
    } else {
        PREVIEW_APPLY
    }
}

/// Reason shown for the disabled Build button while a build runs.
const BUILD_RUNNING: &str = "Building model. Press Cancel first.";

/// Draws the status of a build on its own thread, with its Cancel button.
fn building_row(ui: &mut egui::Ui, app: &mut AppState) {
    let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    // Right to left, so Cancel stays at the end of the row as in the Run to row.
    ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Cancel").on_hover_text("Stop waiting for build").clicked() {
            app.offload_simulation();
        }
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.add(egui::Spinner::new());
            ui.label(BUILDING_MODEL);
        });
    });
}

/// Draws the status of a run or a build that did not stop when the app stopped it.
fn stopping_line(ui: &mut egui::Ui, app: &AppState) {
    let Some(status) = app.stopping_status() else {
        return;
    };
    if status == DID_NOT_STOP {
        ui.colored_label(ui.visuals().warn_fg_color, format!("{MDI_ALERT} {status}"));
    } else {
        ui.weak(format!("{MDI_INFORMATION} {status}"));
    }
}

/// Draws the Run to tick row, or the progress of a pending run to a tick with its Cancel button.
fn run_to_row(ui: &mut egui::Ui, app: &mut AppState, build_disabled_reason: Option<&str>) {
    let current = app.current_tick();

    if let Some(target) = app.run_to_target {
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        // Right to left, so the bar takes the width Cancel leaves.
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").on_hover_text("Stop at current tick").clicked() {
                app.pause_simulation();
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
        let label = ui.label(RUN_TO_TICK);
        let field = ui
            .add(egui::DragValue::new(&mut app.run_to_input))
            .labelled_by(label.id);
        icon_name(ui, field.id, RUN_TO_TICK);
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
///
/// Run drives the preview of the Editor's sheet while Build replays it, and a tick behind the current one replays its
/// start first. Behind a graph's preview, Build would build the selection in the preview's place. The reason is then
/// the preview line's hover text, as [`preview_hover`] returns it.
pub(crate) fn run_refusal(app: &AppState, build_disabled_reason: Option<&str>, current: u64) -> Option<String> {
    // While the sim plays, the loop runs ahead of `current` and might be past the tick already.
    if app.sim_running {
        return Some("Pause to run to a tick".to_owned());
    }
    if app.code_pending() {
        return Some(format!("Model code changed. Press {MDI_RESTART}\u{a0}Build first."));
    }
    if app.loaded_entry().is_some() && !app.selection_is_loaded() && !app.build_replays_preview() {
        // Build would build the model selected before the preview loaded, and end the preview.
        if app.params_drive_preview() {
            return Some(preview_hover(app).to_owned());
        }
        return Some(format!(
            "Selected model not loaded. Press {MDI_RESTART}\u{a0}Build first."
        ));
    }
    // Only a tick behind the current one rebuilds, and only then can the build be rejected.
    build_disabled_reason
        .filter(|_| app.run_to_input < current)
        .map(str::to_owned)
}

/// Draws the line that identifies the sweep run the loaded model replays.
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
    if let Some(warning) = opened_run_warning(app) {
        ui.colored_label(ui.visuals().warn_fg_color, format!("{MDI_ALERT} {warning}"));
    }
}

/// Returns the warning the opened run's line carries, `None` without an opened run or for a model that replays
/// exactly.
///
/// The Results tab shows it beside Open, and a run a host opens never passes through that tab.
fn opened_run_warning(app: &AppState) -> Option<&'static str> {
    app.opened_run.as_ref()?;
    let entry = app.loaded_entry()?;
    (!entry.metadata().replays_exactly).then_some(INEXACT_REPLAY)
}

#[cfg(test)]
mod tests {
    use super::{RUN_TO_TICK, opened_run_warning, playback_ui};
    use crate::options::AppOpening;
    use crate::state::{AppState, NO_SELECTED_MODEL, OpenAt};
    use crate::tests::support::{accessible_texts, gpu_only_app, replay};
    use crate::ui::results::table::INEXACT_REPLAY;

    /// The Run to tick box takes the name of its label.
    #[test]
    fn the_run_to_tick_box_has_a_name() {
        let Some(mut app) = gpu_only_app() else {
            return;
        };
        let names = accessible_texts(
            |ui| playback_ui(ui, &mut app),
            |node| (node.role() == egui::accesskit::Role::SpinButton).then(|| node.label().unwrap_or_default()),
        );
        assert_eq!(names, [RUN_TO_TICK]);
    }

    #[test]
    fn build_is_refused_with_no_model_selected() {
        let Some(app) = gpu_only_app() else {
            return;
        };
        assert_eq!(app.build_refusal().as_deref(), Some(NO_SELECTED_MODEL));
    }

    /// A host's opening never passes through the Results tab, where the warning sits beside Open.
    #[test]
    fn an_opened_run_of_a_model_that_does_not_replay_exactly_carries_the_warning() {
        let Some(mut app) = AppState::headless(henad_models::example_models(), true) else {
            return;
        };
        let run_of = |model: &str| {
            let entry = app.models.get(model).expect("an example model");
            replay(model, entry.setup().values().to_vec())
        };
        let boids = run_of("boids");
        let gpu_boids = run_of("gpu_boids");

        app.open(AppOpening::Run {
            replay: boids,
            open_at: OpenAt::Start,
        });
        app.wait_for_build();
        assert!(app.opened_run.is_some());
        assert_eq!(opened_run_warning(&app), None);

        app.open(AppOpening::Run {
            replay: gpu_boids,
            open_at: OpenAt::Start,
        });
        app.wait_for_build();
        assert!(app.opened_run.is_some(), "the GPU run opened");
        assert_eq!(opened_run_warning(&app), Some(INEXACT_REPLAY));
    }
}
