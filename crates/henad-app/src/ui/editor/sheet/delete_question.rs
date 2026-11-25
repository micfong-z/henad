//! The dialog that asks before a delete empties formulas that the editor cannot write again.
//!
//! A delete of a parameter, a property or a line, and the removal of a summary of a look at agents, empty every formula
//! that names the item. The card writes again a formula of one operation on a Calculate line and the length of one of
//! my properties in a slot, and such a delete runs at once. Any other formula, a measure's included, is written in the
//! model file alone. Its delete waits for Delete, or Remove for a summary, in this dialog, which names the lines and
//! the measures whose formulas it empties.

use egui::{Context, Id, Modal};
use henad_graph::sheet::document::SheetDocument;
use henad_graph::sheet::edit::{SheetEdit, lines_losing_formulas, measures_losing_formulas};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{CANCEL, DELETE, REMOVE, summary_used_by, used_by};

use super::card_lines::button_of;
use super::widgets::{DialogButton, DialogButtonKind, dialog_buttons, dialog_frame, dialog_title};

/// Width in points of the dialog.
const DIALOG_WIDTH: f32 = 420.0;

/// A delete that waits on the dialog, with the dialog's title, question and action.
#[derive(Debug, Clone)]
pub struct DeleteQuestion {
    /// The delete.
    pub edit: SheetEdit,
    /// Title of the dialog, such as "Delete parameter?".
    title: &'static str,
    /// Question of the dialog, such as "Trail falloff is used by lines 3 and 4. Delete it?".
    question: String,
    /// Words of the button that runs the delete, Delete or Remove.
    action: &'static str,
}

/// Answer given in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteAnswer {
    /// Delete or Remove was pressed.
    Delete,
    /// Cancel was pressed, or the dialog was closed.
    Cancel,
}

impl DeleteQuestion {
    /// Returns the question to ask, under the title `title`, before `edit` deletes the item of `document` named `name`,
    /// or `None` for a delete that empties no formula the editor cannot write again, which runs without asking.
    ///
    /// The removal of a summary asks "Remove it?" with Remove, and any other delete "Delete it?" with Delete.
    pub fn before(document: &SheetDocument, edit: &SheetEdit, title: &'static str, name: &str) -> Option<Self> {
        let lines = lines_losing_formulas(document, edit);
        let measures = measures_losing_formulas(document, edit);
        if lines.is_empty() && measures.is_empty() {
            return None;
        }
        let places: Vec<(Option<&str>, usize)> = lines.iter().filter_map(|id| line_place(document, id)).collect();
        let measures: Vec<&str> = measures
            .iter()
            .filter_map(|id| document.measures.iter().find(|measure| measure.id == *id))
            .map(|measure| measure.name.as_str())
            .collect();
        let (question, action) = match edit {
            SheetEdit::DeleteSummary { .. } => (summary_used_by(name, &places), REMOVE),
            _ => (used_by(name, &places, &measures), DELETE),
        };
        Some(Self {
            edit: edit.clone(),
            title,
            question,
            action,
        })
    }

    /// Draws the dialog: its title, the question, then Cancel and Delete or Remove. Returns the answer once one is
    /// given, `None` while the dialog waits.
    ///
    /// Enter presses Delete or Remove. Cancel, Escape and a click outside close the dialog.
    pub fn show(&self, ctx: &Context) -> Option<DeleteAnswer> {
        let mut answer = None;
        let modal = Modal::new(Id::new("henad_delete_question")).frame(dialog_frame(ctx));
        let response = modal.show(ctx, |ui| {
            dialog_title(ui, self.title, self.title, DIALOG_WIDTH);
            ui.label(&self.question);
            let buttons = [
                DialogButton::new(CANCEL, DialogButtonKind::Plain),
                DialogButton::new(self.action, DialogButtonKind::Destructive),
            ];
            answer = dialog_buttons(ui, &buttons).map(|pressed| {
                if pressed == 1 {
                    DeleteAnswer::Delete
                } else {
                    DeleteAnswer::Cancel
                }
            });
        });
        answer.or_else(|| response.should_close().then_some(DeleteAnswer::Cancel))
    }
}

/// Returns the button of the card that holds line `id` of `document`, `None` for a card that runs each tick, and the
/// line's number on that card.
fn line_place<'a>(document: &'a SheetDocument, id: &str) -> Option<(Option<&'a str>, usize)> {
    let (index, _) = document.line(id)?;
    let card = document.cards.get(index)?;
    let number = CardRelations::new(document, card).number(id)?;
    Some((button_of(document, card), number))
}
