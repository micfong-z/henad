//! World panel of the graph editor: the world table and the declarations, as forms that run edit commands.
//!
//! A *declaration* is a table of the file that nodes name: a category, a lane, a parameter, a statistic or an action.
//! A rename rewrites every reference to it, and a removal that something still names is refused with the list.

use egui::{Button, CollapsingHeader, ComboBox, DragValue, Id, RichText, TextEdit, Ui};
use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::{ParamApply, ParamDescriptor, ParamFormat, ParamKind};
use henad_graph::document::{Action, Category, GraphDocument, Lane, Variant, World};
use henad_graph::edit::EditCommand;
use henad_graph::sheet::edit::STATE_COLORS;
use henad_graph::sheet::sentence::color_button_name;
use henad_graph::types::ValueType;
use henad_graph::value::Color;

use super::passes::{FIELD_WIDTH, commit_field, topology_domain};
use super::{GraphEdit, GraphEditor, GraphRequest};
use crate::icons::material_design_icons::{MDI_DELETE_OUTLINE, MDI_PLUS};
use crate::ui::editor::sheet::widgets::{color_button, color_name, set_name};
use crate::ui::sweep::layout::add_button;

/// Largest grid side the world form takes, in cells.
const MAX_GRID_SIDE: u32 = 16384;

/// Colour of a variant added in the form once its category holds every colour of [`STATE_COLORS`].
const NEW_VARIANT_COLOR: Color = [128, 128, 128, 255];

/// Kinds of parameter the add form offers, with their names in the form.
const PARAM_KINDS: [&str; 3] = ["Float", "Integer", "Switch"];

/// Draws the world panel.
pub fn world_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    let document = graph.document();
    ui.strong("World");
    world_table_ui(ui, graph, requests);
    let shown_params = document.params.iter().filter(|param| !param.is_hidden()).count();
    CollapsingHeader::new(format!("Parameters ({shown_params})"))
        .id_salt("henad_graph_params")
        .default_open(true)
        .show(ui, |ui| params_ui(ui, graph, requests));
    CollapsingHeader::new(format!("Lanes ({})", document.lanes.len()))
        .id_salt("henad_graph_lanes")
        .show(ui, |ui| lanes_ui(ui, document, requests));
    CollapsingHeader::new(format!("Categories ({})", document.categories.len()))
        .id_salt("henad_graph_categories")
        .show(ui, |ui| categories_ui(ui, document, requests));
    CollapsingHeader::new(format!("Statistics ({})", document.stats.len()))
        .id_salt("henad_graph_stats")
        .show(ui, |ui| stats_ui(ui, document, requests));
    CollapsingHeader::new(format!("Actions ({})", document.actions.len()))
        .id_salt("henad_graph_actions")
        .show(ui, |ui| actions_ui(ui, document, requests));
}

/// Pushes the edit of one command.
fn push(requests: &mut Vec<GraphRequest>, command: EditCommand) {
    requests.push(GraphRequest::Edit(GraphEdit::one(command)));
}

/// Draws a small button with `icon` alone, named `name`, and returns whether it was pressed.
fn small_button(ui: &mut Ui, icon: &str, name: &str, tooltip: &str) -> bool {
    add_button(ui, true, Button::new(icon).small(), name)
        .on_hover_text(tooltip)
        .clicked()
}

/// Draws the world table: the grid size and boundary, or the agent count and the world size.
///
/// The fields show the world of the edits that wait for the primary button. Otherwise a dragged field would show the
/// document's value until the button goes up. A field clamps the values typed or dragged into it alone. A value the
/// file holds outside the range is drawn as it is and left to the checker.
fn world_table_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    let mut world = graph.shown_world();
    let changed = match &mut world {
        World::Grid(grid) => {
            ui.weak("Grid of cells");
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Size");
                changed |= ui
                    .add(
                        DragValue::new(&mut grid.width)
                            .range(1..=MAX_GRID_SIDE)
                            .clamp_existing_to_range(false),
                    )
                    .on_hover_text("Default grid width in cells")
                    .changed();
                ui.label("by");
                changed |= ui
                    .add(
                        DragValue::new(&mut grid.height)
                            .range(1..=MAX_GRID_SIDE)
                            .clamp_existing_to_range(false),
                    )
                    .on_hover_text("Default grid height in cells")
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Edges");
                ComboBox::from_id_salt("henad_graph_boundary")
                    .selected_text(match grid.boundary {
                        Boundary::Torus => "Wrap around",
                        Boundary::Bounded => "Walls",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut grid.boundary, Boundary::Torus, "Wrap around")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut grid.boundary, Boundary::Bounded, "Walls")
                            .changed();
                    });
            });
            changed
        }
        World::Agents(agents) => {
            ui.weak("Agents in a continuous world");
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Agents");
                changed |= ui
                    .add(
                        DragValue::new(&mut agents.count)
                            .range(1..=agents.max_count)
                            .clamp_existing_to_range(false),
                    )
                    .on_hover_text("Default number of agents")
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.label("Size");
                changed |= ui
                    .add(
                        DragValue::new(&mut agents.width)
                            .range(1.0..=f32::MAX)
                            .clamp_existing_to_range(false),
                    )
                    .on_hover_text("Default world width")
                    .changed();
                ui.label("by");
                changed |= ui
                    .add(
                        DragValue::new(&mut agents.height)
                            .range(1.0..=f32::MAX)
                            .clamp_existing_to_range(false),
                    )
                    .on_hover_text("Default world height")
                    .changed();
            });
            changed
        }
        _ => false,
    };
    if changed {
        requests.push(GraphRequest::Edit(GraphEdit {
            commands: vec![EditCommand::SetWorld(world)],
            coalesce: Some("world".to_owned()),
        }));
    }
}

/// Returns `descriptor` with `label` and `kind`, its id, apply, format and hidden flag kept.
pub(crate) fn param_with(descriptor: &ParamDescriptor, label: &str, kind: ParamKind) -> ParamDescriptor {
    let mut changed = ParamDescriptor::new(descriptor.id().to_owned(), label.to_owned(), kind);
    if descriptor.apply() == ParamApply::OnReload {
        changed = changed.on_reload();
    }
    if descriptor.format() == ParamFormat::Percent {
        changed = changed.percent();
    }
    if descriptor.is_hidden() {
        changed = changed.hidden();
    }
    changed
}

/// Draws each parameter's id, label and range, and the form that adds one.
///
/// The range and the default show the parameter of the edits that wait for the primary button, as the world table does.
/// A hidden parameter, such as a tick length, is left out.
fn params_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    for param in graph.document().params.iter().filter(|param| !param.is_hidden()) {
        let id = Id::new("henad_graph_param").with(param.id());
        CollapsingHeader::new(param.label()).id_salt(id).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Id");
                if let Some(to) = commit_field(ui, id.with("id"), param.id(), "parameter id") {
                    push(
                        requests,
                        EditCommand::RenameParam {
                            from: param.id().to_owned(),
                            to,
                        },
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Label");
                if let Some(label) = commit_field(ui, id.with("label"), param.label(), "label") {
                    push(
                        requests,
                        EditCommand::SetParam(param_with(param, &label, param.kind().clone())),
                    );
                }
            });
            let mut kind = graph.shown_param(param).kind().clone();
            let changed = match &mut kind {
                ParamKind::F32 { min, max, default, .. } => {
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.label("Range");
                        changed |= ui.add(DragValue::new(min).speed(0.01)).changed();
                        changed |= ui.add(DragValue::new(max).speed(0.01)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Default");
                        changed |= ui
                            .add(
                                DragValue::new(default)
                                    .speed(0.01)
                                    .range(*min..=*max)
                                    .clamp_existing_to_range(false),
                            )
                            .changed();
                    });
                    changed
                }
                ParamKind::U32 { min, max, default } => {
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.label("Range");
                        changed |= ui.add(DragValue::new(min)).changed();
                        changed |= ui.add(DragValue::new(max)).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Default");
                        changed |= ui
                            .add(
                                DragValue::new(default)
                                    .range(*min..=*max)
                                    .clamp_existing_to_range(false),
                            )
                            .changed();
                    });
                    changed
                }
                ParamKind::Bool { default } => ui.checkbox(default, "On by default").changed(),
                ParamKind::Choice { .. } => {
                    ui.weak("Choice options are edited in the file.");
                    false
                }
            };
            if changed {
                requests.push(GraphRequest::Edit(GraphEdit {
                    commands: vec![EditCommand::SetParam(param_with(param, param.label(), kind))],
                    coalesce: Some(format!("param {}", param.id())),
                }));
            }
            if small_button(ui, MDI_DELETE_OUTLINE, "Remove parameter", "Remove parameter") {
                push(requests, EditCommand::RemoveParam(param.id().to_owned()));
            }
        });
    }
    add_param_ui(ui, requests);
}

/// Draws the form that adds a parameter: its id and its kind.
fn add_param_ui(ui: &mut Ui, requests: &mut Vec<GraphRequest>) {
    let form_id = Id::new("henad_graph_add_param");
    let (mut id, mut kind) = ui
        .data(|data| data.get_temp::<(String, usize)>(form_id))
        .unwrap_or((String::new(), 0));
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut id)
                .hint_text("new parameter id")
                .desired_width(FIELD_WIDTH),
        );
        ComboBox::from_id_salt(form_id.with("kind"))
            .width(70.0)
            .selected_text(PARAM_KINDS.get(kind).copied().unwrap_or_default())
            .show_ui(ui, |ui| {
                for (index, name) in PARAM_KINDS.iter().enumerate() {
                    ui.selectable_value(&mut kind, index, *name);
                }
            });
    });
    let add = add_button(
        ui,
        !id.is_empty(),
        Button::new(format!("{MDI_PLUS} Add parameter")),
        "Add parameter",
    )
    .on_hover_text("Add parameter at end of schema")
    .on_disabled_hover_text("Enter an id for the parameter.");
    if add.clicked() {
        let param_kind = match kind {
            1 => ParamKind::U32 {
                min: 0,
                max: 100,
                default: 1,
            },
            2 => ParamKind::Bool { default: true },
            _ => ParamKind::F32 {
                min: 0.0,
                max: 1.0,
                default: 0.5,
                step: None,
            },
        };
        push(
            requests,
            EditCommand::AddParam(ParamDescriptor::new(id.clone(), id.clone(), param_kind)),
        );
        id.clear();
    }
    ui.data_mut(|data| data.insert_temp(form_id, (id, kind)));
}

/// Returns the lane types the add form offers: the scalar and vector types, then each category.
fn lane_types(document: &GraphDocument) -> Vec<ValueType> {
    let mut types = vec![
        ValueType::Bool,
        ValueType::U32,
        ValueType::I32,
        ValueType::F32,
        ValueType::Vec2F,
    ];
    types.extend(
        document
            .categories
            .iter()
            .map(|category| ValueType::Category(category.id.clone())),
    );
    types
}

/// Draws each lane with its type, and the form that adds one.
fn lanes_ui(ui: &mut Ui, document: &GraphDocument, requests: &mut Vec<GraphRequest>) {
    for lane in &document.lanes {
        ui.horizontal(|ui| {
            if let Some(to) = commit_field(ui, Id::new("henad_graph_lane").with(&lane.id), &lane.id, "lane id") {
                push(
                    requests,
                    EditCommand::RenameLane {
                        from: lane.id.clone(),
                        to,
                    },
                );
            }
            ui.weak(lane.value_type.to_string());
            if small_button(ui, MDI_DELETE_OUTLINE, "Remove lane", "Remove lane") {
                push(requests, EditCommand::RemoveLane(lane.id.clone()));
            }
        });
    }
    let form_id = Id::new("henad_graph_add_lane");
    let types = lane_types(document);
    let (mut id, mut type_index) = ui
        .data(|data| data.get_temp::<(String, usize)>(form_id))
        .unwrap_or((String::new(), 3));
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut id)
                .hint_text("new lane id")
                .desired_width(FIELD_WIDTH),
        );
        ComboBox::from_id_salt(form_id.with("type"))
            .width(70.0)
            .selected_text(types.get(type_index).map(ToString::to_string).unwrap_or_default())
            .show_ui(ui, |ui| {
                for (index, value_type) in types.iter().enumerate() {
                    ui.selectable_value(&mut type_index, index, value_type.to_string());
                }
            });
    });
    let add = add_button(
        ui,
        !id.is_empty(),
        Button::new(format!("{MDI_PLUS} Add lane")),
        "Add lane",
    )
    .on_hover_text("Add lane, one value per element")
    .on_disabled_hover_text("Enter an id for the lane.");
    if add.clicked()
        && let Some(value_type) = types.get(type_index)
    {
        let domain = topology_domain(document.world.topology());
        push(
            requests,
            EditCommand::AddLane(Lane::new(&id, domain, value_type.clone())),
        );
        id.clear();
    }
    ui.data_mut(|data| data.insert_temp(form_id, (id, type_index)));
}

/// Returns the colour of a variant added to `category`: the first of [`STATE_COLORS`] that no variant of `category`
/// takes, or gray once every one is taken.
pub fn next_variant_color(category: &Category) -> Color {
    STATE_COLORS
        .into_iter()
        .find(|color| category.variants.iter().all(|variant| variant.color != *color))
        .unwrap_or(NEW_VARIANT_COLOR)
}

/// Returns the accessible name of the colour button of the variant `variant`, such as "Colour of recovered: Green".
pub fn variant_color_name(variant: &Variant) -> String {
    color_button_name(&variant.name, color_name(variant.color))
}

/// Draws each category with its variants, each variant's colour button, a form per category that adds a variant, and
/// the form that adds a category with its first variant.
fn categories_ui(ui: &mut Ui, document: &GraphDocument, requests: &mut Vec<GraphRequest>) {
    if document.categories.is_empty() {
        ui.weak("No category.");
    }
    for category in &document.categories {
        ui.label(RichText::new(&category.id).strong());
        for variant in &category.variants {
            ui.horizontal(|ui| {
                if let Some(color) = color_button(ui, &variant_color_name(variant), variant.color)
                    && color != variant.color
                {
                    push(
                        requests,
                        EditCommand::SetVariantColor {
                            category: category.id.clone(),
                            name: variant.name.clone(),
                            color,
                        },
                    );
                }
                ui.label(&variant.name);
                ui.weak(&variant.label);
                if small_button(ui, MDI_DELETE_OUTLINE, "Remove variant", "Remove variant") {
                    push(
                        requests,
                        EditCommand::RemoveVariant {
                            category: category.id.clone(),
                            name: variant.name.clone(),
                        },
                    );
                }
            });
        }
        let form_id = Id::new("henad_graph_add_variant").with(&category.id);
        let mut name = ui.data(|data| data.get_temp::<String>(form_id)).unwrap_or_default();
        ui.horizontal(|ui| {
            let field = ui.add(
                TextEdit::singleline(&mut name)
                    .hint_text("new variant")
                    .desired_width(FIELD_WIDTH),
            );
            set_name(ui, field.id, &format!("New variant of {}", category.id));
            if add_button(ui, !name.is_empty(), Button::new(MDI_PLUS).small(), "Add variant")
                .on_hover_text("Add variant at end of category")
                .clicked()
            {
                push(
                    requests,
                    EditCommand::AddVariant {
                        category: category.id.clone(),
                        variant: Variant::new(&name, next_variant_color(category)),
                    },
                );
                name.clear();
            }
        });
        ui.data_mut(|data| data.insert_temp(form_id, name));
    }
    add_category_ui(ui, requests);
}

/// Draws the form that adds a category with its id and first variant. The checker refuses a category with no variant.
fn add_category_ui(ui: &mut Ui, requests: &mut Vec<GraphRequest>) {
    let form_id = Id::new("henad_graph_add_category");
    let (mut id, mut first) = ui
        .data(|data| data.get_temp::<(String, String)>(form_id))
        .unwrap_or_default();
    let field = ui.add(
        TextEdit::singleline(&mut id)
            .hint_text("new category id")
            .desired_width(FIELD_WIDTH),
    );
    set_name(ui, field.id, "New category id");
    let field = ui.add(
        TextEdit::singleline(&mut first)
            .hint_text("first variant")
            .desired_width(FIELD_WIDTH),
    );
    set_name(ui, field.id, "First variant");
    let add = add_button(
        ui,
        !id.is_empty() && !first.is_empty(),
        Button::new(format!("{MDI_PLUS} Add category")),
        "Add category",
    )
    .on_hover_text("Add category with its first variant")
    .on_disabled_hover_text("Enter an id and a first variant for the category.");
    if add.clicked() {
        push(
            requests,
            EditCommand::AddCategory(Category::new(&id, vec![Variant::new(&first, STATE_COLORS[0])])),
        );
        id.clear();
        first.clear();
    }
    ui.data_mut(|data| data.insert_temp(form_id, (id, first)));
}

/// Draws each statistic with the output it reads.
fn stats_ui(ui: &mut Ui, document: &GraphDocument, requests: &mut Vec<GraphRequest>) {
    if document.stats.is_empty() {
        ui.weak("No statistic. Add one from a node's menu in a stats pass.");
    }
    for stat in &document.stats {
        ui.horizontal(|ui| {
            let id = Id::new("henad_graph_stat").with(&stat.label);
            if let Some(to) = commit_field(ui, id, &stat.label, "label") {
                push(
                    requests,
                    EditCommand::RenameStat {
                        from: stat.label.clone(),
                        to,
                    },
                );
            }
            ui.weak(stat.from.to_string());
            if small_button(ui, MDI_DELETE_OUTLINE, "Remove statistic", "Remove statistic") {
                push(requests, EditCommand::RemoveStat(stat.label.clone()));
            }
        });
    }
}

/// Draws each action with its button label, and the form that adds one.
fn actions_ui(ui: &mut Ui, document: &GraphDocument, requests: &mut Vec<GraphRequest>) {
    for action in &document.actions {
        ui.horizontal(|ui| {
            let id = Id::new("henad_graph_action").with(&action.id);
            if let Some(to) = commit_field(ui, id, &action.id, "action id") {
                push(
                    requests,
                    EditCommand::RenameAction {
                        from: action.id.clone(),
                        to,
                    },
                );
            }
            ui.weak(&action.label);
            if small_button(ui, MDI_DELETE_OUTLINE, "Remove action", "Remove action") {
                push(requests, EditCommand::RemoveAction(action.id.clone()));
            }
        });
    }
    let form_id = Id::new("henad_graph_add_action");
    let mut id = ui.data(|data| data.get_temp::<String>(form_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut id)
                .hint_text("new action id")
                .desired_width(FIELD_WIDTH),
        );
        if add_button(ui, !id.is_empty(), Button::new(MDI_PLUS).small(), "Add action")
            .on_hover_text("Add action button")
            .clicked()
        {
            push(requests, EditCommand::AddAction(Action::new(&id, &id)));
            id.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(form_id, id));
}
