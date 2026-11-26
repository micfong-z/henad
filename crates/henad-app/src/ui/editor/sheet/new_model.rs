//! The New model dialog: a sheet model started blank or from one of the sheet starters, under a name, or a grid rule or
//! a node graph started from its starter.
//!
//! The dialog holds two collapsing headings, Sheet, open as the dialog opens, and Advanced, closed. Advanced holds two
//! more, Grid rule and Node graph. Opening a heading closes the other heading of its level. Create opens the model the
//! open heading offers, and is disabled while no heading that holds starters is open.
//!
//! Under Sheet, "Start from" lists Blank, then the starters [`henad_graph::sheet::starters`] returns, grid and agent
//! starters alike, each a selectable row with its one-line description under its name. Under Blank alone the dialog
//! asks what the world is made of: cells on a grid or agents in open space. The Name field follows the chosen starter's
//! name, or "Untitled model" for Blank, until the author types in it. Create opens the sheet in the Editor, and so does
//! Enter in the Name field. Cancel, Escape and a click beside the dialog close it.
//!
//! Grid rule offers the Life starter, which opens as text in the code view. Node graph lists the graph starters,
//! [`henad_graph::starters::starters`] then [`henad_graph::starters::agent_starters`], each with the description its
//! file holds, and opens the chosen one on the canvas. Both keep the name their starter holds.
//!
//! A blank model is a world of 100 by 100 cells that wraps around, with no parameter and no card. A blank world of
//! cells has no property. A blank world of agents holds 1000 agents called Agent, room for a million, each with a
//! Position that starts anywhere at random and a Heading.
//!
//! A blank model takes the id its name mints, and a starter keeps its own. Where a model compiled into the app holds
//! that id, such as the example `game_of_life`, the new model takes `sheet_` before it, so Apply adds it beside that
//! model instead of replacing it.

use egui::collapsing_header::CollapsingState;
use egui::{CollapsingHeader, Context, Id, Modal, TextEdit};
use henad_graph::GraphFile;
use henad_graph::sheet::ParsedSheet;
use henad_graph::sheet::document::{
    AgentKind, ColorRule, ColorRuleKind, Edges, MadeOf, PropertyType, SheetDocument, SheetHeader, SheetWorld,
    StartRule, StartsAs,
};
use henad_graph::sheet::edit::{IdSpace, SheetBuilder, SheetEdit, mint_id};
use henad_graph::sheet::sentence::{
    ADVANCED_HEADING, AGENTS_IN_OPEN_SPACE, BLANK_AGENT_NAME, CANCEL, CELLS_ON_A_GRID, CHOOSE_A_STARTER, CREATE,
    GRID_RULE_HEADING, GRID_RULE_STARTER, HEADING_KIND, NAME_LABEL, NEW_MODEL, NODE_GRAPH_HEADING, POSITION_KIND,
    SHEET_STARTERS_HEADING, START_FROM, STARTER_CHOICES, UNTITLED_MODEL, WORLD_QUESTION,
};

use super::SheetEditor;
use super::widgets::{
    DialogButton, DialogButtonKind, caption, choice_row, dialog_buttons, dialog_frame, dialog_list, dialog_title,
};
use crate::state::AppState;
use crate::ui::dock::Tab;
use crate::ui::editor::{FileStatus, new_graph_model, new_grid_rule, new_model_id};

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

/// One graph starter the dialog offers under Node graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphChoice {
    /// File name of the starter in [`henad_graph::starters::starters`] or [`henad_graph::starters::agent_starters`].
    pub file_name: &'static str,
    /// Name of the model the starter holds.
    pub name: String,
    /// Description the starter's file holds, `None` for an empty one.
    pub description: Option<String>,
}

/// Returns the graph starters the dialog offers: the grid starters, then the agent starters. A starter that does not
/// read is left out.
pub fn graph_choices() -> Vec<GraphChoice> {
    henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters())
        .filter_map(|starter| {
            let file = GraphFile::parse(starter.text).ok()?;
            let document = file.document();
            Some(GraphChoice {
                file_name: starter.file_name,
                name: document.name.clone(),
                description: (!document.description.is_empty()).then(|| document.description.clone()),
            })
        })
        .collect()
}

/// A collapsing heading of the New model dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewModelHeading {
    /// Sheet, over the sheet starters.
    Sheet,
    /// Advanced, over Grid rule and Node graph.
    Advanced,
    /// Grid rule, under Advanced, over the grid rule starter.
    GridRule,
    /// Node graph, under Advanced, over the graph starters.
    NodeGraph,
}

impl NewModelHeading {
    /// Words of the heading. They also name it.
    pub fn title(self) -> &'static str {
        match self {
            Self::Sheet => SHEET_STARTERS_HEADING,
            Self::Advanced => ADVANCED_HEADING,
            Self::GridRule => GRID_RULE_HEADING,
            Self::NodeGraph => NODE_GRAPH_HEADING,
        }
    }
}

/// Model that Create opens, by the open heading of the New model dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewModelKind {
    /// A sheet, blank or from the chosen sheet starter.
    Sheet,
    /// The grid rule starter.
    GridRule,
    /// The graph starter with this file name.
    NodeGraph(&'static str),
}

/// State of the New model dialog while it is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewModelDialog {
    /// Starters the dialog offers under Sheet.
    choices: Vec<StarterChoice>,
    /// Index in [`Self::choices`] of the chosen starter.
    pub chosen: usize,
    /// Text of the Name field.
    pub name: String,
    /// Whether the author typed in the Name field. Until then the name follows the chosen starter.
    pub name_typed: bool,
    /// Kind of thing a blank world is made of.
    pub made_of: MadeOf,
    /// Open heading of the top level, Sheet or Advanced, `None` while both are closed.
    pub open: Option<NewModelHeading>,
    /// Open heading under Advanced, Grid rule or Node graph, `None` while both are closed. It stays while Advanced is
    /// closed.
    pub open_advanced: Option<NewModelHeading>,
    /// Graph starters the dialog offers under Node graph.
    graphs: Vec<GraphChoice>,
    /// Index in [`Self::graphs`] of the chosen graph starter.
    pub graph_chosen: usize,
    /// Headings drawn since the dialog opened. A heading drawn the first time takes its open state from the dialog at
    /// once, without the state or the animation egui kept from an earlier dialog.
    drawn: Vec<NewModelHeading>,
}

impl Default for NewModelDialog {
    fn default() -> Self {
        Self {
            choices: starter_choices(),
            chosen: 0,
            name: UNTITLED_MODEL.to_owned(),
            name_typed: false,
            made_of: MadeOf::Cells,
            open: Some(NewModelHeading::Sheet),
            open_advanced: None,
            graphs: graph_choices(),
            graph_chosen: 0,
            drawn: Vec::new(),
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

    /// Returns whether `heading` is open. A heading under Advanced counts as open only while Advanced is open too.
    pub fn is_open(&self, heading: NewModelHeading) -> bool {
        match heading {
            NewModelHeading::Sheet | NewModelHeading::Advanced => self.open == Some(heading),
            NewModelHeading::GridRule | NewModelHeading::NodeGraph => {
                self.open == Some(NewModelHeading::Advanced) && self.open_advanced == Some(heading)
            }
        }
    }

    /// Opens `heading` when it is closed, which closes the other heading of its level, and closes it when it is open.
    pub fn toggle(&mut self, heading: NewModelHeading) {
        let level = match heading {
            NewModelHeading::Sheet | NewModelHeading::Advanced => &mut self.open,
            NewModelHeading::GridRule | NewModelHeading::NodeGraph => &mut self.open_advanced,
        };
        *level = if *level == Some(heading) { None } else { Some(heading) };
    }

    /// Returns the model Create opens, by the open heading: a sheet under Sheet, the grid rule under Grid rule and the
    /// chosen graph starter under Node graph. `None` while no such heading is open.
    pub fn kind(&self) -> Option<NewModelKind> {
        if self.is_open(NewModelHeading::Sheet) {
            Some(NewModelKind::Sheet)
        } else if self.is_open(NewModelHeading::GridRule) {
            Some(NewModelKind::GridRule)
        } else if self.is_open(NewModelHeading::NodeGraph) {
            self.graphs
                .get(self.graph_chosen)
                .map(|choice| NewModelKind::NodeGraph(choice.file_name))
        } else {
            None
        }
    }
}

/// Opens the New model dialog.
pub fn open_new_model(app: &mut AppState) {
    app.editor.new_model = Some(NewModelDialog::default());
}

/// Draws the New model dialog while [`crate::ui::editor::EditorPanel::new_model`] holds it, and opens the model on
/// Create.
///
/// The starters under Sheet and under Node graph each scroll in a list as tall as the window leaves, so the Name field,
/// the headings and the buttons stay in view.
pub fn new_model_modal(ctx: &Context, app: &mut AppState) {
    let Some(dialog) = &mut app.editor.new_model else {
        return;
    };
    let mut create = false;
    let mut closed = false;
    let modal = Modal::new(Id::new("henad_new_model_modal")).frame(dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        let top = ui.max_rect().top();
        dialog_title(ui, NEW_MODEL, NEW_MODEL, DIALOG_WIDTH);
        let sheet = heading(ui, dialog, NewModelHeading::Sheet, |ui, dialog| {
            sheet_starters(ui, dialog, top);
            false
        });
        let advanced = heading(ui, dialog, NewModelHeading::Advanced, |ui, dialog| {
            advanced_headings(ui, dialog, top)
        });
        let toggled = sheet || advanced;
        let create_button = DialogButton {
            refusal: dialog.kind().is_none().then_some(CHOOSE_A_STARTER),
            ..DialogButton::new(CREATE, DialogButtonKind::Primary)
        };
        let buttons = [DialogButton::new(CANCEL, DialogButtonKind::Plain), create_button];
        match dialog_buttons(ui, &buttons) {
            Some(0) => closed = true,
            // Enter on a focused heading opens or closes it, and creates nothing.
            Some(_) => create = !toggled,
            None => {}
        }
    });
    if create {
        let kind = dialog.kind();
        let chosen = dialog.choices.get(dialog.chosen).map(|choice| choice.file_name);
        let name = dialog.name.trim().to_owned();
        let made_of = dialog.made_of;
        app.editor.new_model = None;
        match (kind, chosen) {
            (Some(NewModelKind::Sheet), Some(Some(file_name))) => create_model(app, file_name, &name),
            (Some(NewModelKind::Sheet), Some(None)) if made_of == MadeOf::Agents => {
                create_blank_agent_model(app, &name);
            }
            (Some(NewModelKind::Sheet), Some(None)) => create_blank_model(app, &name),
            (Some(NewModelKind::GridRule), _) => new_grid_rule(app),
            (Some(NewModelKind::NodeGraph(file_name)), _) => new_graph_model(app, file_name),
            _ => {}
        }
    } else if closed || response.should_close() {
        app.editor.new_model = None;
    }
}

/// Draws `heading` of `dialog` as egui's collapsing header, open while the dialog holds it open, with `body` under it,
/// and returns whether the heading or a heading `body` draws was pressed.
///
/// The dialog decides whether the heading is open. A press toggles it in the dialog, which opens or closes the header
/// from the next frame. egui keeps the state of a header from an earlier dialog. A heading drawn the first time since
/// the dialog opened, whose header opens or closes from that state, takes the dialog's state at once with no
/// animation, and the frame is drawn again.
fn heading(
    ui: &mut egui::Ui,
    dialog: &mut NewModelDialog,
    heading: NewModelHeading,
    body: impl FnOnce(&mut egui::Ui, &mut NewModelDialog) -> bool,
) -> bool {
    let open = dialog.is_open(heading);
    let response = CollapsingHeader::new(heading.title())
        .id_salt(("henad_new_model_heading", heading.title()))
        .default_open(open)
        .open(Some(open))
        .show(ui, |ui| body(ui, dialog));
    if !dialog.drawn.contains(&heading) {
        dialog.drawn.push(heading);
        let settled = if open { 1.0 } else { 0.0 };
        if response.openness != settled {
            let ctx = ui.ctx();
            // The header's id is the id of its state and of its animation.
            let id = response.header_response.id;
            let mut state = CollapsingState::load_with_default_open(ctx, id, open);
            state.set_open(open);
            state.store(ctx);
            ctx.animate_bool_with_time(id, open, 0.0);
            ctx.request_discard("a heading of the New model dialog took the dialog's state");
        }
    }
    let pressed = response.header_response.clicked();
    if pressed {
        dialog.toggle(heading);
    }
    pressed || response.body_returned.unwrap_or(false)
}

/// Height in points of a row of a dialog: a button, a field or a heading, with the space after it.
fn row_height(ui: &egui::Ui) -> f32 {
    ui.spacing().interact_size.y + ui.spacing().item_spacing.y
}

/// Draws `add`, a list of starters, in [`dialog_list`] as tall as the window leaves, where `top` is the top of the
/// dialog's content and `below` the height in points of what the dialog draws under the list.
fn starter_list(ui: &mut egui::Ui, top: f32, below: f32, add: impl FnOnce(&mut egui::Ui)) {
    // The list measures what is above it from the top of the heading's body. What is above the body counts as below.
    let above_body = ui.max_rect().top() - top;
    dialog_list(ui, below + above_body.max(0.0), add);
}

/// Draws what Sheet holds: "Start from" with Blank and the sheet starters in a list, the world question under Blank
/// while it is chosen, and the Name field. `top` is the top of the dialog's content.
fn sheet_starters(ui: &mut egui::Ui, dialog: &mut NewModelDialog, top: f32) {
    caption(ui, START_FROM);
    ui.add_space(4.0);
    // The Name row, Advanced and the buttons, the first and the last after 12 points of space.
    let below = 2.0 * 12.0 + 3.0 * row_height(ui) + 8.0;
    let mut chosen = dialog.chosen;
    starter_list(ui, top, below, |ui| {
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
    ui.add_space(4.0);
}

/// Draws what Advanced holds, the headings Grid rule and Node graph, and returns whether either was pressed. `top` is
/// the top of the dialog's content.
fn advanced_headings(ui: &mut egui::Ui, dialog: &mut NewModelDialog, top: f32) -> bool {
    let grid_rule = heading(ui, dialog, NewModelHeading::GridRule, |ui, _| {
        grid_rule_starter(ui);
        false
    });
    let node_graph = heading(ui, dialog, NewModelHeading::NodeGraph, |ui, dialog| {
        graph_starters(ui, dialog, top);
        false
    });
    grid_rule || node_graph
}

/// Draws what Grid rule holds: "Start from" with the grid rule starter, chosen.
fn grid_rule_starter(ui: &mut egui::Ui) {
    caption(ui, START_FROM);
    ui.add_space(4.0);
    let (name, description) = GRID_RULE_STARTER;
    choice_row(ui, name, Some(description), true);
    ui.add_space(4.0);
}

/// Draws what Node graph holds: "Start from" with the graph starters in a list, each with its description. `top` is
/// the top of the dialog's content.
fn graph_starters(ui: &mut egui::Ui, dialog: &mut NewModelDialog, top: f32) {
    caption(ui, START_FROM);
    ui.add_space(4.0);
    // The buttons, after 12 points of space.
    let below = 12.0 + row_height(ui) + 8.0;
    let mut chosen = dialog.graph_chosen;
    starter_list(ui, top, below, |ui| {
        for (index, choice) in dialog.graphs.iter().enumerate() {
            if choice_row(
                ui,
                &choice.name,
                choice.description.as_deref(),
                dialog.graph_chosen == index,
            )
            .clicked()
            {
                chosen = index;
            }
        }
    });
    dialog.graph_chosen = chosen;
    ui.add_space(4.0);
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
/// with a Position that starts anywhere at random and a Heading, coloured by their heading.
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
            thing: kind.clone(),
            name: HEADING_KIND.to_owned(),
            property_type: PropertyType::Heading,
        })?;
        builder.apply(SheetEdit::SetColorRule {
            thing: kind,
            rule: Some(ColorRule::new(ColorRuleKind::ByHeading)),
        })
    })();
    // The commands run on a fresh kind of agent, which takes all four.
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
