//! The dialog that asks before an edit takes away parts of the sheet besides the item it deletes or changes.
//!
//! A delete clears every reference to what it deletes. A line that used the item loses the value, a start falls back
//! on another state, and a measure, a colour rule or a shade about the item goes. A new kind for a property does the
//! same, a new start drops the shapes a painted start held, and new colours drop the shades of the rule they replace.
//! An edit that takes nothing else away runs at once. Any other waits for the dialog's action, Delete, Remove, Change
//! or Replace, and the dialog names each part the edit takes away, the lines by the numbers the card shows.

use egui::{Context, Id, Modal};
use henad_graph::sheet::document::{CELLS_ID, GROUND_ID, SheetDocument};
use henad_graph::sheet::edit::{SheetEdit, SheetLosses};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{CANCEL, ColoredThing, DELETE, LostParts, REMOVE, lost_question};

use super::card_lines::button_of;
use super::find_property;
use super::widgets::{DialogButton, DialogButtonKind, dialog_buttons, dialog_frame, dialog_title};

/// Width in points of the dialog.
const DIALOG_WIDTH: f32 = 420.0;

/// Title, question and action of the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// Title of the dialog, such as "Delete parameter?".
    title: &'static str,
    /// Question of the dialog, such as "Lines 3 and 4 will lose Trail falloff.".
    text: String,
    /// Words of the button that runs the edit, such as Delete.
    action: &'static str,
}

/// Answer given in the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteAnswer {
    /// The action was pressed.
    Delete,
    /// Cancel was pressed, or the dialog was closed.
    Cancel,
}

impl Question {
    /// Returns the question `text` under the title `title`, with the action `action`.
    pub fn new(title: &'static str, text: String, action: &'static str) -> Self {
        Self { title, text, action }
    }

    /// Returns the question to ask, under the title `title` and with the action `action`, before an edit takes
    /// `losses` away from `document`, where `name` is the name of what the edit deletes or changes. `None` when the
    /// edit takes nothing away.
    pub fn of_losses(
        document: &SheetDocument,
        losses: &SheetLosses,
        title: &'static str,
        name: &str,
        action: &'static str,
    ) -> Option<Self> {
        if losses.is_empty() {
            return None;
        }
        Some(Self::new(
            title,
            lost_question(name, &lost_parts(document, losses)),
            action,
        ))
    }

    /// Draws the dialog: its title, the question, then Cancel and the action. Returns the answer once one is given,
    /// `None` while the dialog waits.
    ///
    /// Enter presses the action. Cancel, Escape and a click outside close the dialog.
    pub fn show(&self, ctx: &Context) -> Option<DeleteAnswer> {
        let mut answer = None;
        let modal = Modal::new(Id::new("henad_delete_question")).frame(dialog_frame(ctx));
        let response = modal.show(ctx, |ui| {
            dialog_title(ui, self.title, self.title, DIALOG_WIDTH);
            ui.label(&self.text);
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

/// An edit of one command that waits on the dialog, with the dialog's question.
#[derive(Debug, Clone)]
pub struct DeleteQuestion {
    /// The edit.
    pub edit: SheetEdit,
    /// Title, question and action of the dialog.
    question: Question,
}

impl DeleteQuestion {
    /// Returns the question to ask, under the title `title`, before `edit` deletes the item of `document` named `name`,
    /// or `None` for a delete that takes nothing away besides the item, which runs without asking.
    ///
    /// The removal of a summary takes Remove as its action, and any other delete Delete.
    pub fn before(document: &SheetDocument, edit: &SheetEdit, title: &'static str, name: &str) -> Option<Self> {
        let action = match edit {
            SheetEdit::DeleteSummary { .. } => REMOVE,
            _ => DELETE,
        };
        let question = edit_question(document, edit, title, name, action)?;
        Some(Self {
            edit: edit.clone(),
            question,
        })
    }

    /// Draws the dialog as [`Question::show`] does.
    pub fn show(&self, ctx: &Context) -> Option<DeleteAnswer> {
        self.question.show(ctx)
    }
}

/// Returns the question to ask, under the title `title` and with the action `action`, before `edit` takes parts of
/// `document` away besides the item named `name` that it deletes or sets. `None` for an edit that takes nothing else
/// away.
pub fn edit_question(
    document: &SheetDocument,
    edit: &SheetEdit,
    title: &'static str,
    name: &str,
    action: &'static str,
) -> Option<Question> {
    Question::of_losses(document, &SheetLosses::of_edit(document, edit), title, name, action)
}

/// Returns `losses`, ids of `document`, named as the editor shows them: each line by its card's button and its
/// number, each other item by its name. An id the document does not hold is left out.
fn lost_parts<'a>(document: &'a SheetDocument, losses: &'a SheetLosses) -> LostParts<'a> {
    let lines = |ids: &[String]| ids.iter().filter_map(|id| line_place(document, id)).collect();
    let property = |id: &String| find_property(document, id).map(|property| property.name.as_str());
    let properties = |ids: &'a [String]| ids.iter().filter_map(property).collect();
    let measures = |ids: &'a [String]| {
        ids.iter()
            .filter_map(|id| document.measures.iter().find(|measure| measure.id == *id))
            .map(|measure| measure.name.as_str())
            .collect()
    };
    let mut lost = LostParts::default();
    lost.lines = lines(&losses.lines);
    lost.starts = properties(&losses.starts);
    lost.start_cells = losses
        .start_cells
        .iter()
        .filter_map(|(id, ground, state)| {
            let ground = find_property(document, ground)?;
            let state = ground.states.iter().find(|known| known.id == *state)?;
            Some((property(id)?, state.name.as_str()))
        })
        .collect();
    lost.kept = properties(&losses.kept);
    lost.buttons = losses
        .buttons
        .iter()
        .filter_map(|id| document.buttons.iter().find(|button| button.id == *id))
        .map(|button| button.name.as_str())
        .collect();
    lost.measures = measures(&losses.measures);
    lost.deleted_lines = lines(&losses.deleted_lines);
    lost.removed_measures = measures(&losses.removed_measures);
    lost.colors = losses
        .colors
        .iter()
        .filter_map(|(thing, id)| Some((colored_thing(thing), property(id)?)))
        .collect();
    lost.shades = losses
        .shades
        .iter()
        .map(|(thing, ids)| (colored_thing(thing), ids.iter().filter_map(property).collect()))
        .collect();
    lost.shapes = losses
        .shapes
        .iter()
        .filter_map(|(id, count)| Some((property(id)?, *count)))
        .collect();
    lost
}

/// Returns the kind of thing of id `thing` as the dialog names it: the cells, the ground, or the agents.
fn colored_thing(thing: &str) -> ColoredThing {
    match thing {
        CELLS_ID => ColoredThing::Cells,
        GROUND_ID => ColoredThing::Ground,
        _ => ColoredThing::Agents,
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
