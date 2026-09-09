//! Writing a run's results out: the stat series, the final state, run details, and the viewport.

pub mod image;
pub mod metadata;
pub mod save;

use henad_compute::snapshot::SnapshotView;
use henad_core::export::{StatsWriter, state as state_export};

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CHART_LINE, MDI_RECORD_CIRCLE_OUTLINE, MDI_STOP, MDI_TRAY_ARROW_DOWN,
};
use crate::state::AppState;

/// A stat recording.
pub enum Recording {
    Off,
    Running {
        writer: StatsWriter<Vec<u8>>,
        from_tick: u64,
    },
    Done {
        csv: Vec<u8>,
        rows: u64,
        from_tick: u64,
        to_tick: u64,
    },
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

/// The chart history, written out with the same columns the headless runner writes.
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
            "Turn on Unlimited history in the {MDI_CHART_LINE} Charts panel to capture full run history. Alternatively, use the {MDI_RECORD_CIRCLE_OUTLINE} Record button below."
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
                let from_tick = app.snapshot.as_ref().map_or(0, |snap| snap.tick);
                app.recording = Recording::Running {
                    writer: StatsWriter::new(Vec::new()),
                    from_tick,
                };
                app.export_status = None;
            }
        }
        Recording::Running { writer, from_tick } => {
            let rows = writer.rows();
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

    let is_cpu = matches!(app.snapshot.as_ref().map(|snap| &snap.view), Some(SnapshotView::Cpu(_)));

    let button = ui.add_enabled(
        loaded && is_cpu,
        egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save state")),
    );
    let button = if loaded && is_cpu {
        button.on_hover_text("Write the current model state as text")
    } else {
        button.on_disabled_hover_text("GPU model states are not exportable")
    };
    if button.clicked() {
        export_state(app);
    }

    ui.separator();
    ui.strong("Viewport");

    let dims = image::capture_dims(app, app.render_ctx.device.limits().max_texture_dimension_2d);
    match dims {
        Some((width, height)) => ui.label(format!("{width} x {height} pixels")),
        None => ui.label("Nothing to display"),
    };

    if ui
        .add_enabled(
            loaded && dims.is_some(),
            egui::Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save image")),
        )
        .on_hover_text("Capture the layers as a PNG at their own resolution, not the panel's")
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
        app.save(&file_name(app, "run", "json"), json.into_bytes());
    }
}

/// `henad-<model>-<export_type>-<tick>.<ext>`, so files from several runs sort together.
fn file_name(app: &AppState, export_type: &str, ext: &str) -> String {
    let model = app
        .loaded_model
        .and_then(|index| app.registry.get(index))
        .map_or("model", |entry| entry.id.as_str());
    let tick = app.snapshot.as_ref().map_or(0, |snap| snap.tick);
    format!("henad-{model}-{export_type}-{tick}.{ext}")
}

fn export_history(app: &mut AppState) {
    let Some(history) = &app.stats_history else {
        return;
    };
    let mut writer = StatsWriter::new(Vec::new());
    for j in 0..history.len() {
        let (Some(entries), Some(tick)) = (history.entries(j), history.tick(j)) else {
            break;
        };
        if let Err(err) = writer.push(tick, &entries) {
            app.export_status = Some(format!("Export failed: {err}"));
            return;
        }
    }
    match writer.into_inner() {
        Ok((csv, _)) => app.save(&file_name(app, "stats", "csv"), csv),
        Err(err) => app.export_status = Some(format!("Export failed: {err}")),
    }
}

fn save_recording(app: &mut AppState) {
    let Recording::Done { csv, .. } = &app.recording else {
        return;
    };
    let bytes = csv.clone();
    app.save(&file_name(app, "recording", "csv"), bytes);
}

fn export_state(app: &mut AppState) {
    let Some(snapshot) = &app.snapshot else {
        return;
    };
    let SnapshotView::Cpu(layers) = &snapshot.view else {
        return;
    };

    let mut out = Vec::new();
    let written = layers
        .grid
        .as_ref()
        .map_or(Ok(()), |grid| {
            state_export::write_grid(&mut out, grid.width, grid.height, &grid.cells)
        })
        .and_then(|()| {
            layers.points.as_ref().map_or(Ok(()), |points| {
                let color = (!points.color.is_empty()).then_some(points.color.as_slice());
                state_export::write_points(&mut out, &points.pos_x, &points.pos_y, color)
            })
        });

    match written {
        Ok(()) if out.is_empty() => app.export_status = Some("Model exposes no view to export".to_owned()),
        Ok(()) => app.save(&file_name(app, "state", "txt"), out),
        Err(err) => app.export_status = Some(format!("Export failed: {err}")),
    }
}
