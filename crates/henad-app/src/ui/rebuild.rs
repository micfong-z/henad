//! Modal asking before the loaded model is rebuilt under replaced code. A rebuild starts the run again from tick 0.

use egui::{Context, Id, Modal};

use crate::state::{AppState, PendingRebuild};
use crate::ui::editor::sheet::widgets::{DialogButton, DialogButtonKind, dialog_buttons, dialog_frame, dialog_title};
use crate::ui::plural;

/// Title of the modal.
const REBUILD_HEADING: &str = "Rebuild model?";

/// Width in points of the modal's content.
const DIALOG_WIDTH: f32 = 420.0;

/// Line saying what a rebuild does to the run.
const REBUILD_TEXT: &str = "The model will be rebuilt from tick 0. The current run will be lost.";

/// Asks before [`AppState::rebuild_loaded`] rebuilds the loaded model, while [`AppState::pending_rebuild`] holds a
/// rebuild.
///
/// Rebuild is disabled while [`AppState::rebuild_refusal`] returns a reason, with the reason as its tooltip. Enter
/// presses Rebuild. Cancel, Escape and a click beside the modal close it, and the run goes on under its own code. A
/// playing run waits while the modal is open, as [`AppState::hold_for_modal`] holds it.
pub fn rebuild_modal(ctx: &Context, app: &mut AppState) {
    let Some(pending) = &app.pending_rebuild else {
        return;
    };
    if app.loaded_id() != Some(pending.model.as_str()) {
        app.pending_rebuild = None;
        return;
    }
    let lines = remap_lines(pending);
    let refusal = app.rebuild_refusal();

    let mut rebuild = false;
    let mut closed = false;
    let modal = Modal::new(Id::new("henad_rebuild_modal")).frame(dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        dialog_title(ui, REBUILD_HEADING, REBUILD_HEADING, DIALOG_WIDTH);
        ui.label(REBUILD_TEXT);
        for line in &lines {
            ui.label(line);
        }
        let buttons = [
            DialogButton::new("Cancel", DialogButtonKind::Plain),
            DialogButton {
                refusal: refusal.as_deref(),
                ..DialogButton::new("Rebuild", DialogButtonKind::Destructive)
            },
        ];
        match dialog_buttons(ui, &buttons) {
            Some(0) => closed = true,
            Some(_) => rebuild = true,
            None => {}
        }
    });

    if rebuild {
        app.rebuild_loaded();
    } else if closed || response.should_close() {
        app.pending_rebuild = None;
    }
}

/// Returns the lines that name what the new code does not take from the run, as in "2 parameters will use their
/// defaults.", empty when the rebuild keeps every value and action.
fn remap_lines(pending: &PendingRebuild) -> Vec<String> {
    let mut lines = Vec::new();
    let params = pending.defaulted_params;
    if params > 0 {
        let (noun, possessive) = if params == 1 {
            ("parameter", "its default")
        } else {
            ("parameters", "their defaults")
        };
        lines.push(format!("{params} {noun} will use {possessive}."));
    }
    let actions = pending.dropped_actions as u64;
    if actions > 0 {
        lines.push(format!(
            "{actions} scheduled {} will be removed.",
            plural(actions, "action")
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::remap_lines;
    use crate::state::PendingRebuild;

    #[test]
    fn the_modal_names_what_the_rebuild_drops() {
        let pending = |defaulted_params, dropped_actions| PendingRebuild {
            model: "sir".to_owned(),
            defaulted_params,
            dropped_actions,
        };
        assert!(remap_lines(&pending(0, 0)).is_empty());
        assert_eq!(remap_lines(&pending(1, 0)), ["1 parameter will use its default."]);
        assert_eq!(
            remap_lines(&pending(2, 1)),
            [
                "2 parameters will use their defaults.",
                "1 scheduled action will be removed."
            ]
        );
        assert_eq!(remap_lines(&pending(0, 3)), ["3 scheduled actions will be removed."]);
    }
}
