//! The Parameters panel's preview mode: the parameters of the Editor's preview, each change tried in the preview,
//! and Reset and Keep under a tried value.

use super::values::{display_value, sheet_number, value_widget};
use super::{NO_PARAMETERS, action_buttons, param_text};
use crate::state::AppState;
use crate::ui::editor::EditorModel;
use crate::ui::editor::graph::world::param_with;
use crate::ui::editor::preview::{SIZE_PARAMS, model_value};
use crate::ui::editor::sheet::sheet_pane::button_does;
use crate::ui::sweep::layout::add_button;
use henad_compute::cpu::sim_thread::SimCommand;
use henad_compute::entry::ModelEntry;
use henad_core::action::ActionDescriptor;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_graph::edit::EditCommand;
use henad_graph::sheet::sentence::{KEEP, KEEP_TOOLTIP, RESET, RESTART_NOTICE, keep_name, reset_name, trying_value};
use web_time::Instant;

/// Choice the user made for one parameter of the preview.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TrialChoice {
    /// Moved the slider to this value.
    Try(ParamValue),
    /// Pressed Reset.
    Reset,
    /// Pressed Keep.
    Keep,
}

/// Draws the parameters of the Editor's preview, a change of each tried in the preview, then the buttons of its
/// actions.
///
/// A size is left out, since the preview sets its own, and so is a hidden parameter. A tried value shows the value the
/// model keeps under its slider, with Reset, and with Keep while the Editor holds the document the preview runs. The
/// Seed field and the schedule are left out. Both belong to the next build of the selected model.
pub(super) fn preview_params_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let Some(entry) = app.loaded_entry().cloned() else {
        return;
    };
    let descriptors = entry.param_descriptors();
    let shown: Vec<usize> = descriptors
        .iter()
        .enumerate()
        .filter(|(_, descriptor)| !descriptor.is_hidden() && !SIZE_PARAMS.contains(&descriptor.id()))
        .map(|(index, _)| index)
        .collect();
    if shown.is_empty() {
        ui.label(NO_PARAMETERS);
    }
    let can_keep = holds_previewed_document(app, &entry);
    let mut chosen = None;
    for index in shown {
        let descriptor = &descriptors[index];
        let keeps = model_value(&app.editor.model, descriptor);
        let tried = app
            .editor
            .preview
            .tried(descriptor.id())
            .map(|tried| tried.value.clone());
        let mut value = tried
            .clone()
            .unwrap_or_else(|| app.loaded_values.get(index).cloned().unwrap_or_else(|| keeps.clone()));
        let hint = (!descriptor.is_live()).then_some(RESTART_NOTICE);
        if value_widget(ui, descriptor, &mut value, param_text(ui, descriptor, false), hint) {
            chosen = Some((index, TrialChoice::Try(value)));
        }
        if let Some(tried) = &tried
            && let Some(choice) = trial_row(ui, descriptor, tried, &keeps, can_keep)
        {
            chosen = Some((index, choice));
        }
    }
    if let Some((index, choice)) = chosen {
        apply_trial(app, &entry, index, choice);
    }

    let actions = entry.action_descriptors();
    if !actions.is_empty() {
        ui.add_space(8.0);
        ui.separator();
        let descriptions = button_descriptions(app, actions);
        if let Some(index) = action_buttons(ui, actions, app.has_simulation(), &descriptions) {
            app.send_command(SimCommand::Act(index));
            app.editor.preview.acted = true;
        }
    }
}

/// Returns what each of `actions` does, by the index of the action, while the preview runs the sheet the Editor holds:
/// the words of the sheet's button with the action's id, as the Sheet pane's Buttons section shows them. Returns no
/// description while it runs anything else.
fn button_descriptions(app: &AppState, actions: &[ActionDescriptor]) -> Vec<Option<String>> {
    let Some(sheet) = app.editor.model.sheet().filter(|_| app.previews_editor_sheet()) else {
        return Vec::new();
    };
    let document = sheet.document();
    actions
        .iter()
        .map(|action| {
            document
                .buttons
                .iter()
                .find(|button| button.id == action.id())
                .map(|button| button_does(document, button))
                .filter(|does| !does.is_empty())
        })
        .collect()
}

/// Returns whether the Editor holds the document the preview of `entry` runs: the sheet whose preview it is, or a
/// graph with the entry's id.
fn holds_previewed_document(app: &AppState, entry: &ModelEntry) -> bool {
    match &app.editor.model {
        EditorModel::Sheet(_) => app.previews_editor_sheet(),
        EditorModel::Graph(graph) => graph.document().target.id == entry.id(),
        EditorModel::Empty | EditorModel::Rule(_) => false,
    }
}

/// Draws the note under the slider of `descriptor` while the preview tries `tried` and the model keeps `keeps`, with
/// Reset and, when `can_keep` holds, Keep, and returns the button pressed.
fn trial_row(
    ui: &mut egui::Ui,
    descriptor: &ParamDescriptor,
    tried: &ParamValue,
    keeps: &ParamValue,
    can_keep: bool,
) -> Option<TrialChoice> {
    let kept = display_value(descriptor, keeps);
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.weak(trying_value(&kept));
        let reset = add_button(
            ui,
            true,
            egui::Button::new(RESET),
            &reset_name(descriptor.label(), &kept),
        );
        if reset.clicked() {
            chosen = Some(TrialChoice::Reset);
        }
        if can_keep {
            let name = keep_name(&display_value(descriptor, tried), descriptor.label());
            let keep = add_button(ui, true, egui::Button::new(KEEP), &name).on_hover_text(KEEP_TOOLTIP);
            if keep.clicked() {
                chosen = Some(TrialChoice::Keep);
            }
        }
    });
    chosen
}

/// Applies `choice` to parameter `index` of `entry`, the preview's entry, and sends the value the preview then runs
/// with.
///
/// A live parameter reaches the running preview at once. Any other one rebuilds the preview at tick 0 after the
/// preview's debounce, with every tried value. Keep makes the tried value the one the Editor's document keeps, as one
/// edit, and needs neither, since the preview already runs with it.
pub(crate) fn apply_trial(app: &mut AppState, entry: &ModelEntry, index: usize, choice: TrialChoice) {
    let Some(descriptor) = entry.param_descriptors().get(index) else {
        return;
    };
    let keeps = model_value(&app.editor.model, descriptor);
    let value = match choice {
        TrialChoice::Try(value) => {
            app.editor.preview.try_value(descriptor.id(), value.clone(), keeps);
            value
        }
        TrialChoice::Reset => {
            app.editor.preview.end_trial(descriptor.id());
            keeps
        }
        TrialChoice::Keep => {
            if let Some(tried) = app
                .editor
                .preview
                .tried(descriptor.id())
                .map(|tried| tried.value.clone())
                && keep_value(&mut app.editor.model, descriptor, &tried)
            {
                app.editor.preview.end_trial(descriptor.id());
            }
            return;
        }
    };
    if descriptor.is_live() {
        app.send_preview_param(index, value);
    } else {
        let builds = app.builds();
        app.editor.preview.rebuild_after(Instant::now(), builds);
    }
}

/// Makes `value` the value that `model` keeps for the parameter `descriptor` describes, widening its range to hold it,
/// as one edit of the sheet or the graph. Returns whether the edit was made.
fn keep_value(model: &mut EditorModel, descriptor: &ParamDescriptor, value: &ParamValue) -> bool {
    match model {
        EditorModel::Sheet(sheet) => {
            let Some(number) = sheet_number(value) else {
                return false;
            };
            sheet.keep_value(descriptor.id(), number).is_ok()
        }
        EditorModel::Graph(graph) => {
            let Some(param) = graph
                .document()
                .params
                .iter()
                .find(|param| param.id() == descriptor.id())
                .cloned()
            else {
                return false;
            };
            let Some(kind) = kind_keeping(param.kind(), value) else {
                return false;
            };
            let kept = param_with(&param, param.label(), kind);
            graph.edit(vec![EditCommand::SetParam(kept)], None)
        }
        EditorModel::Empty | EditorModel::Rule(_) => false,
    }
}

/// Returns `kind` with `value` as its default, its range widened to hold it, or `None` for a value of another kind.
fn kind_keeping(kind: &ParamKind, value: &ParamValue) -> Option<ParamKind> {
    Some(match (kind, value) {
        (ParamKind::F32 { min, max, step, .. }, ParamValue::F32(number)) => ParamKind::F32 {
            min: min.min(*number),
            max: max.max(*number),
            default: *number,
            step: *step,
        },
        (ParamKind::U32 { min, max, .. }, ParamValue::U32(number)) => ParamKind::U32 {
            min: (*min).min(*number),
            max: (*max).max(*number),
            default: *number,
        },
        (ParamKind::Bool { .. }, ParamValue::Bool(flag)) => ParamKind::Bool { default: *flag },
        (ParamKind::Choice { options, .. }, ParamValue::Choice(choice)) => ParamKind::Choice {
            options: options.clone(),
            default: *choice,
        },
        _ => return None,
    })
}
