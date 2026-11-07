//! The New model dialog: a sheet model started from one of the sheet starters, under a name.
//!
//! "Start from" lists the starters [`henad_graph::sheet::starters`] returns, each with its one-line description. The
//! Name field follows the chosen starter's name until the author types in it. Create opens the sheet in the Editor,
//! and so does Enter in the Name field. Cancel, Escape and a click beside the dialog close it.

use egui::{Context, Id, Key, Modal, RichText, TextEdit};
use henad_graph::sheet::ParsedSheet;
use henad_graph::sheet::edit::{SheetBuilder, SheetEdit};
use henad_graph::sheet::sentence::{
    CANCEL, CREATE, NAME_LABEL, NEW_MODEL, START_FROM, STARTER_CHOICES, UNTITLED_MODEL,
};

use super::SheetEditor;
use crate::state::AppState;
use crate::ui::dock::Tab;
use crate::ui::sweep::layout::filled_button;

/// Width in points of the dialog's content.
const DIALOG_WIDTH: f32 = 420.0;

/// One starter the dialog offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterChoice {
    /// File name of the starter in [`henad_graph::sheet::starters`].
    pub file_name: &'static str,
    /// Name of the model the starter holds.
    pub name: String,
    /// One-line description the dialog shows under the name, `None` for a starter the wording table lacks.
    pub description: Option<&'static str>,
}

/// Returns the starters the dialog offers, in the order [`henad_graph::sheet::starters`] returns them.
pub fn starter_choices() -> Vec<StarterChoice> {
    henad_graph::sheet::starters()
        .iter()
        .filter_map(|starter| {
            let name = ParsedSheet::parse(starter.text).ok()?.document().sheet.name.clone();
            let description = STARTER_CHOICES
                .iter()
                .find(|(choice, _)| *choice == name)
                .map(|(_, description)| *description);
            Some(StarterChoice {
                file_name: starter.file_name,
                name,
                description,
            })
        })
        .collect()
}

/// State of the New model dialog while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewModelDialog {
    /// Starters the dialog offers.
    choices: Vec<StarterChoice>,
    /// Index in [`Self::choices`] of the chosen starter.
    pub chosen: usize,
    /// Text of the Name field.
    pub name: String,
    /// Whether the author typed in the Name field. Until then the name follows the chosen starter.
    pub name_typed: bool,
}

impl Default for NewModelDialog {
    fn default() -> Self {
        let choices = starter_choices();
        let name = choices
            .first()
            .map_or_else(|| UNTITLED_MODEL.to_owned(), |choice| choice.name.clone());
        Self {
            choices,
            chosen: 0,
            name,
            name_typed: false,
        }
    }
}

impl NewModelDialog {
    /// Starters the dialog offers.
    #[cfg(test)]
    pub fn choices(&self) -> &[StarterChoice] {
        &self.choices
    }

    /// Chooses the starter at `index`, whose name the Name field takes until the author types in it.
    pub fn choose(&mut self, index: usize) {
        self.chosen = index;
        if !self.name_typed {
            self.name = self
                .choices
                .get(index)
                .map_or_else(|| UNTITLED_MODEL.to_owned(), |choice| choice.name.clone());
        }
    }
}

/// Opens the New model dialog.
pub fn open_new_model(app: &mut AppState) {
    app.editor.new_model = Some(NewModelDialog::default());
}

/// Draws the New model dialog while [`crate::ui::editor::EditorPanel::new_model`] holds it, and creates the model on
/// Create.
pub fn new_model_modal(ctx: &Context, app: &mut AppState) {
    let Some(dialog) = &mut app.editor.new_model else {
        return;
    };
    let mut create = false;
    let mut closed = false;
    let response = Modal::new(Id::new("henad_new_model_modal")).show(ctx, |ui| {
        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
            node.set_role(egui::accesskit::Role::Dialog);
            node.set_label(NEW_MODEL);
        });
        ui.set_max_width(DIALOG_WIDTH);
        ui.heading(NEW_MODEL);
        ui.add_space(4.0);
        ui.label(RichText::new(START_FROM).strong());
        let mut chosen = dialog.chosen;
        for (index, choice) in dialog.choices.iter().enumerate() {
            ui.radio_value(&mut chosen, index, &choice.name);
            if let Some(description) = choice.description {
                ui.indent(choice.file_name, |ui| ui.weak(description));
            }
        }
        if chosen != dialog.chosen {
            dialog.choose(chosen);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = ui.label(NAME_LABEL);
            let field = ui.add(TextEdit::singleline(&mut dialog.name)).labelled_by(label.id);
            if field.changed() {
                dialog.name_typed = true;
            }
            if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                create = true;
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(CANCEL).clicked() {
                closed = true;
            }
            if ui.add(filled_button(CREATE.to_owned())).clicked() {
                create = true;
            }
        });
    });
    if create {
        let chosen = dialog.choices.get(dialog.chosen).map(|choice| choice.file_name);
        let name = dialog.name.trim().to_owned();
        app.editor.new_model = None;
        if let Some(file_name) = chosen {
            create_model(app, file_name, &name);
        }
    } else if closed || response.should_close() {
        app.editor.new_model = None;
    }
}

/// Opens the sheet starter whose file is named `file_name` in the Editor under the name `name`, and brings the Editor
/// to the front.
///
/// An empty name takes [`UNTITLED_MODEL`]. A name the sheet refuses leaves the starter's name, and the Editor's status
/// says why.
pub fn create_model(app: &mut AppState, file_name: &str, name: &str) {
    let Some(parsed) = henad_graph::sheet::starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .and_then(|starter| ParsedSheet::parse(starter.text).ok())
    else {
        app.editor.file_status = Some(format!("Starter {file_name} does not read"));
        return;
    };
    let name = if name.is_empty() { UNTITLED_MODEL } else { name };
    let mut document = parsed.into_document();
    let mut status = None;
    if document.sheet.name != name {
        let mut builder = SheetBuilder::new(document.clone());
        match builder.apply(SheetEdit::SetSheetName { name: name.to_owned() }) {
            Ok(_) => document = builder.into_document(),
            Err(error) => status = Some(format!("Name not kept: {error}")),
        }
    }
    app.editor.load_sheet(SheetEditor::new(document), None);
    app.editor.file_status = status;
    app.focus_request = Some(Tab::Editor);
}
