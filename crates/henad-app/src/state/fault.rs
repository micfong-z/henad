//! Faults of the live simulation: the modal that shows one, or the Editor for a fault of the code it applied.

use henad_compute::entry::ModelEntry;
use henad_compute::fault::{BUILDING, Fault};

use super::AppState;
use crate::ui::dock::Tab;
use crate::ui::editor::EditorPanel;

/// Fault the modal shows, with the model it stopped.
#[derive(Debug)]
pub struct ShownFault {
    pub fault: Fault,
    /// Name of the model the fault stopped, `None` when no model was building or loaded.
    pub model: Option<String>,
}

impl AppState {
    /// Offloads the live simulation and passes the fault to the modal, or to the Editor for a fault of the code the
    /// Editor applied. A running sweep carries on.
    ///
    /// A fault while building belongs to the selected model, and any other to the loaded one. The Model panel stays
    /// usable while a model runs, and the selection can be another model by then.
    ///
    /// While a build runs on its own thread, the fault goes to the modal. The build thread returns the build's own
    /// faults, and a fault in a sink meanwhile comes from a detached run or from rendering.
    pub fn report_fault(&mut self, fault: Fault) {
        let model = if fault.during == BUILDING {
            self.selected_entry()
        } else {
            self.loaded_entry()
        };
        let model = model.cloned();
        if self.is_building() {
            self.show_fault_in_modal(fault, model.as_ref());
        } else {
            self.show_fault(fault, model.as_ref());
        }
    }

    /// Offloads the live simulation and passes the fault to the modal, naming `model` as the model it stopped.
    ///
    /// A fault of a run built from the code the Editor applied last, before the run passes tick 0, goes to the Editor
    /// instead, as the outcome of the edit, when an edit of the code can fix it ([`EditorPanel::claims`]). So does a
    /// fault of the Editor's preview at any tick, while the Editor holds a graph or a sheet to show it under. The fault
    /// of a preview that outlived its model goes to the modal.
    ///
    /// On native the build thread returns the fault of a build in a later frame than the press that started it, and in
    /// a browser the device reports a shader it refuses after the build that created it.
    pub(super) fn show_fault(&mut self, fault: Fault, model: Option<&ModelEntry>) {
        if self.preview.is_some() && self.editor.model.holds_preview_model() && EditorPanel::is_code_fault(&fault) {
            log::error!("{fault}");
            self.offload_simulation();
            self.editor.show_preview_fault(&fault);
            return;
        }
        let claimed = self.current_tick() == 0 && model.is_some_and(|entry| self.editor.claims(entry.source(), &fault));
        if claimed {
            log::error!("{fault}");
            self.offload_simulation();
            self.editor.show_fault(&fault);
            self.focus_request = Some(Tab::Editor);
        } else {
            self.show_fault_in_modal(fault, model);
        }
    }

    /// Offloads the live simulation and passes the fault to the modal, naming `model` as the model it stopped, for a
    /// fault the Editor never claims.
    pub(super) fn show_fault_in_modal(&mut self, fault: Fault, model: Option<&ModelEntry>) {
        log::error!("{fault}");
        self.offload_simulation();
        self.fault = Some(ShownFault {
            fault,
            model: model.map(|entry| entry.name().to_owned()),
        });
    }
}

#[cfg(test)]
mod tests {
    use henad_compute::fault::{BUILDING, Fault, STEPPING};

    use crate::state::AppState;

    /// The modal shows the name of the model that faulted. The selection can be another model by then.
    #[test]
    fn a_fault_names_the_model_it_stopped() {
        let Some(mut app) = AppState::headless(henad_models::example_models(), false) else {
            return;
        };
        let name = |app: &AppState, id: &str| app.models.get(id).map(|entry| entry.name().to_owned());
        app.reset_simulation();
        app.wait_for_build();
        assert_eq!(app.loaded_id(), Some("sir"));
        app.select_model("boids");
        app.report_fault(Fault::refused(STEPPING, "a test fault"));
        assert_eq!(
            app.fault.as_ref().and_then(|shown| shown.model.clone()),
            name(&app, "sir")
        );
        assert_eq!(app.loaded_id(), None, "the faulted model is offloaded");

        app.report_fault(Fault::refused(BUILDING, "a test fault"));
        assert_eq!(
            app.fault.as_ref().and_then(|shown| shown.model.clone()),
            name(&app, "boids"),
            "a build fault is the selected model's"
        );
    }
}
