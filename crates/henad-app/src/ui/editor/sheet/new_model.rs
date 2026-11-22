//! The New model dialog: a sheet model started blank or from one of the sheet starters, under a name.
//!
//! "Start from" lists Blank, then the starters [`henad_graph::sheet::starters`] returns, grid and agent starters alike,
//! each a selectable row with its one-line description under its name. Under Blank alone the dialog asks what the world is made of: cells on a grid
//! or agents in open space. The Name field follows the chosen starter's name, or "Untitled model" for Blank, until the
//! author types in it. Create opens the sheet in the Editor, and so does Enter in the Name field. Cancel, Escape and a
//! click beside the dialog close it.
//!
//! A blank model is a world of 100 by 100 cells that wraps around, with no parameter and no card. A blank world of
//! cells has no property. A blank world of agents holds 1000 agents called Agent, room for a million, each with a
//! Position that starts anywhere at random and a Heading.
//!
//! A blank model takes the id its name mints, and a starter keeps its own. Where a model compiled into the app holds
//! that id, such as the example `game_of_life`, the new model takes `sheet_` before it, so Apply adds it beside that
//! model instead of replacing it.

use egui::{Context, Id, Modal, TextEdit};
use henad_graph::sheet::ParsedSheet;
use henad_graph::sheet::document::{
    AgentKind, Edges, MadeOf, PropertyType, SheetDocument, SheetHeader, SheetWorld, StartRule, StartsAs,
};
use henad_graph::sheet::edit::{IdSpace, SheetBuilder, SheetEdit, mint_id};
use henad_graph::sheet::sentence::{
    AGENTS_IN_OPEN_SPACE, BLANK_AGENT_NAME, CANCEL, CELLS_ON_A_GRID, CREATE, HEADING_KIND, NAME_LABEL, NEW_MODEL,
    POSITION_KIND, START_FROM, STARTER_CHOICES, UNTITLED_MODEL, WORLD_QUESTION,
};

use super::SheetEditor;
use super::widgets::{
    DialogButton, DialogButtonKind, caption, choice_row, dialog_buttons, dialog_frame, dialog_list, dialog_title,
};
use crate::state::AppState;
use crate::ui::dock::Tab;
use crate::ui::editor::{FileStatus, new_model_id};

/// Width in points of the dialog's content.
const DIALOG_WIDTH: f32 = 420.0;

/// Name of the first choice of the dialog, the model with nothing in it.
const BLANK: &str = "Blank";

/// Side in cells of a blank model's grid.
const BLANK_SIDE: u32 = 100;

/// Number of agents of a blank model of agents at the start.
const BLANK_AGENT_COUNT: u32 = 1000;

/// Largest number of agents a blank model of agents holds.
const BLANK_AGENT_MAX: u32 = 1_000_000;

/// Text before the id of a new sheet whose own id a compiled model holds, as before the ids of the sheet starters of
/// example models.
const SHEET_ID_PREFIX: &str = "sheet_";

/// One starter the dialog offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterChoice {
    /// File name of the starter in [`henad_graph::sheet::starters`], `None` for Blank.
    pub file_name: Option<&'static str>,
    /// Name of the model the starter holds.
    pub name: String,
    /// One-line description the dialog shows under the name, `None` for a starter the wording table lacks.
    pub description: Option<&'static str>,
}

/// Returns the starters the dialog offers: Blank, then the starters in the order [`henad_graph::sheet::starters`]
/// returns them.
pub fn starter_choices() -> Vec<StarterChoice> {
    let blank = StarterChoice {
        file_name: None,
        name: BLANK.to_owned(),
        description: description_of(BLANK),
    };
    let starters = henad_graph::sheet::starters().iter().filter_map(|starter| {
        let parsed = ParsedSheet::parse(starter.text).ok()?;
        let name = parsed.document().sheet.name.clone();
        Some(StarterChoice {
            file_name: Some(starter.file_name),
            description: description_of(&name),
            name,
        })
    });
    std::iter::once(blank).chain(starters).collect()
}

/// Returns the one-line description of the starter named `name`, `None` for a starter the wording table lacks.
fn description_of(name: &str) -> Option<&'static str> {
    STARTER_CHOICES
        .iter()
        .find(|(choice, _)| *choice == name)
        .map(|(_, description)| *description)
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
    /// Kind of thing a blank world is made of.
    pub made_of: MadeOf,
}

impl Default for NewModelDialog {
    fn default() -> Self {
        Self {
            choices: starter_choices(),
            chosen: 0,
            name: UNTITLED_MODEL.to_owned(),
            name_typed: false,
            made_of: MadeOf::Cells,
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
                .and_then(|choice| choice.file_name.map(|_| choice.name.clone()))
                .unwrap_or_else(|| UNTITLED_MODEL.to_owned());
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
    let modal = Modal::new(Id::new("henad_new_model_modal")).frame(dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        dialog_title(ui, NEW_MODEL, NEW_MODEL, DIALOG_WIDTH);
        caption(ui, START_FROM);
        ui.add_space(4.0);
        // The Name row and the buttons, each a row after 12 points of space.
        let below = 2.0 * (12.0 + ui.spacing().interact_size.y + ui.spacing().item_spacing.y);
        let chosen = dialog_list(ui, below, |ui| {
            let mut chosen = dialog.chosen;
            for (index, choice) in dialog.choices.iter().enumerate() {
                if choice_row(ui, &choice.name, choice.description, dialog.chosen == index).clicked() {
                    chosen = index;
                }
                if choice.file_name.is_none() && dialog.chosen == index {
                    ui.scope(|ui| {
                        // The question stands under Blank by its indent alone, with no line at its left.
                        ui.visuals_mut().indent_has_left_vline = false;
                        ui.indent(("henad_new_model_choice", index), |ui| {
                            world_question(ui, &mut dialog.made_of);
                        });
                    });
                }
            }
            chosen
        });
        if chosen != dialog.chosen {
            dialog.choose(chosen);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let label = ui.label(NAME_LABEL);
            let width = ui.available_width();
            let field = ui
                .add(TextEdit::singleline(&mut dialog.name).desired_width(width))
                .labelled_by(label.id);
            if field.changed() {
                dialog.name_typed = true;
            }
        });
        let buttons = [
            DialogButton::new(CANCEL, DialogButtonKind::Plain),
            DialogButton::new(CREATE, DialogButtonKind::Primary),
        ];
        match dialog_buttons(ui, &buttons) {
            Some(0) => closed = true,
            Some(_) => create = true,
            None => {}
        }
    });
    if create {
        let chosen = dialog.choices.get(dialog.chosen).map(|choice| choice.file_name);
        let name = dialog.name.trim().to_owned();
        let made_of = dialog.made_of;
        app.editor.new_model = None;
        match chosen {
            Some(Some(file_name)) => create_model(app, file_name, &name),
            Some(None) if made_of == MadeOf::Agents => create_blank_agent_model(app, &name),
            Some(None) => create_blank_model(app, &name),
            None => {}
        }
    } else if closed || response.should_close() {
        app.editor.new_model = None;
    }
}

/// Draws the world question under Blank: a world of cells on a grid or of agents in open space, chosen in `made_of`.
fn world_question(ui: &mut egui::Ui, made_of: &mut MadeOf) {
    ui.add_space(4.0);
    ui.label(WORLD_QUESTION);
    ui.radio_value(made_of, MadeOf::Cells, CELLS_ON_A_GRID);
    ui.radio_value(made_of, MadeOf::Agents, AGENTS_IN_OPEN_SPACE);
    ui.add_space(4.0);
}

/// Returns a blank model named `name`: a world of 100 by 100 cells that wraps around, with nothing else.
///
/// The model's id is minted from its name, or is `untitled_model` for a name that mints no id.
pub fn blank_model(name: &str) -> SheetDocument {
    let id = mint_id(name, IdSpace::Card, |_| false).unwrap_or_else(|_| "untitled_model".to_owned());
    SheetDocument::new(
        SheetHeader::new(id, name, 1),
        SheetWorld::new(MadeOf::Cells, BLANK_SIDE, BLANK_SIDE, Edges::Wrap),
    )
}

/// Returns a blank model of agents named `name`: 1000 agents called Agent in a world 100 by 100 that wraps around, each
/// with a Position that starts anywhere at random and a Heading.
///
/// The model's id is minted from its name, as [`blank_model`] mints it.
pub fn blank_agent_model(name: &str) -> SheetDocument {
    let mut document = blank_model(name);
    document.world = SheetWorld::new(MadeOf::Agents, BLANK_SIDE, BLANK_SIDE, Edges::Wrap);
    let kind = mint_id(BLANK_AGENT_NAME, IdSpace::Thing, |_| false).unwrap_or_default();
    document.agents.push(AgentKind::new(
        kind.clone(),
        BLANK_AGENT_NAME,
        BLANK_AGENT_COUNT,
        BLANK_AGENT_MAX,
    ));
    let mut builder = SheetBuilder::new(document.clone());
    let built = (|| {
        let position = builder.apply(SheetEdit::AddProperty {
            thing: kind.clone(),
            name: POSITION_KIND.to_owned(),
            property_type: PropertyType::Position,
        })?;
        builder.apply(SheetEdit::SetStartsAs {
            property: position.into_iter().next().unwrap_or_default(),
            starts_as: Some(Box::new(StartsAs::new(StartRule::Anywhere))),
        })?;
        builder.apply(SheetEdit::AddProperty {
            thing: kind,
            name: HEADING_KIND.to_owned(),
            property_type: PropertyType::Heading,
        })
    })();
    // The commands run on a fresh kind of agent, which takes all three.
    match built {
        Ok(_) => builder.into_document(),
        Err(_) => document,
    }
}

/// Opens a blank model named `name` in the Editor, and brings the Editor to the front. An empty name takes
/// [`UNTITLED_MODEL`].
pub fn create_blank_model(app: &mut AppState, name: &str) {
    open_blank(app, blank_model(if name.is_empty() { UNTITLED_MODEL } else { name }));
}

/// Opens a blank model of agents named `name` in the Editor, as [`create_blank_model`] opens one of cells.
pub fn create_blank_agent_model(app: &mut AppState, name: &str) {
    open_blank(
        app,
        blank_agent_model(if name.is_empty() { UNTITLED_MODEL } else { name }),
    );
}

/// Opens the blank model `document` in the Editor with no file name, and brings the Editor to the front.
///
/// The model keeps the id its name mints unless a compiled model of the app's set holds it, and then takes `sheet_`
/// before it ([`new_model_id`]).
fn open_blank(app: &mut AppState, mut document: SheetDocument) {
    document.sheet.id = new_model_id(&app.models, &document.sheet.id, SHEET_ID_PREFIX);
    app.editor.load_sheet(SheetEditor::new(document), None);
    app.editor.file_status = None;
    app.focus_request = Some(Tab::Editor);
}

/// Opens the sheet starter whose file is named `file_name` in the Editor under the name `name`, and brings the Editor
/// to the front.
///
/// An empty name takes [`UNTITLED_MODEL`]. A name the sheet refuses leaves the starter's name, and the Editor's status
/// says why. The model keeps the starter's id unless a compiled model of the app's set holds it ([`new_model_id`]).
pub fn create_model(app: &mut AppState, file_name: &str, name: &str) {
    let Some(parsed) = henad_graph::sheet::starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .and_then(|starter| ParsedSheet::parse(starter.text).ok())
    else {
        app.editor.file_status = Some(FileStatus::failure(format!("Starter {file_name} cannot be read")));
        return;
    };
    let name = if name.is_empty() { UNTITLED_MODEL } else { name };
    let mut document = parsed.into_document();
    document.sheet.id = new_model_id(&app.models, &document.sheet.id, SHEET_ID_PREFIX);
    let mut status = None;
    if document.sheet.name != name {
        let mut builder = SheetBuilder::new(document.clone());
        match builder.apply(SheetEdit::SetSheetName { name: name.to_owned() }) {
            Ok(_) => document = builder.into_document(),
            Err(error) => status = Some(FileStatus::failure(format!("Name not kept: {error}"))),
        }
    }
    app.editor.load_sheet(SheetEditor::new(document), None);
    app.editor.file_status = status;
    app.focus_request = Some(Tab::Editor);
}
