//! The Model panel: model selection, and the declarations of the selected model.

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::capacity::Demand;
use henad_core::helpers::fmt_bytes;
use henad_core::metadata::{LaneSpec, Structure};
use henad_core::params::ParamDescriptor;
use henad_core::topology::{NeighborhoodKind, TopologyHint};

use crate::icons::material_design_icons::{
    MDI_CHECK, MDI_CLOSE, MDI_CODE_BRACES, MDI_FILE_DOCUMENT_PLUS_OUTLINE, MDI_FILE_PLUS_OUTLINE,
};
use crate::state::{AppState, OpeningRefusal};
use crate::ui::editor;
use crate::ui::sweep::layout::add_button;
use crate::ui::{KvGridRows, kv_grid};

/// Text the Model panel shows when no model of the set runs on this machine.
const NO_MODEL_RUNS: &str = "No model in this build runs on this device. GPU models need a GPU with compute support.";

/// Text the Model panel shows for a set without models.
const NO_MODELS: &str = "This build has no models.";

/// Hover text of the Exact replay row for a model that replays exactly.
const REPLAYS_EXACTLY: &str = "Two runs with the same seed match.";

/// Hover text of the Exact replay row for a model that does not replay exactly.
const DOES_NOT_REPLAY: &str = "Two runs with the same seed can differ.";

/// Draws the opening that the app rejected and the reason, then `next_step`.
fn opening_refusal(ui: &mut egui::Ui, refusal: &OpeningRefusal, next_step: &str) {
    ui.colored_label(ui.visuals().warn_fg_color, refusal.lead);
    ui.label(format!("{}.", refusal.reason));
    ui.label(next_step);
}

/// Returns the label of each model in the model list, for `offered` as `(id, name)` pairs, as [`label_among`] words
/// it.
fn list_labels(offered: &[(String, String)]) -> Vec<String> {
    offered
        .iter()
        .map(|(id, name)| label_among(offered, id, name))
        .collect()
}

/// Returns the label of model `id` named `name` beside `offered`, the offered models as `(id, name)` pairs: the name,
/// followed by the id in parentheses when another offered model has the same name, letter case aside.
fn label_among(offered: &[(String, String)], id: &str, name: &str) -> String {
    let lower = name.to_lowercase();
    if offered
        .iter()
        .any(|(other_id, other)| other_id != id && other.to_lowercase() == lower)
    {
        format!("{name} ({id})")
    } else {
        name.to_owned()
    }
}

/// Returns the id and name of each model of the set that runs on this machine.
fn offered_names(app: &AppState) -> Vec<(String, String)> {
    app.offered_models()
        .map(|entry| (entry.id().to_owned(), entry.name().to_owned()))
        .collect()
}

/// Returns the label of model `id` named `name` as the model list shows it: the name, with the id in parentheses when
/// another model the app offers has the same name, letter case aside.
///
/// The Sweep tab and the Results tab name a model by this label, a model the set lacks included.
pub fn model_label(app: &AppState, id: &str, name: &str) -> String {
    label_among(&offered_names(app), id, name)
}

pub fn model_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let offered = offered_names(app);
    if offered.is_empty() {
        // The refusal of a hidden GPU model already mentions the GPU, and an empty set has no GPU to blame.
        match &app.opening_refusal {
            Some(refusal) => opening_refusal(ui, refusal, "No model in this build runs on this device."),
            None if app.models.is_empty() => {
                ui.label(NO_MODELS);
            }
            None => {
                ui.label(NO_MODEL_RUNS);
            }
        }
        return;
    }
    let labels = list_labels(&offered);
    let selected_name = offered
        .iter()
        .zip(&labels)
        .find(|((id, _), _)| app.selected_model.as_deref() == Some(id.as_str()))
        .map_or_else(
            || app.selected_entry().map_or("None", ModelEntry::name).to_owned(),
            |(_, label)| label.clone(),
        );
    let mut picked = None;

    egui::ComboBox::from_label("Select model")
        .selected_text(selected_name)
        // As tall as the window allows. The model list outgrows egui's default of 200 points.
        .height(ui.ctx().content_rect().height())
        .show_ui(ui, |ui| {
            for ((id, _), label) in offered.iter().zip(&labels) {
                let selected = app.selected_model.as_deref() == Some(id.as_str());
                if ui.selectable_label(selected, label).clicked() && !selected {
                    picked = Some(id.clone());
                }
            }
        });

    let changed_model = picked.is_some();
    if let Some(id) = picked {
        app.select_model(&id);
    }
    editor_buttons(ui, app);

    let Some(entry) = app.selected_entry() else {
        if let Some(refusal) = &app.opening_refusal {
            ui.separator();
            opening_refusal(ui, refusal, "Select a model to continue.");
        }
        return;
    };

    ui.separator();
    ui.label(entry.description());
    ui.separator();

    // Recomputed as the sliders move, so the footprint tracks the panel next door.
    let demand = entry.demand(&app.param_values, &app.runtime.granted);
    let storage_limit = app.runtime.granted.max_storage_buffers_per_shader_stage;

    let mut scroll = egui::ScrollArea::vertical();
    if changed_model {
        // Models differ in how many rows they declare, so a carried-over offset can open the
        // next model halfway down.
        scroll = scroll.vertical_scroll_offset(0.0);
    }

    scroll.show(ui, |ui| {
        section(ui, "Identity");
        kv_grid(ui, "model_identity_grid").show(ui, |ui, rows| {
            rows.row(ui, "Id", entry.id());
            rows.row(ui, "Backend", entry.metadata().backend.label());
            rows.row(ui, "Topology", topology_label(entry.topology_hint()));
            let (value, hover) = replay_cells(entry.metadata().replays_exactly);
            ui.label("Exact replay").on_hover_text(hover);
            ui.label(value).on_hover_text(hover);
            rows.end_row(ui);
        });

        ui.add_space(8.0);
        section(ui, "Structure");
        kv_grid(ui, "model_structure_grid").show(ui, |ui, rows| structure_rows(ui, rows, &entry.metadata().structure));

        ui.add_space(8.0);
        section(ui, "Interface");
        kv_grid(ui, "model_interface_grid").show(ui, |ui, rows| interface_rows(ui, rows, entry));

        // A CPU model allocates on the host, and the Performance tab already reports that.
        if let Some(demand) = &demand {
            ui.add_space(8.0);
            section(ui, "Footprint");
            kv_grid(ui, "model_footprint_grid").show(ui, |ui, rows| {
                footprint_rows(ui, rows, entry.id(), demand, storage_limit);
            });
        }
    });
}

/// Draws New model, New grid rule, and Edit model for a selected model whose code the Editor can show. New model opens
/// its dialog, and the other two open the Editor.
fn editor_buttons(ui: &mut egui::Ui, app: &mut AppState) {
    let editable = app.selected_entry().is_some_and(editor::can_edit);
    ui.horizontal_wrapped(|ui| {
        let model = add_button(
            ui,
            true,
            egui::Button::new(format!("{MDI_FILE_DOCUMENT_PLUS_OUTLINE} {}", editor::NEW_MODEL_BUTTON)),
            editor::NEW_MODEL_BUTTON,
        )
        .on_hover_text("Start new model in Editor");
        if model.clicked() {
            editor::replace_model(app, editor::Replacement::NewModel);
        }
        let new = add_button(
            ui,
            true,
            egui::Button::new(format!("{MDI_FILE_PLUS_OUTLINE} New grid rule")),
            "New grid rule",
        )
        .on_hover_text("Open Game of Life rule in Editor");
        if new.clicked() {
            editor::replace_model(app, editor::Replacement::NewGridRule);
        }
        if editable {
            let edit = add_button(
                ui,
                true,
                egui::Button::new(format!("{MDI_CODE_BRACES} Edit model")),
                "Edit model",
            )
            .on_hover_text("Open model file in Editor");
            if edit.clicked() {
                editor::edit_model(app);
            }
        }
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.strong(title);
    ui.add_space(2.0);
}

/// Adds a row with the count `value`, whose two cells both list `names` on hover.
///
/// A value as short as "7" is barely a pointer wide on its own.
fn row_listing(ui: &mut egui::Ui, rows: &mut KvGridRows, label: &str, value: String, names: &str) {
    ui.label(label).on_hover_text(names);
    ui.label(value).on_hover_text(names);
    rows.end_row(ui);
}

fn topology_label(hint: TopologyHint) -> &'static str {
    match (hint.grid, hint.agents, hint.edges) {
        (false, true, true) => "Network",
        (true, true, true) => "Network and grid",
        (true, true, false) => "Agents and grid",
        (true, false, false) => "Grid",
        (false, true, false) => "Agents",
        _ => "None",
    }
}

fn neighborhood_label(kind: NeighborhoodKind) -> &'static str {
    match kind {
        NeighborhoodKind::Moore => "Moore",
        NeighborhoodKind::VonNeumann => "Von Neumann",
    }
}

fn yes_no(present: bool) -> &'static str {
    if present { MDI_CHECK } else { MDI_CLOSE }
}

/// Returns the value and the hover text of the Exact replay row, for a model that replays exactly when
/// `replays_exactly` holds.
fn replay_cells(replays_exactly: bool) -> (&'static str, &'static str) {
    if replays_exactly {
        (MDI_CHECK, REPLAYS_EXACTLY)
    } else {
        (MDI_CLOSE, DOES_NOT_REPLAY)
    }
}

/// Returns the count `count`, followed by the count `flagged` labelled `flag` unless `flagged` is zero, as in
/// "7 (2 reload)".
fn count_of(count: usize, flagged: usize, flag: &str) -> String {
    if flagged == 0 {
        count.to_string()
    } else {
        format!("{count} ({flagged} {flag})")
    }
}

/// Shows the lane rows, shared by agent and network models.
fn lane_rows(ui: &mut egui::Ui, rows: &mut KvGridRows, lanes: &[LaneSpec]) {
    let doubled = lanes.iter().filter(|lane| lane.double_buffered).count();
    let names: Vec<String> = lanes
        .iter()
        .map(|lane| {
            let mark = if lane.double_buffered { " (double-buffered)" } else { "" };
            format!("{}: {}{mark}", lane.name, lane.ty)
        })
        .collect();
    row_listing(
        ui,
        rows,
        "Lanes",
        count_of(lanes.len(), doubled, "double-buffered"),
        &names.join("\n"),
    );
}

fn structure_rows(ui: &mut egui::Ui, rows: &mut KvGridRows, structure: &Structure) {
    match structure {
        Structure::Grid { neighborhood } => {
            rows.row(ui, "Neighbourhood", neighborhood_label(*neighborhood));
        }
        Structure::Agents {
            chunk,
            lanes,
            index,
            field,
        } => {
            lane_rows(ui, rows, lanes);
            rows.row(ui, "Chunk size", format!("{chunk} agents"));
            rows.row(ui, "Neighbour index", *index);
            rows.row(ui, "Field layer", *field);
        }
        Structure::Network { chunk, lanes, .. } => {
            lane_rows(ui, rows, lanes);
            rows.row(ui, "Chunk size", format!("{chunk} nodes"));
        }
        Structure::GpuGrid { buffers, workgroup } => {
            row_listing(ui, rows, "Buffers", buffers.len().to_string(), &buffers.join("\n"));
            rows.row(ui, "Workgroup", format!("{workgroup} × {workgroup}"));
        }
        Structure::GpuAgents {
            buffers,
            passes,
            index,
            display,
            counters,
        } => {
            let doubled = buffers.iter().filter(|spec| spec.double_buffered).count();
            let buffer_names: Vec<&str> = buffers.iter().map(|spec| spec.label.as_ref()).collect();
            let pass_names: Vec<&str> = passes.iter().map(|pass| pass.label.as_ref()).collect();
            row_listing(
                ui,
                rows,
                "Buffers",
                count_of(buffers.len(), doubled, "double-buffered"),
                &buffer_names.join("\n"),
            );
            row_listing(
                ui,
                rows,
                "Step passes",
                passes.len().to_string(),
                &pass_names.join("\n"),
            );
            rows.row(ui, "Neighbour index", if *index { "Spatial hash" } else { "None" });
            rows.row(ui, "Display pass", yes_no(*display));
            if *counters > 0 {
                rows.row(ui, "Counters", counters.to_string());
            }
        }
        // A structure this build does not know has no rows to show.
        _ => {}
    }
}

/// Adds the rows of the model's parameters, statistics and palettes.
///
/// The Parameters row counts the parameters the Parameters tab shows, a hidden tick length left out.
fn interface_rows(ui: &mut egui::Ui, rows: &mut KvGridRows, entry: &ModelEntry) {
    let shown: Vec<&ParamDescriptor> = entry
        .param_descriptors()
        .iter()
        .filter(|descriptor| !descriptor.is_hidden())
        .collect();
    let reload = shown.iter().filter(|descriptor| !descriptor.is_live()).count();
    rows.row(ui, "Parameters", count_of(shown.len(), reload, "reload"));

    ui.label("Statistics");
    ui.horizontal(|ui| {
        ui.label(entry.stat_descriptors().len().to_string());
        swatches(ui, entry.stat_descriptors().iter().map(|stat| stat.color()));
    });
    rows.end_row(ui);

    ui.label("Palette");
    match &entry.metadata().palette {
        Some(palette) => {
            ui.horizontal(|ui| {
                ui.label(palette.len().to_string());
                swatches(ui, palette.iter().copied());
            });
        }
        // A GPU agent model's shaders write RGBA directly and declare no palette.
        None => {
            ui.label("In shader")
                .on_hover_text("Palette colours are declared in the shaders and cannot be retrieved.");
        }
    }
    rows.end_row(ui);

    if let Structure::Network { edge_palette, .. } = &entry.metadata().structure {
        ui.label("Edge palette");
        ui.horizontal(|ui| {
            ui.label(edge_palette.len().to_string());
            swatches(ui, edge_palette.iter().copied());
        });
        rows.end_row(ui);
    }
}

/// Adds the rows of the model's expected footprint on the GPU.
fn footprint_rows(ui: &mut egui::Ui, rows: &mut KvGridRows, id: &str, demand: &Demand, storage_limit: u32) {
    rows.row(ui, "Expected memory", fmt_bytes(demand.bytes()));

    if let Some(largest) = demand.buffers.iter().max_by_key(|buffer| buffer.bytes) {
        row_listing(
            ui,
            rows,
            "Largest buffer",
            format!("{}, {}", strip_id(&largest.label, id), fmt_bytes(largest.bytes)),
            &largest.label,
        );
    }

    if let Some((width, height)) = demand.texture {
        rows.row(ui, "Display texture", format!("{width} × {height}"));
    }

    if let Some(widest) = demand.passes.iter().max_by_key(|pass| pass.storage) {
        let hint = format!("Pass with the most storage bindings: '{}'", widest.label);
        ui.label("Storage bindings").on_hover_text(&hint);
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 0.0;
            ui.label(widest.storage.to_string()).on_hover_text(&hint);
            ui.weak(format!(" of {storage_limit}")).on_hover_text(&hint);
        });
        rows.end_row(ui);
    }
}

/// Returns `label` without its `{id}_` prefix. The panel already shows the model's name.
fn strip_id<'a>(label: &'a str, id: &str) -> &'a str {
    label
        .strip_prefix(id)
        .and_then(|rest| rest.strip_prefix('_'))
        .unwrap_or(label)
}

/// Draws a row of colour chips.
fn swatches(ui: &mut egui::Ui, colors: impl Iterator<Item = [u8; 4]>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let size = egui::vec2(9.0, 9.0);
        for [r, g, b, a] in colors {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::ZERO,
                egui::Color32::from_rgba_unmultiplied(r, g, b, a),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        DOES_NOT_REPLAY, REPLAYS_EXACTLY, count_of, label_among, list_labels, replay_cells, strip_id, topology_label,
    };
    use crate::icons::material_design_icons::{MDI_CHECK, MDI_CLOSE};
    use henad_core::topology::TopologyHint;

    /// An applied sheet of Game of Life sits beside the example of that name, and the list tells the two apart.
    #[test]
    fn the_model_list_names_the_ids_of_models_that_share_a_name() {
        let offered: Vec<(String, String)> = [
            ("sir", "SIR Epidemic"),
            ("boids", "Boids"),
            ("game_of_life", "Game of Life"),
            ("sheet_game_of_life", "Game of Life"),
            ("sheet_sir", "SIR epidemic"),
        ]
        .map(|(id, name)| (id.to_owned(), name.to_owned()))
        .into();
        assert_eq!(
            list_labels(&offered),
            [
                "SIR Epidemic (sir)",
                "Boids",
                "Game of Life (game_of_life)",
                "Game of Life (sheet_game_of_life)",
                "SIR epidemic (sheet_sir)"
            ],
            "names that differ in case alone"
        );
        assert_eq!(
            label_among(&offered, "folder_sir", "SIR EPIDEMIC"),
            "SIR EPIDEMIC (folder_sir)",
            "a model the set lacks, as a results folder names it"
        );
        assert_eq!(label_among(&offered, "boids", "Boids"), "Boids");
    }

    /// The Exact replay row marks GPU Boids, whose agents add up floats over their neighbours, and no CPU model.
    #[test]
    fn the_replay_row_marks_a_model_that_does_not_replay_exactly() {
        let models = henad_models::example_models();
        let cells = |id: &str| {
            let entry = models.get(id).expect("an example model");
            replay_cells(entry.metadata().replays_exactly)
        };
        assert_eq!(cells("gpu_boids"), (MDI_CLOSE, DOES_NOT_REPLAY));
        assert_eq!(cells("boids"), (MDI_CHECK, REPLAYS_EXACTLY));
        assert_eq!(cells("gpu_game_of_life"), (MDI_CHECK, REPLAYS_EXACTLY));
    }

    #[test]
    fn topology_names_every_layer_combination() {
        assert_eq!(topology_label(TopologyHint::GRID), "Grid");
        assert_eq!(topology_label(TopologyHint::AGENTS), "Agents");
        assert_eq!(topology_label(TopologyHint::COMPOSITE), "Agents and grid");
        assert_eq!(topology_label(TopologyHint::NETWORK), "Network");
        assert_eq!(topology_label(TopologyHint::NONE), "None");
    }

    /// A model with nothing flagged shows as a bare count, without "(0 reload)".
    #[test]
    fn a_count_drops_its_qualifier_when_nothing_carries_it() {
        assert_eq!(count_of(5, 0, "reload"), "5");
        assert_eq!(count_of(5, 2, "reload"), "5 (2 reload)");
    }

    /// A buffer whose label does not start with the id keeps its whole label.
    #[test]
    fn a_buffer_label_loses_only_the_model_id() {
        assert_eq!(strip_id("gpu_ants_accum_a", "gpu_ants"), "accum_a");
        assert_eq!(strip_id("gpu_ants_hash_counts", "gpu_ants"), "hash_counts");
        assert_eq!(strip_id("scratch", "gpu_ants"), "scratch");
        assert_eq!(strip_id("gpu_antsy", "gpu_ants"), "gpu_antsy");
    }
}
