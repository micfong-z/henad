//! Model selection, and what the selected model declares about itself.

use henad_compute::gpu::capacity::Demand;
use henad_core::helpers::fmt_bytes;
use henad_core::metadata::Structure;
use henad_core::params::ParamDescriptor;
use henad_core::topology::{NeighborhoodKind, TopologyHint};
use henad_models::registry::ModelEntry;

use crate::icons::material_design_icons::{MDI_CHECK, MDI_CLOSE};
use crate::state::AppState;
use crate::ui::kv_grid;

pub fn model_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let model_names: Vec<&str> = app.registry.iter().map(|m| m.name.as_str()).collect();
    let mut changed_model = false;

    egui::ComboBox::from_label("Select Model")
        .selected_text(model_names.get(app.selected_model).copied().unwrap_or("None"))
        .show_ui(ui, |ui| {
            for (i, name) in model_names.iter().enumerate() {
                if ui.selectable_value(&mut app.selected_model, i, *name).changed() {
                    changed_model = true;
                }
            }
        });

    if changed_model {
        app.load_default_params();
    }

    let Some(entry) = app.registry.get(app.selected_model) else {
        return;
    };

    ui.separator();
    ui.label(entry.description.as_str());
    ui.separator();

    // Recomputed as the sliders move, so the footprint tracks the panel next door.
    let demand = entry.demand(&app.param_values);
    let storage_limit = app.runtime.granted.max_storage_buffers_per_shader_stage;

    let mut scroll = egui::ScrollArea::vertical();
    if changed_model {
        // Models differ in how many rows they declare, so a carried-over offset can land the
        // next one halfway down.
        scroll = scroll.vertical_scroll_offset(0.0);
    }

    scroll.show(ui, |ui| {
        section(ui, "Identity");
        kv_grid(ui, "model_identity_grid").show(ui, |ui| {
            row(ui, "Id", entry.id.as_str());
            row(ui, "Backend", entry.metadata.backend.label());
            row(ui, "Topology", topology_label(entry.topology_hint));
        });

        ui.add_space(8.0);
        section(ui, "Structure");
        kv_grid(ui, "model_structure_grid").show(ui, |ui| structure_rows(ui, &entry.metadata.structure));

        ui.add_space(8.0);
        section(ui, "Interface");
        kv_grid(ui, "model_interface_grid").show(ui, |ui| interface_rows(ui, entry));

        // A CPU model allocates on the host, and the Performance tab already reports that.
        if let Some(demand) = &demand {
            ui.add_space(8.0);
            section(ui, "Footprint");
            kv_grid(ui, "model_footprint_grid").show(ui, |ui| {
                footprint_rows(ui, entry.id.as_str(), demand, storage_limit);
            });
        }
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.strong(title);
    ui.add_space(2.0);
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::WidgetText>) {
    ui.label(label);
    ui.label(value);
    ui.end_row();
}

/// A row whose cells both carry the names behind the count they report.
///
/// Both, since a value as short as "7" is barely a pointer wide on its own.
fn row_listing(ui: &mut egui::Ui, label: &str, value: String, names: &str) {
    ui.label(label).on_hover_text(names);
    ui.label(value).on_hover_text(names);
    ui.end_row();
}

fn topology_label(hint: TopologyHint) -> &'static str {
    match (hint.grid, hint.agents) {
        (true, true) => "Agents and Grid",
        (true, false) => "Grid",
        (false, true) => "Agents",
        (false, false) => "None",
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

/// `n` of a thing, with the subset that carries `flag` named after it.
fn count_of(n: usize, flagged: usize, flag: &str) -> String {
    if flagged == 0 {
        n.to_string()
    } else {
        format!("{n} ({flagged} {flag})")
    }
}

fn structure_rows(ui: &mut egui::Ui, structure: &Structure) {
    match structure {
        Structure::Grid { neighborhood } => {
            row(ui, "Neighbourhood", neighborhood_label(*neighborhood));
        }
        Structure::Agents {
            chunk,
            lanes,
            index,
            field,
        } => {
            let doubled = lanes.iter().filter(|lane| lane.double_buffered).count();
            let names: Vec<String> = lanes
                .iter()
                .map(|lane| {
                    let mark = if lane.double_buffered { " (dual)" } else { "" };
                    format!("{}: {}{mark}", lane.name, lane.ty)
                })
                .collect();
            row_listing(
                ui,
                "Lanes",
                count_of(lanes.len(), doubled, "double-buffered"),
                &names.join("\n"),
            );
            row(ui, "Chunk size", format!("{chunk} agents"));
            row(ui, "Neighbour index", *index);
            row(ui, "Field layer", *field);
        }
        Structure::GpuGrid { buffers, workgroup } => {
            row_listing(ui, "Buffers", buffers.len().to_string(), &buffers.join("\n"));
            row(ui, "Workgroup", format!("{workgroup} × {workgroup}"));
        }
        Structure::GpuAgents {
            buffers,
            passes,
            index,
            display,
            counters,
        } => {
            let doubled = buffers.iter().filter(|spec| spec.double_buffered).count();
            let buffer_names: Vec<&str> = buffers.iter().map(|spec| spec.label).collect();
            let pass_names: Vec<&str> = passes.iter().map(|pass| pass.label).collect();
            row_listing(
                ui,
                "Buffers",
                count_of(buffers.len(), doubled, "double-buffered"),
                &buffer_names.join("\n"),
            );
            row_listing(ui, "Step passes", passes.len().to_string(), &pass_names.join("\n"));
            row(ui, "Neighbour index", if *index { "Spatial hash" } else { "None" });
            row(ui, "Display pass", yes_no(*display));
            if *counters > 0 {
                row(ui, "Counters", counters.to_string());
            }
        }
    }
}

fn interface_rows(ui: &mut egui::Ui, entry: &ModelEntry) {
    let descs: &[ParamDescriptor] = &entry.param_descriptors;
    let reload = descs.iter().filter(|desc| !desc.is_live()).count();
    row(ui, "Parameters", count_of(descs.len(), reload, "reload"));

    ui.label("Statistics");
    ui.horizontal(|ui| {
        ui.label(entry.stat_descriptors.len().to_string());
        swatches(ui, entry.stat_descriptors.iter().map(|stat| stat.color));
    });
    ui.end_row();

    ui.label("Palette");
    match entry.metadata.palette {
        Some(palette) => {
            ui.horizontal(|ui| {
                ui.label(palette.len().to_string());
                swatches(ui, palette.iter().copied());
            });
        }
        // A GPU agent model's shaders write RGBA directly and declare no palette.
        None => {
            ui.label("In shader").on_hover_text(
                "Palette colours are declared directly in the shaders and are not retrievable directly.",
            );
        }
    }
    ui.end_row();
}

/// Expected footprint of the model.
fn footprint_rows(ui: &mut egui::Ui, id: &str, demand: &Demand, storage_limit: u32) {
    row(ui, "Expected memory", fmt_bytes(demand.bytes()));

    if let Some(largest) = demand.buffers.iter().max_by_key(|alloc| alloc.bytes) {
        row_listing(
            ui,
            "Largest buffer",
            format!("{}, {}", strip_id(&largest.label, id), fmt_bytes(largest.bytes)),
            &largest.label,
        );
    }

    if let Some((w, h)) = demand.texture {
        row(ui, "Display texture", format!("{w} × {h}"));
    }

    if let Some(widest) = demand.passes.iter().max_by_key(|pass| pass.storage) {
        let hint = format!("Widest pass is '{}'", widest.label);
        ui.label("Storage bindings").on_hover_text(&hint);
        ui.horizontal(|ui| {
            ui.style_mut().spacing.item_spacing.x = 0.0;
            ui.label(widest.storage.to_string()).on_hover_text(&hint);
            ui.weak(format!(" of {storage_limit}")).on_hover_text(&hint);
        });
        ui.end_row();
    }
}

/// Buffer labels are prefixed with the model id, which the panel has already said.
fn strip_id<'a>(label: &'a str, id: &str) -> &'a str {
    label
        .strip_prefix(id)
        .and_then(|rest| rest.strip_prefix('_'))
        .unwrap_or(label)
}

/// A row of colour chips
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
    use super::{count_of, strip_id, topology_label};
    use henad_core::topology::TopologyHint;

    #[test]
    fn topology_names_every_layer_combination() {
        assert_eq!(topology_label(TopologyHint::GRID), "Grid");
        assert_eq!(topology_label(TopologyHint::AGENTS), "Agents");
        assert_eq!(topology_label(TopologyHint::COMPOSITE), "Agents and Grid");
        assert_eq!(topology_label(TopologyHint::NONE), "None");
    }

    /// A model with nothing flagged should read as a bare count, not "5 (0 reload)".
    #[test]
    fn a_count_drops_its_qualifier_when_nothing_carries_it() {
        assert_eq!(count_of(5, 0, "reload"), "5");
        assert_eq!(count_of(5, 2, "reload"), "5 (2 reload)");
    }

    /// A buffer whose label does not start with the id keeps every character of it.
    #[test]
    fn a_buffer_label_loses_only_the_model_id() {
        assert_eq!(strip_id("gpu_ants_accum_a", "gpu_ants"), "accum_a");
        assert_eq!(strip_id("gpu_ants_hash_counts", "gpu_ants"), "hash_counts");
        assert_eq!(strip_id("scratch", "gpu_ants"), "scratch");
        assert_eq!(strip_id("gpu_antsy", "gpu_ants"), "gpu_antsy");
    }
}
