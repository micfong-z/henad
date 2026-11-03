//! Pass stack of the graph editor: each stage with its passes and their issue counts, and the forms that add, order,
//! rename, switch and remove a pass.

use egui::{Button, ComboBox, CornerRadius, Id, RichText, TextEdit, Ui};
use henad_graph::document::{Domain, Pass, PassEnabled, PassStage, Topology};
use henad_graph::edit::{EditCommand, Scope};

use super::{GraphEdit, GraphEditor, GraphRequest};
use crate::icons::material_design_icons::{MDI_ARROW_DOWN, MDI_ARROW_UP, MDI_DELETE_OUTLINE, MDI_PLUS};
use crate::ui::mcs;
use crate::ui::sweep::layout::add_button;

/// Stages in the order the pass stack runs them, with the heading the panel shows for each.
const STAGES: [(PassStage, &str); 5] = [
    (PassStage::Init, "Init"),
    (PassStage::Tick, "Tick"),
    (PassStage::View, "View"),
    (PassStage::Stats, "Stats"),
    (PassStage::Action, "Actions"),
];

/// Width in points of a name field in the side panel.
pub const FIELD_WIDTH: f32 = 140.0;

/// Draws a single-line field that shows `current`, and returns its text when the field loses focus with another text.
///
/// The text being typed lives in egui's memory under `id` until then. A refused edit leaves the field showing
/// `current` again.
pub fn commit_field(ui: &mut Ui, id: Id, current: &str, hint: &str) -> Option<String> {
    let draft_id = id.with("draft");
    let mut text = ui
        .data(|data| data.get_temp::<String>(draft_id))
        .unwrap_or_else(|| current.to_owned());
    let response = ui.add(
        TextEdit::singleline(&mut text)
            .id(id)
            .hint_text(hint)
            .desired_width(FIELD_WIDTH),
    );
    if response.lost_focus() {
        ui.data_mut(|data| data.remove::<String>(draft_id));
        return (text != current && !text.is_empty()).then_some(text);
    }
    if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(draft_id, text));
    }
    None
}

/// Draws a badge of `count` in the MCS colours `fill` and `text`, with `tooltip` on hover.
pub fn count_badge(ui: &mut Ui, count: usize, fill: egui::Color32, text: egui::Color32, tooltip: &str) {
    if count == 0 {
        return;
    }
    ui.add(
        Button::new(RichText::new(count.to_string()).small().color(text))
            .fill(fill)
            .corner_radius(CornerRadius::ZERO)
            .sense(egui::Sense::hover()),
    )
    .on_hover_text(tooltip);
}

/// Returns the domain a new pass of a graph over `topology` runs over.
pub fn topology_domain(topology: Topology) -> Domain {
    match topology {
        Topology::Agents => Domain::Agents,
        _ => Domain::Cells,
    }
}

/// Draws the pass stack.
pub fn passes_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    ui.strong("Passes");
    let document = graph.document();
    for (stage, heading) in STAGES {
        let passes: Vec<&Pass> = document.passes.iter().filter(|pass| pass.stage == stage).collect();
        if passes.is_empty() {
            continue;
        }
        ui.label(RichText::new(heading).small().weak());
        for pass in passes {
            pass_row(ui, graph, pass, requests);
        }
    }
    if let Some(pass) = graph.open_pass() {
        ui.add_space(4.0);
        open_pass_ui(ui, graph, pass, requests);
    }
    ui.add_space(4.0);
    add_pass_ui(ui, graph, requests);
}

/// Draws the row of `pass`: its name, which opens it, its issue counts and its switch.
fn pass_row(ui: &mut Ui, graph: &GraphEditor, pass: &Pass, requests: &mut Vec<GraphRequest>) {
    ui.horizontal(|ui| {
        let open = graph.scope() == &Scope::pass(&pass.id);
        let name = match &pass.action {
            Some(action) => format!("{} ({action})", pass.id),
            None => pass.id.clone(),
        };
        if ui
            .selectable_label(open, name)
            .on_hover_text("Open pass on canvas")
            .clicked()
        {
            requests.push(GraphRequest::Open(Scope::pass(&pass.id)));
        }
        let (errors, warnings) = graph.pass_issue_counts(&pass.id);
        count_badge(ui, errors, mcs::RED_900, mcs::RED_200, "Errors in this pass");
        count_badge(ui, warnings, mcs::ORANGE_900, mcs::ORANGE_200, "Warnings in this pass");
        match &pass.enabled {
            PassEnabled::Literal(false) => {
                ui.weak("off");
            }
            PassEnabled::Param(param) => {
                ui.weak(format!("when {param}"));
            }
            _ => {}
        }
    });
}

/// Draws the forms of the open pass: rename, move within its stage, switch and remove.
fn open_pass_ui(ui: &mut Ui, graph: &GraphEditor, pass: &Pass, requests: &mut Vec<GraphRequest>) {
    let document = graph.document();
    let stage_passes: Vec<&str> = document
        .passes
        .iter()
        .filter(|known| known.stage == pass.stage)
        .map(|known| known.id.as_str())
        .collect();
    let index = stage_passes.iter().position(|id| *id == pass.id).unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label("Name");
        if let Some(to) = commit_field(ui, Id::new("henad_graph_pass_name").with(&pass.id), &pass.id, "pass id") {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::RenamePass {
                from: pass.id.clone(),
                to,
            })));
        }
    });
    ui.horizontal(|ui| {
        let up = add_button(ui, index > 0, Button::new(MDI_ARROW_UP).small(), "Move up")
            .on_hover_text("Run pass earlier in its stage");
        if up.clicked() {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::MovePass {
                id: pass.id.clone(),
                index: index.saturating_sub(1),
            })));
        }
        let down = add_button(
            ui,
            index + 1 < stage_passes.len(),
            Button::new(MDI_ARROW_DOWN).small(),
            "Move down",
        )
        .on_hover_text("Run pass later in its stage");
        if down.clicked() {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::MovePass {
                id: pass.id.clone(),
                index: index + 1,
            })));
        }
        let mut enabled = !matches!(pass.enabled, PassEnabled::Literal(false));
        let bound = matches!(pass.enabled, PassEnabled::Param(_));
        let switch = ui
            .add_enabled(!bound, egui::Checkbox::new(&mut enabled, "Runs"))
            .on_hover_text("Switch pass on or off")
            .on_disabled_hover_text("A parameter switches this pass.");
        if switch.changed() {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::SetPassEnabled {
                id: pass.id.clone(),
                enabled: PassEnabled::Literal(enabled),
            })));
        }
        let remove = add_button(ui, true, Button::new(MDI_DELETE_OUTLINE).small(), "Remove pass")
            .on_hover_text("Remove pass and its nodes");
        if remove.clicked() {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::RemovePass(
                pass.id.clone(),
            ))));
        }
    });
}

/// Draws the form that adds a pass: its stage, its action for the action stage, and its id.
fn add_pass_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    let document = graph.document();
    let form_id = Id::new("henad_graph_add_pass");
    let (mut stage_index, mut action, mut id) = ui
        .data(|data| data.get_temp::<(usize, String, String)>(form_id))
        .unwrap_or((1, String::new(), String::new()));
    ui.horizontal(|ui| {
        ComboBox::from_id_salt(form_id.with("stage"))
            .width(70.0)
            .selected_text(STAGES.get(stage_index).map_or("", |(_, heading)| *heading))
            .show_ui(ui, |ui| {
                for (index, (_, heading)) in STAGES.iter().enumerate() {
                    ui.selectable_value(&mut stage_index, index, *heading);
                }
            });
        ui.add(
            TextEdit::singleline(&mut id)
                .hint_text("new pass id")
                .desired_width(90.0),
        );
    });
    let stage = STAGES.get(stage_index).map_or(PassStage::Tick, |(stage, _)| *stage);
    if stage == PassStage::Action {
        ComboBox::from_id_salt(form_id.with("action"))
            .selected_text(if action.is_empty() { "Action" } else { action.as_str() })
            .show_ui(ui, |ui| {
                for known in &document.actions {
                    ui.selectable_value(&mut action, known.id.clone(), &known.label);
                }
            });
    }
    let ready = !id.is_empty() && (stage != PassStage::Action || !action.is_empty());
    let add = add_button(ui, ready, Button::new(format!("{MDI_PLUS} Add pass")), "Add pass")
        .on_hover_text("Add empty pass at end of its stage")
        .on_disabled_hover_text(if stage == PassStage::Action && document.actions.is_empty() {
            "Add an action in the World panel first."
        } else {
            "Enter an id for the pass."
        });
    if add.clicked() {
        let mut pass = Pass::new(&id, stage, Some(topology_domain(document.world.topology())));
        if stage == PassStage::Action {
            pass.action = Some(action.clone());
        }
        requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::AddPass(pass))));
        requests.push(GraphRequest::Open(Scope::pass(&id)));
        id.clear();
    }
    ui.data_mut(|data| data.insert_temp(form_id, (stage_index, action, id)));
}
