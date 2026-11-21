//! Writing a run's results out: the stat series, the final state, run details, and the viewport.

pub mod image;
pub mod metadata;

use henad_compute::snapshot::{CpuLayers, SnapshotView};
use henad_core::export::{StatsWriteError, StatsWriter, state as state_export};
use henad_core::view::StatEntry;

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CHART_LINE, MDI_RECORD_CIRCLE_OUTLINE, MDI_STOP, MDI_TRAY_ARROW_DOWN,
};
use crate::state::AppState;
use crate::ui::files::SaveTarget;
use crate::ui::time_axis::RunTime;

/// Recording of the stats the sim publishes, from the tick Record is pressed at until it stops.
///
/// A recording keeps the id of the model it recorded. Its file is named after that model even once another model is
/// loaded.
pub enum Recording {
    Off,
    Running {
        writer: StatsWriter<Vec<u8>>,
        /// Id of the recorded model.
        model: String,
        from_tick: u64,
        /// Newest row with its tick, held back until a later tick arrives or the recording stops.
        held: Option<(u64, Vec<StatEntry>)>,
    },
    Done {
        csv: Vec<u8>,
        /// Id of the recorded model.
        model: String,
        rows: u64,
        from_tick: u64,
        to_tick: u64,
    },
}

impl Recording {
    /// Starts a recording of model `model` at `from_tick`. A run with a time scale, `time`, adds a `time` column after
    /// `tick`, as `--export-stats` writes it.
    pub fn start(model: String, from_tick: u64, time: Option<RunTime>) -> Self {
        Self::Running {
            writer: stats_writer(time),
            model,
            from_tick,
            held: None,
        }
    }

    /// Takes one published sample, replacing the held row if it has the same tick.
    ///
    /// A publish repeats a tick when nothing stepped, as after an action. The held row then takes
    /// the action's stats, and the CSV keeps one row per tick. Does nothing unless a recording is
    /// running.
    ///
    /// # Errors
    ///
    /// Returns an error when the row it writes out does not match the layout the first row fixed.
    pub fn push(&mut self, tick: u64, stats: &[StatEntry]) -> Result<(), StatsWriteError> {
        let Self::Running { writer, held, .. } = self else {
            return Ok(());
        };
        match held {
            Some((held_tick, held_stats)) => {
                if *held_tick != tick {
                    writer.push(*held_tick, held_stats)?;
                    *held_tick = tick;
                }
                held_stats.clear();
                held_stats.extend_from_slice(stats);
            }
            None => *held = Some((tick, stats.to_vec())),
        }
        Ok(())
    }

    /// Writes out the held row and finishes a running recording, which ends at `to_tick`.
    ///
    /// Anything but a running recording is returned unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the held row does not match the layout the first row fixed, or the final flush fails.
    pub fn stop(self, to_tick: u64) -> Result<Self, StatsWriteError> {
        match self {
            Self::Running {
                mut writer,
                model,
                from_tick,
                held,
            } => {
                if let Some((tick, stats)) = held {
                    writer.push(tick, &stats)?;
                }
                let (csv, rows) = writer.into_inner()?;
                Ok(Self::Done {
                    csv,
                    model,
                    rows,
                    from_tick,
                    to_tick,
                })
            }
            other => Ok(other),
        }
    }
}

pub fn export_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let loaded = app.sim_thread.is_some();

    stats_section(ui, app, loaded);
    ui.separator();
    recording_section(ui, app, loaded);
    ui.separator();
    state_section(ui, app, loaded);
    ui.separator();
    details_section(ui, app, loaded);

    if let Some(status) = &app.export_status {
        ui.separator();
        ui.label(status);
    }
}

fn plural(count: u64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Draws the Statistics section. Save stats writes the chart history with the columns the headless runner writes.
fn stats_section(ui: &mut egui::Ui, app: &mut AppState, loaded: bool) {
    ui.strong("Statistics");

    let span = app.stats_history.as_ref().and_then(|history| {
        let last = history.len().checked_sub(1)?;
        Some((history.len() as u64, history.tick(0)?, history.tick(last)?))
    });

    match span {
        Some((samples, from, to)) => ui.label(format!("{samples} sample{} (ticks {from} to {to})", plural(samples))),
        None => ui.label("No samples yet."),
    };

    if loaded && app.history_capacity.is_some() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{MDI_ALERT} History length is limited."),
        )
        .on_hover_text(format!(
            "Turn on Unlimited history in the {MDI_CHART_LINE} Charts tab to capture full run history. Alternatively, press {MDI_RECORD_CIRCLE_OUTLINE} Record below."
        ));
    }

    if ui
        .add_enabled(
            loaded && span.is_some(),
            egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save stats")),
        )
        .on_hover_text("Export stats history as CSV")
        .clicked()
    {
        export_history(app);
    }
}

fn recording_section(ui: &mut egui::Ui, app: &mut AppState, loaded: bool) {
    ui.strong("Recording");

    match &app.recording {
        Recording::Off => {
            ui.label("Start recording to capture stats from a specific time.");
            if ui
                .add_enabled(loaded, egui::Button::new(format!("{MDI_RECORD_CIRCLE_OUTLINE} Record")))
                .on_hover_text("Start stats capture")
                .clicked()
            {
                start_recording(app);
            }
        }
        Recording::Running {
            writer,
            from_tick,
            held,
            ..
        } => {
            // The held row counts, since stopping writes it.
            let rows = writer.rows() + u64::from(held.is_some());
            ui.label(format!("{rows} row{} since tick {from_tick}", plural(rows)));
            if ui.button(format!("{MDI_STOP} Stop")).clicked() {
                app.stop_recording();
            }
        }
        Recording::Done {
            rows,
            from_tick,
            to_tick,
            ..
        } => {
            ui.label(format!("{rows} row{}, ticks {from_tick} to {to_tick}", plural(*rows)));
            ui.horizontal(|ui| {
                if ui.button(format!("{MDI_TRAY_ARROW_DOWN} Save recording")).clicked() {
                    save_recording(app);
                }
                if ui.button("Discard").clicked() {
                    app.recording = Recording::Off;
                    app.export_status = None;
                }
            });
        }
    }
}

fn state_section(ui: &mut egui::Ui, app: &mut AppState, loaded: bool) {
    ui.strong("Current state");

    let is_cpu = matches!(
        app.snapshot.as_ref().map(|snapshot| &snapshot.view),
        Some(SnapshotView::Cpu(_))
    );

    let button = ui.add_enabled(
        loaded && is_cpu,
        egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save state")),
    );
    let button = if loaded && is_cpu {
        button.on_hover_text("Write current model state as text")
    } else {
        button.on_disabled_hover_text("GPU model states cannot be exported")
    };
    if button.clicked() {
        export_state(app);
    }

    ui.separator();
    ui.strong("Viewport");

    let dims = image::capture_dims(app, app.render_ctx.device.limits().max_texture_dimension_2d);
    match dims {
        Some((width, height)) => ui.label(format!("{width} × {height} pixels")),
        None => ui.label("Nothing to display"),
    };

    if ui
        .add_enabled(
            loaded && dims.is_some(),
            egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save image")),
        )
        .on_hover_text("Capture layers as a PNG at native resolution")
        .clicked()
    {
        let name = file_name(app, "viewport", "png");
        app.request_viewport_capture(name);
    }
}

fn details_section(ui: &mut egui::Ui, app: &mut AppState, loaded: bool) {
    ui.strong("Run details");
    if ui
        .add_enabled(loaded, egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save details")))
        .on_hover_text("Save run details as JSON")
        .clicked()
    {
        let json = metadata::run_details(app);
        app.save_as(SaveTarget::Export, &file_name(app, "run", "json"), json.into_bytes());
    }
}

/// Returns the file name `henad-<model>-<export_type>-<tick>.<extension>` of the loaded model at the current tick, so
/// that files from several runs sort together.
fn file_name(app: &AppState, export_type: &str, extension: &str) -> String {
    export_name(loaded_model(app), export_type, app.current_tick(), extension)
}

/// Returns the file name `henad-<model>-<export_type>-<tick>.<extension>`.
fn export_name(model: &str, export_type: &str, tick: u64, extension: &str) -> String {
    format!("henad-{model}-{export_type}-{tick}.{extension}")
}

/// Id of the loaded model, or `model` with nothing loaded.
fn loaded_model(app: &AppState) -> &str {
    app.loaded_entry().map_or("model", |entry| entry.id())
}

/// Returns a writer of a stat series into memory, with a `time` column for a run with a time scale, `time`.
fn stats_writer(time: Option<RunTime>) -> StatsWriter<Vec<u8>> {
    let writer = StatsWriter::new(Vec::new());
    match time {
        Some(time) => writer.tick_length(time.tick_length),
        None => writer,
    }
}

/// Starts a recording at the current tick of the loaded run, with its model's id and time scale.
pub fn start_recording(app: &mut AppState) {
    app.recording = Recording::start(loaded_model(app).to_owned(), app.current_tick(), app.loaded_time());
    app.export_status = None;
}

/// Returns the chart history of the loaded run as CSV, with the columns the headless runner writes, `None` with no
/// history.
///
/// # Errors
///
/// Returns an error when a sample does not match the layout the first sample fixed.
pub fn history_csv(app: &AppState) -> Option<Result<Vec<u8>, StatsWriteError>> {
    let history = app.stats_history.as_ref()?;
    let mut writer = stats_writer(app.loaded_time());
    for position in 0..history.len() {
        let (Some(entries), Some(tick)) = (history.entries(position), history.tick(position)) else {
            break;
        };
        if let Err(error) = writer.push(tick, &entries) {
            return Some(Err(error));
        }
    }
    Some(writer.into_inner().map(|(csv, _)| csv))
}

fn export_history(app: &mut AppState) {
    match history_csv(app) {
        Some(Ok(csv)) => app.save_as(SaveTarget::Export, &file_name(app, "stats", "csv"), csv),
        Some(Err(error)) => app.export_status = Some(format!("Export failed: {error}")),
        None => {}
    }
}

/// Saves a finished recording, named after the model it recorded and the tick it stopped at.
fn save_recording(app: &mut AppState) {
    let Recording::Done {
        csv, model, to_tick, ..
    } = &app.recording
    else {
        return;
    };
    let name = export_name(model, "recording", *to_tick, "csv");
    let bytes = csv.clone();
    app.save_as(SaveTarget::Export, &name, bytes);
}

fn export_state(app: &mut AppState) {
    let Some(snapshot) = &app.snapshot else {
        return;
    };
    let SnapshotView::Cpu(layers) = &snapshot.view else {
        return;
    };

    let mut out = Vec::new();
    match write_state(&mut out, layers) {
        Ok(()) if out.is_empty() => app.export_status = Some("Model has no view to export".to_owned()),
        Ok(()) => app.save_as(SaveTarget::Export, &file_name(app, "state", "txt"), out),
        Err(error) => app.export_status = Some(format!("Export failed: {error}")),
    }
}

/// Writes the grid of `layers`, then its points, then its edges when it has points, to `out`.
///
/// # Errors
///
/// Returns the first error of a write.
fn write_state(out: &mut Vec<u8>, layers: &CpuLayers) -> std::io::Result<()> {
    if let Some(grid) = &layers.grid {
        state_export::write_grid(out, grid.width, grid.height, &grid.cells)?;
    }
    if let Some(points) = &layers.points {
        let color = (!points.color.is_empty()).then_some(points.color.as_slice());
        state_export::write_points(out, &points.pos_x, &points.pos_y, color)?;
        if let Some(edges) = &layers.edges {
            let rows = state_export::point_rows(&points.pos_x, &points.pos_y);
            state_export::write_edges(out, &edges.src, &edges.dst, &edges.color, &rows)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use henad_core::view::{StatEntry, StatValue};

    use super::{Recording, save_recording, start_recording};
    use crate::state::AppState;
    use crate::tests::support::{set_grid_side, step_past};
    use crate::ui::files::SaveTarget;

    fn row(value: f64) -> Vec<StatEntry> {
        vec![StatEntry::new("a", StatValue::Scalar(value), [0, 0, 0, 255])]
    }

    /// An action publishes at the tick it was pressed on. The recording keeps one row for that
    /// tick, holding the action's stats, and stopping writes the last row out.
    #[test]
    fn a_publish_at_the_same_tick_replaces_the_held_row() {
        let mut recording = Recording::start("sir".to_owned(), 5, None);
        for (tick, value) in [(5, 1.0), (5, 2.0), (6, 3.0), (6, 3.0), (7, 4.0), (7, 5.0)] {
            recording.push(tick, &row(value)).expect("one series throughout");
        }
        let Ok(Recording::Done {
            csv,
            rows,
            from_tick,
            to_tick,
            ..
        }) = recording.stop(7)
        else {
            panic!("a running recording did not stop cleanly");
        };
        assert_eq!(String::from_utf8(csv).expect("CSV is UTF-8"), "tick,a\n5,2\n6,3\n7,5\n");
        assert_eq!((rows, from_tick, to_tick), (3, 5, 7));
    }

    /// A recording saved after a build of another model, or after Offload, is named after the model it recorded and
    /// the tick it stopped at.
    #[test]
    fn a_recording_is_named_after_the_run_it_recorded() {
        let Some(mut app) = AppState::headless(henad_models::example_models(), false) else {
            return;
        };
        app.select_model("sir");
        set_grid_side(&mut app, 64);
        app.reset_simulation();
        step_past(&mut app, 0);
        start_recording(&mut app);
        step_past(&mut app, 1);
        app.stop_recording();
        let Recording::Done { to_tick, .. } = &app.recording else {
            panic!("the recording did not stop");
        };
        let name = format!("henad-sir-recording-{to_tick}.csv");

        app.select_model("boids");
        app.reset_simulation();
        app.wait_for_build();
        save_recording(&mut app);
        app.offload_simulation();
        save_recording(&mut app);
        assert_eq!(
            app.saves_requested,
            Some(vec![(SaveTarget::Export, name.clone()), (SaveTarget::Export, name)])
        );
    }
}
