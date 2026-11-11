//! The Parameters panel: widgets generated from the model's `ParamDescriptor`s, the Seed row, the action buttons,
//! the scheduled actions and the notice under them, or the parameters of the Editor's preview while one runs.

mod schedule;
mod seed;
mod trial;
mod values;

pub use seed::{INVALID_SEED, parse_seed};
pub(crate) use seed::{draw_seed, seed_field_width};
#[cfg(test)]
pub(crate) use trial::{TrialChoice, apply_trial};
pub(crate) use values::{display_value, sheet_number};

use crate::icons::material_design_icons::{MDI_ALERT, MDI_INFORMATION, MDI_RESTART};
use crate::state::AppState;
use crate::ui::banner;
use henad_compute::cpu::sim_thread::SimCommand;
use henad_core::action::ActionDescriptor;
use henad_core::params::ParamDescriptor;
use schedule::schedule_ui;
use seed::seed_row;
use trial::preview_params_ui;
use values::value_widget;

/// Line the panel shows instead of its rows while no model is selected.
const NO_MODEL_SELECTED: &str = "No model selected.";

/// Line the panel shows instead of its sliders while every parameter of the model is hidden.
const NO_PARAMETERS: &str = "This model has no parameters.";

pub fn params_ui(ui: &mut egui::Ui, app: &mut AppState) {
    if app.params_drive_preview() {
        preview_params_ui(ui, app);
        return;
    }
    // With nothing selected, a seed or a schedule would apply to no build.
    let Some(entry) = app.selected_entry() else {
        ui.label(NO_MODEL_SELECTED);
        return;
    };
    let descriptors = entry.param_descriptors().to_vec();

    // Drawn before the sliders. A long slider label widens the region behind it, and the footer
    // would then wrap against that width and be clipped.
    let panel_width = ui.available_width();

    seed_row(ui, app);

    // A hidden parameter, such as a tick length, gets no slider. A sweep still varies it.
    if descriptors.iter().all(ParamDescriptor::is_hidden) {
        ui.label(NO_PARAMETERS);
        actions_ui(ui, app);
        notice(ui, app, &descriptors, panel_width);
        return;
    }

    let pending: Vec<bool> = descriptors
        .iter()
        .enumerate()
        .map(|(index, descriptor)| is_pending_reload(app, index, descriptor))
        .collect();

    let mut param_changed = Vec::new();

    let reload_hint = format!(
        "This parameter is only read when the model is built. Press {MDI_RESTART}\u{a0}Build to apply after change."
    );
    let pending_hint =
        format!("This parameter has been changed but not applied. Press {MDI_RESTART}\u{a0}Build to apply.");

    for (index, descriptor) in descriptors.iter().enumerate() {
        if descriptor.is_hidden() {
            continue;
        }
        let hint = if descriptor.is_live() {
            None
        } else if pending[index] {
            Some(pending_hint.as_str())
        } else {
            Some(reload_hint.as_str())
        };
        let text = param_text(ui, descriptor, pending[index]);

        let Some(value) = app.param_values.get_mut(index) else {
            continue;
        };
        if value_widget(ui, descriptor, value, text, hint) {
            param_changed.push((index, value.clone()));
        }
    }

    let mut sent_live = false;
    for (index, value) in &param_changed {
        // The running state rejects a reload-only parameter, so the edit is only marked pending.
        if !descriptors[*index].is_live() {
            if let Some(mark) = app.pending_reload.get_mut(*index) {
                *mark = true;
            }
            continue;
        }
        if app.send_live_param(*index, value.clone()) {
            sent_live = true;
        }
    }
    if sent_live {
        app.mark_opened_run_modified();
    }

    actions_ui(ui, app);
    notice(ui, app, &descriptors, panel_width);
}

/// Draws a button per action the model declares, under the parameter widgets, then the scheduled actions.
///
/// A press goes to the running sim. An action changes state that exists only once the model is built.
fn actions_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let actions: Vec<ActionDescriptor> = app
        .selected_entry()
        .map(|entry| entry.action_descriptors().to_vec())
        .unwrap_or_default();
    if actions.is_empty() {
        return;
    }

    let loaded = app.selection_is_loaded();

    ui.add_space(8.0);
    ui.separator();
    if let Some(index) = action_buttons(ui, &actions, loaded) {
        app.send_command(SimCommand::Act(index));
        app.mark_opened_run_modified();
    }

    schedule_ui(ui, app, &actions);
}

/// Draws a button per action of `actions`, enabled while `enabled` holds, and returns the index of the one pressed.
fn action_buttons(ui: &mut egui::Ui, actions: &[ActionDescriptor], enabled: bool) -> Option<usize> {
    let mut pressed = None;
    ui.horizontal_wrapped(|ui| {
        for (index, action) in actions.iter().enumerate() {
            if ui
                .add_enabled(enabled, egui::Button::new(action.label()))
                .on_disabled_hover_text(format!(
                    "Press {MDI_RESTART}\u{a0}Build to run this model, then {}",
                    action.label().to_lowercase()
                ))
                .clicked()
            {
                pressed = Some(index);
            }
        }
    });
    pressed
}

/// Returns whether parameter `index` has been edited to a value the running sim will not pick up on its own.
fn is_pending_reload(app: &AppState, index: usize, descriptor: &ParamDescriptor) -> bool {
    !descriptor.is_live() && app.selection_is_loaded() && app.pending_reload.get(index) == Some(&true)
}

/// Returns the widget label, marked when the parameter needs a reload and coloured while an edit of it is pending.
fn param_text(ui: &egui::Ui, descriptor: &ParamDescriptor, pending: bool) -> egui::RichText {
    if descriptor.is_live() {
        return egui::RichText::new(descriptor.label());
    }
    let text = egui::RichText::new(format!("{} {MDI_RESTART}", descriptor.label()));
    if pending {
        text.color(ui.visuals().warn_fg_color)
    } else {
        text
    }
}

/// Draws a banner on the state of the parameters when one applies: a size the device cannot hold, no simulation,
/// another model loaded, or edits or code awaiting a build.
fn notice(ui: &mut egui::Ui, app: &AppState, descriptors: &[ParamDescriptor], width: f32) {
    let pending_count = descriptors
        .iter()
        .enumerate()
        .filter(|(index, descriptor)| is_pending_reload(app, *index, descriptor))
        .count()
        + usize::from(app.seed_pending())
        + usize::from(app.schedule_pending());
    let shortfalls = app.selection_shortfalls();
    let Some(notice) = notice_content(app, pending_count, &shortfalls) else {
        return;
    };
    let color = match notice.tone {
        ParamsNoticeTone::Error => ui.visuals().error_fg_color,
        ParamsNoticeTone::Warn => ui.visuals().warn_fg_color,
        ParamsNoticeTone::Plain => ui.visuals().text_color(),
    };

    ui.add_space(8.0);
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.separator();
        banner(ui, notice.icon, color, notice.title, &notice.detail);
    });
}

/// Returns `reasons` joined as sentences, without the closing full stop.
///
/// A reason may close with its own full stop, as a sheet's reasons in the model's words do, and the stop is not
/// doubled.
fn sentences(reasons: &[String]) -> String {
    reasons
        .iter()
        .map(|reason| reason.trim_end_matches('.'))
        .collect::<Vec<_>>()
        .join(". ")
}

/// Banner on the state of the parameters, as [`notice_content`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParamsNotice {
    pub icon: &'static str,
    pub tone: ParamsNoticeTone,
    pub title: &'static str,
    pub detail: String,
}

/// Colour role of a [`ParamsNotice`], read from the theme when it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamsNoticeTone {
    Error,
    Warn,
    Plain,
}

/// Returns the banner on the state of the parameters, or `None` when none applies.
///
/// `pending_count` is the number of edits of the selection awaiting a build, and `shortfalls` holds the reasons the
/// device cannot build the selection.
pub(crate) fn notice_content(app: &AppState, pending_count: usize, shortfalls: &[String]) -> Option<ParamsNotice> {
    let notice = |icon, tone, title, detail| {
        Some(ParamsNotice {
            icon,
            tone,
            title,
            detail,
        })
    };
    if !shortfalls.is_empty() {
        return notice(
            MDI_ALERT,
            ParamsNoticeTone::Error,
            "Too large for this device",
            format!("{}. Reduce the size parameters to build.", sentences(shortfalls)),
        );
    }
    if !app.has_simulation() {
        return notice(
            MDI_INFORMATION,
            ParamsNoticeTone::Plain,
            "No simulation loaded",
            format!("Parameters will be applied after {MDI_RESTART}\u{a0}Build."),
        );
    }
    if app.code_pending() {
        return notice(
            MDI_ALERT,
            ParamsNoticeTone::Warn,
            "Reload needed",
            format!("Model code changed. Press {MDI_RESTART}\u{a0}Build to apply."),
        );
    }
    if !app.selection_is_loaded() {
        let running = app.loaded_entry().map_or("Another model", |entry| entry.name());
        return notice(
            MDI_ALERT,
            ParamsNoticeTone::Warn,
            "Selected model not loaded",
            format!(
                "The parameters above do not apply to {running}. Press {MDI_RESTART}\u{a0}Build to switch to the selected model."
            ),
        );
    }
    if pending_count > 0 {
        let (plural, verb) = if pending_count == 1 {
            ("", "takes")
        } else {
            ("s", "take")
        };
        return notice(
            MDI_ALERT,
            ParamsNoticeTone::Warn,
            "Reload needed",
            format!("{pending_count} change{plural} {verb} effect after {MDI_RESTART}\u{a0}Build."),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::schedule::remove_button;
    use super::seed::dice_button;
    use super::{NO_MODEL_SELECTED, params_ui};
    use crate::icons::material_design_icons::{MDI_DELETE_OUTLINE, MDI_DICE_5};
    use crate::tests::support::{accessible_names, accessible_texts, gpu_only_app};

    /// With no model selected, the panel shows neither a Seed field nor a claim that the model has no parameters.
    #[test]
    fn with_no_model_selected_the_panel_offers_nothing_to_edit() {
        let Some(mut app) = gpu_only_app() else {
            return;
        };
        assert!(app.selected_entry().is_none());
        let texts = accessible_texts(
            |ui| params_ui(ui, &mut app),
            |node| node.value().or_else(|| node.label()),
        );
        assert!(texts.iter().any(|text| text == NO_MODEL_SELECTED), "{texts:?}");
        assert!(
            !texts
                .iter()
                .any(|text| text == "Generate random seed" || text.contains("no parameters")),
            "{texts:?}"
        );
    }

    /// The dice and remove buttons carry names a screen reader can read, instead of their icon glyphs.
    #[test]
    fn the_icon_buttons_are_named_for_what_they_do() {
        let names = accessible_names(|ui| {
            dice_button(ui);
            remove_button(ui, "Seed outbreak at tick 5");
        });
        assert!(names.iter().any(|name| name == "Generate random seed"), "{names:?}");
        assert!(
            names.iter().any(|name| name == "Remove Seed outbreak at tick 5"),
            "{names:?}"
        );
        assert!(
            !names
                .iter()
                .any(|name| name == MDI_DICE_5 || name == MDI_DELETE_OUTLINE),
            "{names:?}"
        );
    }
}
