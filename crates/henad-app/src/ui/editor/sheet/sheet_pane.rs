//! The Sheet pane: what a sheet model holds, one section per heading of
//! [`SheetSection`](henad_graph::sheet::sentence::SheetSection), each heading with its "?" help.
//!
//! Purpose, World (with the length of one tick), Cells (each property with its kind, its start and the lines that
//! change it), Parameters, Measures, Colours and Buttons. A world of agents shows Agents, Ground and Events in place of
//! Cells. Every section is edited here: the purpose, the size and the edges of the grid, the length of one tick, each
//! property of a kind of thing with its kind, its states and how it starts, each parameter's name, value, range and
//! unit, the measures plotted, the property that colours each kind of thing, and the buttons beside the world. The
//! sections of a world of agents are drawn by [`agents`].
//!
//! Every edit runs commands through [`SheetEditor::edit`]. A number field commits on Enter or on losing focus, and
//! text it cannot read stays in the field with a red message under its row. A property is added or changed through
//! the *property form* under the table of its kind of thing, a measure through the *measure form* and a button through
//! the *button form*. A Starts as number is a plain field with "Use a parameter" beside it, and a chosen parameter
//! shows as its chip. A parameter's slider commits once, when it is let go.

use std::hash::Hash;

use egui::{
    Align, Button, CornerRadius, Id, Key, Label, Layout, Rect, Response, RichText, Sense, Slider, TextEdit, Ui, vec2,
};
use henad_core::metadata::TimeUnit;
use henad_graph::limits::MAX_GRID_SIDE;
use henad_graph::sheet::check::SheetReport;
use henad_graph::sheet::document::{
    ButtonStep, ButtonStepRule, CardRuns, ColorRule, ColorRuleKind, Edges, LineKind, MadeOf, MeasureRule, PropertyType,
    ShadeHue, SheetButton, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetMeasure, SheetParameter,
    SheetProperty, SheetState, SheetTime, SheetValue, Slot, StartRule, StartsAs, StartsAsRow,
};
use henad_graph::sheet::edit::{
    IdSpace, STATE_COLORS, SheetBuilder, SheetEdit, SheetEditError, SheetItem, is_reserved_name, suggested_state_color,
};
use henad_graph::sheet::format_number;
use henad_graph::sheet::rates::chance_line_numbers;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    A_RANDOM_NUMBER, A_VALUE, ADD_BUTTON, ADD_MEASURE, ADD_PARAMETER, ADD_PROPERTY, ADD_ROW, ADD_STATE,
    AGENTS_IN_OPEN_SPACE, AVERAGE_MEASURE, AVERAGE_MEASURE_LEAD, BUTTON_CARD_EMPTY, BUTTON_DOES, BUTTON_NAME_FIELD,
    BUTTON_NAME_MISSING, BUTTON_NAME_PLACEHOLDER, BY_CHANCE, Blank, CANCEL, CELLS_COLUMNS, CELLS_ON_A_GRID,
    CHOOSE_A_PARAMETER, COLOR_CELLS_BY, COUNT_MEASURE, COUNT_MEASURE_IS, COUNT_MEASURE_LEAD, ChipKind, DARKEST_NAME,
    DELETE_PROPERTY, DONE, EDGES_LABEL, EDIT, EVERY_PROPERTY, HEIGHT_NAME, INTEGER_KIND, INTEGER_KIND_TOOLTIP,
    KIND_LABEL, LIGHTEST_NAME, MAX_LABEL, MEASURE_ALREADY_PLOTTED, MEASURE_NEEDS_NUMBER_PROPERTY, MEASURE_NEEDS_STATE,
    MEASURE_NUMBER_PROPERTY_EMPTY, MEASURE_PROPERTY_EMPTY, MEASURE_PROPERTY_NAME, MEASURE_STATE_EMPTY,
    MEASURE_STATE_NAME, MIN_LABEL, NAME_LABEL, NEW_BUTTON, NEW_MEASURE, NEW_PROPERTY, NUMBER_KIND,
    NUMBER_KIND_DESCRIPTION, ONE_TICK_IS, OTHERWISE, PARAMETER_COLUMNS, PARAMETER_NAME_MISSING, PARAMETERS, PLOT,
    PLOTTED, PROPERTY_NAME_FIELD, PROPERTY_NAME_MISSING, PROPERTY_NAME_PLACEHOLDER, PURPOSE_PLACEHOLDER,
    RANDOM_NUMBER_AND, RANDOM_NUMBER_BETWEEN, REMOVE_ROW_LABEL, SHADE, SHADE_COLORS, SHADE_DARK_TO, SHADE_ENDS_EQUAL,
    SHADE_FROM, SHADE_LIGHT, SHEET_EMPTY, SIZE_BY, SIZE_LABEL, START_AGAIN, START_AGAIN_FOR, START_AGAIN_TO_SEE,
    STATE_NAMES_REPEATED, STATES_KIND, STATES_KIND_DESCRIPTION, SentenceChip, Sentences, SheetSection, StartsField,
    TICK_LENGTH_NAME, TICK_UNIT_NAME, TOO_FEW_STATES, TYPE_A_CHANCE, TYPE_ONE_NUMBER, UNIT_FIELD_PLACEHOLDER,
    UNIT_LABEL, USE_A_PARAMETER, WALLS, WALLS_SENTENCE, WIDTH_NAME, WITH_CHANCE, WRAP_AROUND, WRAP_SENTENCE,
    WRAP_TOOLTIP, add_row_name, average_measure, button_name_taken, chances_per_tick_note, changed_by, count_measure,
    delete_button_name, delete_parameter_name, edit_button_name, edit_property_heading, literal_words,
    measure_color_name, parameter_max_name, parameter_min_name, parameter_name_taken, parameter_unit_name, plot_state,
    property_name_taken, read_typed_number, remove_measure_name, remove_row_name, remove_state_name, run_once,
    sheet_parameter_name_field, sheet_trying_value, start_again_for, starts_all, starts_field_name, starts_high_place,
    starts_low_place, starts_otherwise_name, starts_place, starts_row_place, state_color_name, state_name_field,
    tick_unit_choice, unit_suggestions, value_field_name, value_slider_name, world_side_out_of_range,
};
use henad_graph::sheet::sentence::{AGENT_WALLS_SENTENCE, AGENT_WRAP_TOOLTIP, name_reserved};
use henad_graph::sheet::sentence::{
    ANYWHERE_AT_RANDOM, ARROW_KIND, ARROW_KIND_DESCRIPTION, BY_HEADING_NOTE, COLOR_AGENTS_BY, COLOR_GROUND_BY,
    COUNT_AGENTS_LEAD, COUNT_AGENTS_MEASURE, HEADING_KIND, HEADING_KIND_DESCRIPTION, HEADING_START, NO, PAINTED,
    POSITION_KIND, POSITION_KIND_DESCRIPTION, YES, YES_NO_KIND, YES_NO_KIND_DESCRIPTION, count_agents_measure,
    every_starts_at, on_ground_start, painted_shapes, set_everywhere,
};
use henad_graph::value::Color;

use super::SheetEditor;
use super::card_pane::card_lines;
use super::help::Help;
use super::widgets::{
    CHIP_HEIGHT, DialogOutcome, NewParameterDialog, NumberUse, OverlayItem, Picker, PickerChoice, PickerGroup,
    PickerItem, REGION_GAP, caption, chip_within, color_button, color_name, color32, commit_area, commit_field,
    commit_number, field_message, forget_field, hairline, message_ui, new_parameter_modal, notice_ui, refusal_text,
    region, select_list, select_within, set_name, small_label, suggestion_field, swatch_ui, text_control,
    text_control_width,
};
use crate::init::semibold_font;
use crate::ui::sweep::layout::{add_button, destructive_button, filled_button};
use layout::{Column, Form, ITEM_GAP, Table, line, section, text_cell, text_width, wrapped_line};

mod agents;
mod layout;

/// A value the world tries for a parameter of the sheet while the model keeps its own.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetTrial {
    /// Id of the parameter.
    pub parameter: String,
    /// Value the world tries.
    pub value: f64,
}

/// Kind of thing whose properties the Cells section lists.
const CELLS: &str = "cells";

/// Kind of thing whose properties the Ground section lists.
const GROUND: &str = "ground";

/// Width in points of a number field.
const FIELD_WIDTH: f32 = 56.0;

/// Width in points of a state's name field in the property form.
const STATE_FIELD_WIDTH: f32 = 140.0;

/// Side in points of the gradient of a shade.
const SWATCH_SIDE: f32 = 10.0;

/// Width in points of a parameter's name field.
const PARAMETER_NAME_WIDTH: f32 = 120.0;

/// Width in points of a parameter's slider while its table has room.
const SLIDER_WIDTH: f32 = 100.0;

/// Width in points of a parameter's unit field while its table has room.
const UNIT_WIDTH: f32 = 80.0;

/// Width in points of a table's last column, the column of a row's delete button.
const DELETE_WIDTH: f32 = 18.0;

/// Width in points an inline select takes besides its words: the padding, the gap and the chevron.
const SELECT_FRAME: f32 = 22.0;

/// Width in points of the swatch of a shade, from its dark end to its light end.
const GRADIENT_WIDTH: f32 = 60.0;

/// Number of bands the swatch of a shade is drawn in.
const GRADIENT_BANDS: usize = 16;

/// State of the Sheet pane between frames: the id its widgets are keyed under, the forms, the New parameter dialog
/// and the reasons the last edits were refused.
#[derive(Debug)]
pub struct PaneState {
    /// Id the pane's widgets are keyed under, one for each sheet opened. A field's refused text then stays with the
    /// sheet it was typed in.
    widgets: Id,
    /// Property form under the table of its kind of thing, `None` while it is closed.
    property_form: Option<PropertyForm>,
    /// Measure form under the Plotted list, `None` while it is closed.
    measure_form: Option<MeasureForm>,
    /// Button form, `None` while it is closed.
    button_form: Option<ButtonForm>,
    /// Event form, `None` while it is closed.
    event_form: Option<agents::EventForm>,
    /// New parameter dialog with the Starts as number it fills, or `None` for a parameter that "+ Add parameter"
    /// adds. The whole is `None` while the dialog is closed.
    new_parameter: Option<(Option<NumberTarget>, NewParameterDialog)>,
    /// Reason the last edit outside the forms was refused, `None` while none shows.
    refusal: Option<String>,
    /// Id of the parameter whose new name was refused, with the message under it, `None` while none shows.
    parameter_message: Option<(String, String)>,
    /// Whether the last edit of the pane changed the length or the unit of one tick. The notice under the Time line
    /// then names the lines that use chances per tick, read from the sheet as it is drawn.
    time_changed: bool,
}

impl PaneState {
    /// Returns the state of a new pane, its widgets keyed by `identity`, the number of the load that opened the sheet.
    pub fn new(identity: u64) -> Self {
        Self {
            widgets: Id::new(("henad_sheet_pane", identity)),
            property_form: None,
            measure_form: None,
            button_form: None,
            event_form: None,
            new_parameter: None,
            refusal: None,
            parameter_message: None,
            time_changed: false,
        }
    }

    /// Closes every form and the New parameter dialog, and drops every message, as an undo or a redo does.
    pub fn close_forms(&mut self) {
        self.property_form = None;
        self.measure_form = None;
        self.button_form = None;
        self.event_form = None;
        self.new_parameter = None;
        self.refusal = None;
        self.parameter_message = None;
        self.time_changed = false;
    }

    /// Returns whether the New parameter dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.new_parameter.is_some()
    }
}

/// The form that adds a measure.
#[derive(Debug, Clone, Default)]
struct MeasureForm {
    /// Whether the measure averages a number property. Otherwise it counts the cells in a state.
    average: bool,
    /// Id of the property chosen, `None` before one is chosen.
    property: Option<String>,
    /// Id of the state chosen for a count, `None` before one is chosen.
    state: Option<String>,
    /// Message of the last refused Done, `None` while none shows.
    message: Option<String>,
}

impl MeasureForm {
    /// Returns the form for a measure of `document`: a count of the first States property, or an average when the
    /// kind of thing measured has none.
    fn add(document: &SheetDocument) -> Self {
        let properties = properties_of(document, measured_thing(document));
        let first = properties
            .iter()
            .find(|property| property.property_type == PropertyType::States);
        Self {
            average: first.is_none() && !properties.is_empty(),
            property: first.map(|property| property.id.clone()),
            ..Self::default()
        }
    }
}

/// The form that adds a button or changes one.
#[derive(Debug, Clone)]
struct ButtonForm {
    /// Id of the button the form changes, `None` for a new button.
    editing: Option<String>,
    /// Name of the button when the form opened.
    original: String,
    /// Text of the Name field.
    name: String,
    /// Whether the button runs its own lines once (`Some(true)`) or starts properties again (`Some(false)`). `None`
    /// for a button whose steps only set properties everywhere, until a kind is chosen.
    run_once: Option<bool>,
    /// Id of the property a Start again button starts again, `None` for every property.
    property: Option<String>,
    /// Message of the last refused Done, `None` while none shows.
    message: Option<String>,
    /// Whether the Name field takes the focus on the next frame.
    focus_name: bool,
}

impl ButtonForm {
    /// Returns the form that adds a button that starts every property again.
    fn add() -> Self {
        Self {
            editing: None,
            original: String::new(),
            name: String::new(),
            run_once: Some(false),
            property: None,
            message: None,
            focus_name: true,
        }
    }

    /// Returns the form that changes `button`.
    fn edit(button: &SheetButton) -> Self {
        let has = |rule| button.does.iter().any(|step| step.rule == rule);
        let run_once = if has(ButtonStepRule::RunOnce) {
            Some(true)
        } else {
            has(ButtonStepRule::StartAgain).then_some(false)
        };
        let property = button
            .does
            .iter()
            .find(|step| step.rule == ButtonStepRule::StartAgain)
            .and_then(|step| step.properties.first().cloned());
        Self {
            editing: Some(button.id.clone()),
            original: button.name.clone(),
            name: button.name.clone(),
            run_once,
            property,
            message: None,
            focus_name: true,
        }
    }
}

/// The form that adds a property to a kind of thing or changes one.
#[derive(Debug, Clone)]
struct PropertyForm {
    /// Kind of thing the property belongs to: `cells`, `ground` or an agent kind's id.
    thing: String,
    /// Id of the property the form changes, `None` for a new property.
    editing: Option<String>,
    /// Name of the property when the form opened.
    original: String,
    /// Text of the Name field.
    name: String,
    /// Kind chosen.
    kind: PropertyType,
    /// Rows of states, for a States property.
    states: Vec<FormState>,
    /// Message of the last refused Done, `None` while none shows.
    message: Option<String>,
    /// Row of states whose name field takes the focus on the next frame.
    focus_state: Option<usize>,
    /// Whether the Name field takes the focus on the next frame.
    focus_name: bool,
}

/// One row of states in the property form.
#[derive(Debug, Clone)]
struct FormState {
    /// Id of the state, `None` for a state the form adds.
    id: Option<String>,
    /// Text of the state's name field.
    name: String,
    /// Colour of the state.
    color: Color,
    /// Whether the colour was chosen by hand. Until then a usual name gives its usual colour.
    picked: bool,
}

impl FormState {
    /// Returns an empty row at `index`, in the colour new states take there.
    fn empty(index: usize) -> Self {
        Self {
            id: None,
            name: String::new(),
            color: default_color(index),
            picked: false,
        }
    }
}

impl PropertyForm {
    /// Returns the form that adds a property to `thing`: a States property with two empty states.
    fn add(thing: &str) -> Self {
        Self {
            thing: thing.to_owned(),
            editing: None,
            original: String::new(),
            name: String::new(),
            kind: PropertyType::States,
            states: vec![FormState::empty(0), FormState::empty(1)],
            message: None,
            focus_state: None,
            focus_name: true,
        }
    }

    /// Returns the form that changes `property` of `thing`. Its states count as coloured by hand, and a new name keeps
    /// their colours.
    fn edit(property: &SheetProperty, thing: &str) -> Self {
        let states = if property.property_type == PropertyType::States {
            property
                .states
                .iter()
                .map(|state| FormState {
                    id: Some(state.id.clone()),
                    name: state.name.clone(),
                    color: state.color,
                    picked: true,
                })
                .collect()
        } else {
            vec![FormState::empty(0), FormState::empty(1)]
        };
        Self {
            thing: thing.to_owned(),
            editing: Some(property.id.clone()),
            original: property.name.clone(),
            name: property.name.clone(),
            kind: property.property_type,
            states,
            message: None,
            focus_state: None,
            focus_name: true,
        }
    }
}

/// Place of a number in a Starts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StartSlot {
    /// The chance of a row of a by-chance start.
    Row(usize),
    /// The number every cell starts at.
    Value,
    /// The low end of a random number.
    Low,
    /// The high end of a random number.
    High,
}

/// A number of the sheet that a field, its "Use a parameter" and the New parameter dialog fill.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NumberTarget {
    /// A number of a property's Starts as.
    Start {
        /// Id of the property.
        property: String,
        /// Place of the number.
        slot: StartSlot,
    },
    /// The share of a Number property of the ground left after each tick.
    Kept {
        /// Id of the property.
        property: String,
    },
}

impl NumberTarget {
    /// Returns the edit that puts `value` at the target.
    ///
    /// A start's number is a changed start. The share kept changes the run, and keeps the property's marks rule.
    fn edit(&self, value: Slot) -> Pending {
        match self.clone() {
            Self::Start { property, slot } => start_edit(property, move |start| set_start_slot(start, slot, value)),
            Self::Kept { property } => Pending {
                starts_only: false,
                form: None,
                run: Box::new(move |builder| {
                    let marks = all_properties(builder.document())
                        .find(|known| known.id == property)
                        .and_then(|known| known.marks);
                    builder
                        .apply(SheetEdit::SetMarks {
                            property,
                            marks,
                            kept_per_tick: value,
                        })
                        .map(drop)
                }),
            },
        }
    }
}

/// A number field of the sheet with "Use a parameter" beside it, or the chip of the parameter it holds.
struct NumberField<'a> {
    /// Place of the number in an accessible name, such as "Status starts as, row 1".
    place: String,
    /// Id of the field.
    id: Id,
    /// Value the sheet holds there, `None` where the sheet holds nothing.
    current: Option<&'a Slot>,
    /// Target the number fills.
    target: NumberTarget,
    /// Whether the number is a chance or any other number.
    number_use: NumberUse,
    /// Range a typed number must fall in, with the message for a number outside it, `None` for any number. A field
    /// with a range cannot be left empty either.
    limit: Option<(f64, f64, &'static str)>,
    /// Width in points to keep free after the chip of a parameter on its line, for the control after it.
    after: f32,
}

/// One edit of the sheet: commands run on a copy of it, kept when every command is accepted.
type EditRun = Box<dyn FnOnce(&mut SheetBuilder) -> Result<(), SheetEditError>>;

/// Form of the Sheet pane an edit comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormKind {
    /// The property form.
    Property,
    /// The measure form.
    Measure,
    /// The button form.
    Button,
    /// The event form.
    Event,
}

/// An edit made during the frame, run once the pane is drawn.
struct Pending {
    /// Whether the edit leaves the running world alone apart from how properties start, as a new start, a new name
    /// or a new purpose does.
    starts_only: bool,
    /// Form the edit comes from, `None` for a field or a button of a section. The form closes once the edit is kept,
    /// and shows a refusal as its message.
    form: Option<FormKind>,
    /// The commands.
    run: EditRun,
}

impl Pending {
    /// Returns the edit that runs the one command `edit`.
    fn one(edit: SheetEdit) -> Self {
        Self {
            starts_only: matches!(
                edit,
                SheetEdit::SetStartsAs { .. } | SheetEdit::SetPurpose { .. } | SheetEdit::SetSheetName { .. }
            ),
            form: None,
            run: Box::new(move |builder| builder.apply(edit).map(drop)),
        }
    }

    /// Returns the edit that runs `run`, made in the form `form`.
    fn from_form(form: FormKind, run: EditRun) -> Self {
        Self {
            starts_only: false,
            form: Some(form),
            run,
        }
    }
}

/// Draws every section of `sheet`, in the order of [`SheetSection::shown`] for its world, each heading with its help,
/// and runs the edits made in it.
///
/// `past_start` is whether the preview has passed tick 0 or a button was pressed in it. A changed Starts as then says
/// "Start again to see it". `trials` holds the values the world tries, each shown under its parameter.
pub fn sheet_pane(ui: &mut Ui, sheet: &mut SheetEditor, past_start: bool, trials: &[SheetTrial]) {
    let stale: Vec<String> = all_properties(sheet.document())
        .filter(|property| past_start && sheet.start_changed(&property.id))
        .map(|property| property.id.clone())
        .collect();
    let mut pending = None;
    {
        let SheetEditor {
            builder, pane, report, ..
        } = &mut *sheet;
        let document = builder.document();
        ui.push_id(pane.widgets, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let agents = document.world.made_of == MadeOf::Agents;
            for (index, &shown) in SheetSection::shown(document.world.made_of).iter().enumerate() {
                ui.add_space(if index == 0 { 4.0 } else { REGION_GAP });
                let help = match shown {
                    SheetSection::World if agents => Help::AgentWorld,
                    _ => Help::Section(shown),
                };
                section(ui, shown.heading(), help, |ui| match shown {
                    SheetSection::Purpose => purpose(ui, document, pane.widgets, &mut pending),
                    SheetSection::World => world(ui, document, pane, &mut pending),
                    SheetSection::Cells => cells(ui, document, pane, &stale, &mut pending),
                    SheetSection::Agents => agents::agents(ui, document, pane, &stale, &mut pending),
                    SheetSection::Ground => agents::ground(ui, document, pane, &stale, &mut pending),
                    SheetSection::Events => agents::events(ui, document, pane, &mut pending),
                    SheetSection::Parameters => parameters(ui, document, pane, trials, &mut pending),
                    SheetSection::Measures => measures(ui, document, pane, &mut pending),
                    SheetSection::Colours => colors(ui, document, pane.widgets, &mut pending),
                    SheetSection::Buttons => buttons(ui, document, report, pane, &mut pending),
                    _ => {}
                });
            }
            ui.add_space(REGION_GAP);
            new_parameter(ui, document, pane, &mut pending);
        });
    }
    let Some(Pending { starts_only, form, run }) = pending else {
        return;
    };
    let time = sheet.document().time;
    let outcome = sheet.edit(past_start, starts_only, run);
    let time_changed = sheet.document().time.is_some() && sheet.document().time != time;
    if time_changed {
        // The notice of a time a rate's unit set no longer states the length of one tick.
        sheet.card.drop_tick_notice();
    }
    let pane = &mut sheet.pane;
    match outcome {
        Ok(()) => {
            pane.refusal = None;
            pane.time_changed = time_changed;
            match form {
                Some(FormKind::Property) => pane.property_form = None,
                Some(FormKind::Measure) => pane.measure_form = None,
                Some(FormKind::Button) => pane.button_form = None,
                Some(FormKind::Event) => pane.event_form = None,
                None => {}
            }
        }
        Err(error) => {
            let message = Some(refusal_text(&error));
            match form {
                Some(FormKind::Property) if let Some(open) = &mut pane.property_form => open.message = message,
                Some(FormKind::Measure) if let Some(open) = &mut pane.measure_form => open.message = message,
                Some(FormKind::Button) if let Some(open) = &mut pane.button_form => open.message = message,
                Some(FormKind::Event) if let Some(open) = &mut pane.event_form => open.message = message,
                _ => pane.refusal = message,
            }
        }
    }
}

/// Draws what the model is for as a field of several lines that commits on losing focus. `widgets` is the id the
/// pane's widgets are keyed under.
fn purpose(ui: &mut Ui, document: &SheetDocument, widgets: Id, pending: &mut Option<Pending>) {
    let current = document.sheet.purpose.as_deref().unwrap_or_default();
    commit_area(
        ui,
        widgets.with("henad_sheet_purpose"),
        SheetSection::Purpose.heading(),
        current,
        PURPOSE_PLACEHOLDER,
        |text| {
            let purpose = text.trim();
            *pending = Some(Pending::one(SheetEdit::SetPurpose {
                purpose: (!purpose.is_empty()).then(|| purpose.to_owned()),
            }));
            None
        },
    );
}

/// Draws the world's kind, then its size, its edges and the length of one tick as form rows, and the notice of `pane`
/// under them.
fn world(ui: &mut Ui, document: &SheetDocument, pane: &PaneState, pending: &mut Option<Pending>) {
    let widgets = pane.widgets;
    let world = document.world;
    let form = Form::new(ui, &[SIZE_LABEL, EDGES_LABEL, ONE_TICK_IS]);
    text_cell(
        ui,
        Label::new(RichText::new(match world.made_of {
            MadeOf::Agents => AGENTS_IN_OPEN_SPACE,
            _ => CELLS_ON_A_GRID,
        })),
    );
    let width_id = widgets.with("henad_sheet_world_width");
    let height_id = widgets.with("henad_sheet_world_height");
    form.row(ui, SIZE_LABEL, |ui| {
        side_field(ui, width_id, WIDTH_NAME, world.width, pending, |width| {
            SheetEdit::SetWorld {
                width,
                height: world.height,
                edges: world.edges,
            }
        });
        ui.label(SIZE_BY);
        side_field(ui, height_id, HEIGHT_NAME, world.height, pending, |height| {
            SheetEdit::SetWorld {
                width: world.width,
                height,
                edges: world.edges,
            }
        });
    });
    if let Some(message) = field_message(ui, width_id).or_else(|| field_message(ui, height_id)) {
        form.under(ui, |ui| message_ui(ui, &message));
    }
    let agents = world.made_of == MadeOf::Agents;
    let (words, sentence, tooltip) = match world.edges {
        Edges::Walls if agents => (WALLS, AGENT_WALLS_SENTENCE, AGENT_WALLS_SENTENCE),
        Edges::Walls => (WALLS, WALLS_SENTENCE, WALLS_SENTENCE),
        _ if agents => (WRAP_AROUND, WRAP_SENTENCE, AGENT_WRAP_TOOLTIP),
        _ => (WRAP_AROUND, WRAP_SENTENCE, WRAP_TOOLTIP),
    };
    form.row(ui, EDGES_LABEL, |ui| {
        let options = [(Edges::Wrap, WRAP_AROUND), (Edges::Walls, WALLS)];
        let items = options.map(|(edges, label)| OverlayItem {
            label,
            current: edges == world.edges,
            ..OverlayItem::default()
        });
        let (response, chosen) = select(ui, "henad_sheet_edges", EDGES_LABEL, words, &items);
        response.on_hover_text(tooltip);
        if let Some((edges, _)) = chosen.and_then(|index| options.get(index))
            && *edges != world.edges
        {
            *pending = Some(Pending::one(SheetEdit::SetWorld {
                width: world.width,
                height: world.height,
                edges: *edges,
            }));
        }
    });
    form.under(ui, |ui| ui.weak(sentence));
    time_line(ui, document, &form, widgets, pending);
    // A chance is per tick, and a new length of one tick leaves the lines that draw with one behind the time scale.
    if pane.time_changed && document.time.is_some() {
        let lines = chance_line_numbers(document);
        if !lines.is_empty() {
            ui.add_space(4.0);
            notice_ui(ui, &chances_per_tick_note(&lines));
        }
    }
}

/// Adds the field of one side of the grid, named `name`, holding `side` cells. A whole number of cells from 1 to
/// [`MAX_GRID_SIDE`] commits the command `edit` returns for it.
fn side_field(
    ui: &mut Ui,
    id: Id,
    name: &str,
    side: u32,
    pending: &mut Option<Pending>,
    edit: impl FnOnce(u32) -> SheetEdit,
) {
    commit_number(ui, id, name, &side.to_string(), FIELD_WIDTH, |text| {
        let Some(number) = read_typed_number(text).and_then(SheetLiteral::number) else {
            return Some(TYPE_ONE_NUMBER.to_owned());
        };
        if number.fract() != 0.0 || !(1.0..=f64::from(MAX_GRID_SIDE)).contains(&number) {
            return Some(world_side_out_of_range(MAX_GRID_SIDE));
        }
        // The checks above hold the side to a whole number within `u32`.
        let side = number as u32;
        *pending = Some(Pending::one(edit(side)));
        None
    });
}

/// Draws the Time line of `form`, "One tick is [number] [unit]", with the number hidden while the tick has no unit.
fn time_line(ui: &mut Ui, document: &SheetDocument, form: &Form, widgets: Id, pending: &mut Option<Pending>) {
    let length_id = widgets.with("henad_sheet_tick_length");
    let time = document.time;
    form.row(ui, ONE_TICK_IS, |ui| {
        if let Some(time) = time {
            let current = format_number(time.tick_length).unwrap_or_default();
            commit_number(ui, length_id, TICK_LENGTH_NAME, &current, FIELD_WIDTH, |text| {
                let Some(length) = read_typed_number(text).and_then(SheetLiteral::number) else {
                    return Some(TYPE_ONE_NUMBER.to_owned());
                };
                // A length of 0 or less becomes 1, as the page has it.
                let length = if length > 0.0 { length } else { 1.0 };
                *pending = Some(Pending::one(SheetEdit::SetTime {
                    time: Some(SheetTime::new(length, time.tick_unit)),
                }));
                None
            });
        }
        let current = time.map(|time| time.tick_unit);
        let choices: Vec<Option<TimeUnit>> = std::iter::once(None)
            .chain(TimeUnit::ALL.iter().copied().map(Some))
            .collect();
        let items: Vec<OverlayItem<'_>> = choices
            .iter()
            .map(|choice| OverlayItem {
                label: tick_unit_choice(*choice),
                current: *choice == current,
                ..OverlayItem::default()
            })
            .collect();
        let shown = tick_unit_choice(current);
        let (_, chosen) = select(ui, "henad_sheet_tick_unit", TICK_UNIT_NAME, shown, &items);
        if let Some(&unit) = chosen.and_then(|index| choices.get(index))
            && unit != current
        {
            let length = time.map_or(1.0, |time| time.tick_length);
            *pending = Some(Pending::one(SheetEdit::SetTime {
                time: unit.map(|unit| SheetTime::new(length, unit)),
            }));
        }
    });
    if time.is_some()
        && let Some(message) = field_message(ui, length_id)
    {
        form.under(ui, |ui| message_ui(ui, &message));
    }
}

/// Adds an inline select keyed by `salt`, named `name` and showing `words`, with its options `items` in a list under
/// it, and returns its response and the index of the option chosen.
///
/// The select is at most as wide as the line it sits on. Longer words end in an ellipsis, whole in its tooltip.
fn select(
    ui: &mut Ui,
    salt: impl Hash + std::fmt::Debug,
    name: &str,
    words: &str,
    items: &[OverlayItem<'_>],
) -> (Response, Option<usize>) {
    let id = ui.make_persistent_id(salt);
    let width = ui.max_rect().width();
    let response = select_within(ui, Some(id), words, name, words.is_empty(), width);
    let chosen = select_list(ui, &response, id.with("picker"), items);
    (response, chosen)
}

/// Returns the width in points of an inline select showing `words`.
fn select_width(ui: &Ui, words: &str) -> f32 {
    text_width(ui, words) + SELECT_FRAME
}

/// Draws the Cells table, with each property's start as controls, and the property form or "+ Add property" under
/// it. A sheet with no property says how to start.
fn cells(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, stale: &[String], pending: &mut Option<Pending>) {
    let properties = properties_of(document, CELLS);
    if properties.is_empty() {
        text_cell(ui, Label::new(SHEET_EMPTY));
    } else {
        property_table(ui, document, CELLS, pane, stale, pending);
    }
    ui.add_space(6.0);
    property_form_or_add(ui, document, CELLS, ADD_PROPERTY, properties.is_empty(), pane, pending);
    if let Some(refusal) = &pane.refusal {
        message_ui(ui, refusal);
    }
}

/// Draws the table of the properties of `thing`: each property's name, which opens the property form, its kind, its
/// start as controls, and the lines that change it. `stale` holds the properties whose start waits for Start again.
///
/// Starts as takes the room left over. Short of room, Starts as gives up width down to 150 points, then Changed by,
/// Kind and Name down to their longest words, wrapping at them, then Starts as down to the width of "Use a parameter",
/// its selects and chips ending in an ellipsis, and last Kind and Name down to their least widths, a word too long for
/// them ending in an ellipsis.
fn property_table(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    pane: &mut PaneState,
    stale: &[String],
    pending: &mut Option<Pending>,
) {
    let properties = properties_of(document, thing);
    let widest = |width: &dyn Fn(&SheetProperty) -> f32| properties.iter().map(width).fold(0.0, f32::max);
    let changed = |property: &SheetProperty| changed_by(&changing_lines(document, &property.id));
    let names = widest(&|property| text_width(ui, &property.name));
    let name_words = widest(&|property| longest_word(ui, &property.name));
    let kinds = widest(&|property| kind_width(ui, property, text_width));
    let kind_words = widest(&|property| kind_width(ui, property, longest_word));
    let starts = widest(&|property| starts_width(ui, document, thing, property));
    let changes = widest(&|property| text_width(ui, &changed(property)));
    let change_words = widest(&|property| longest_word(ui, &changed(property)));
    // "Use a parameter" is the widest control of Starts as that neither wraps nor ends in an ellipsis.
    let starts_least = text_control_width(ui, USE_A_PARAMETER);
    let header_word = |index: usize| {
        CELLS_COLUMNS[index]
            .split_whitespace()
            .map(|word| caption_width(ui, word))
            .fold(0.0, f32::max)
    };
    let columns = vec![
        Column::new(
            CELLS_COLUMNS[0],
            names.max(caption_width(ui, CELLS_COLUMNS[0])),
            header_word(0).max(40.0),
        ),
        Column::new(
            CELLS_COLUMNS[1],
            kinds.max(caption_width(ui, CELLS_COLUMNS[1])),
            header_word(1).max(56.0),
        ),
        Column::new(CELLS_COLUMNS[2], starts, starts_least),
        Column::new(
            CELLS_COLUMNS[3],
            changes.max(caption_width(ui, CELLS_COLUMNS[3])),
            change_words.max(header_word(3)),
        ),
    ];
    let id = ui.make_persistent_id(("henad_sheet_properties", thing));
    let give_up = [
        (2, 150.0),
        (3, 0.0),
        (1, kind_words),
        (0, name_words),
        (2, starts_least),
        (1, 0.0),
        (0, 0.0),
    ];
    Table::new(id, columns, 2, &give_up).show(ui, |ui, rows| {
        for property in properties {
            rows.row(ui, &property.id, |row| {
                row.cell(0, |ui| {
                    line(ui, |ui| {
                        if property_name_ui(ui, property).clicked() {
                            pane.property_form = Some(PropertyForm::edit(property, thing));
                        }
                    });
                });
                row.cell(1, |ui| kind_ui(ui, property));
                row.cell(2, |ui| {
                    starts_ui(ui, document, thing, property, pane, pending);
                    if stale.contains(&property.id) {
                        ui.weak(START_AGAIN_TO_SEE);
                    }
                });
                row.cell(3, |ui| {
                    text_cell(ui, Label::new(changed(property)));
                });
            });
        }
    });
}

/// Returns the width in points of the longest word of `text` in the body font.
fn longest_word(ui: &Ui, text: &str) -> f32 {
    text.split_whitespace()
        .map(|word| text_width(ui, word))
        .fold(0.0, f32::max)
}

/// Adds the name of `property` as a frameless button named `edit_property_heading`. A click opens the property form.
/// A name too long for its column wraps, and a word too long for it ends in an ellipsis, whole in the tooltip.
fn property_name_ui(ui: &mut Ui, property: &SheetProperty) -> Response {
    // With no padding, the name starts on the column's edge, under its header.
    ui.spacing_mut().button_padding.x = 0.0;
    let elided = longest_word(ui, &property.name) > ui.available_width();
    let button = Button::new(&property.name).frame(false);
    let button = if elided { button.truncate() } else { button.wrap() };
    let response = add_button(ui, true, button, &edit_property_heading(&property.name));
    if elided {
        response.on_hover_text(&property.name)
    } else {
        response
    }
}

/// Returns a label of `text` that wraps at the right edge of `ui`, or ends in an ellipsis, whole in its tooltip, where
/// a word of it is too long to wrap.
fn fitted_label(ui: &Ui, text: impl Into<egui::WidgetText>) -> Label {
    let text = text.into();
    if longest_word(ui, text.text()) > ui.available_width() {
        Label::new(text).truncate()
    } else {
        Label::new(text).wrap()
    }
}

/// Returns the width in points of `text` as the small upper-case label of a table header.
fn caption_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_uppercase(),
            egui::FontId::monospace(super::widgets::SMALL_LABEL),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
}

/// Returns the width in points the kind of `property` takes: its word, or the swatch and the name of its widest state,
/// each measured by `measure`, the width of a whole text or of its longest word.
fn kind_width(ui: &Ui, property: &SheetProperty, measure: fn(&Ui, &str) -> f32) -> f32 {
    let word = measure(ui, kind_words(property.property_type));
    property
        .states
        .iter()
        .map(|state| measure(ui, &state.name) + STATE_SWATCH_WIDTH)
        .fold(word, f32::max)
}

/// Width in points of the swatch before a state's name, with the gap after it.
const STATE_SWATCH_WIDTH: f32 = 13.0;

/// Returns the width in points the Starts as of `property`, a property of `thing`, takes while its first rows fit on
/// one line each.
fn starts_width(ui: &Ui, document: &SheetDocument, thing: &str, property: &SheetProperty) -> f32 {
    let selected = starts_text(document, property);
    let mut width = select_width(ui, &selected);
    let Some(start) = &property.starts_as else {
        return width;
    };
    let widest_state = property
        .states
        .iter()
        .map(|state| select_width(ui, &state.name))
        .fold(0.0, f32::max);
    let row = match start.rule {
        StartRule::ByChance => widest_state + text_width(ui, WITH_CHANCE) + FIELD_WIDTH + 2.0 * ITEM_GAP,
        StartRule::Value if !matches!(property.property_type, PropertyType::States | PropertyType::YesNo) => {
            let lead = if holds_random_number(&start.value) {
                RANDOM_NUMBER_BETWEEN.to_owned()
            } else {
                every_starts_at(&Sentences::new(document).thing_word(thing))
            };
            text_width(ui, &lead) + FIELD_WIDTH + ITEM_GAP
        }
        _ => 0.0,
    };
    width = width.max(row);
    width.min(STARTS_MOST)
}

/// Width in points the Starts as column takes at most while the table fits.
const STARTS_MOST: f32 = 280.0;

/// Draws the property form under the table of `thing` while it is open for that thing, or the button that opens it,
/// named `add_name`. The button is the section's primary action while `primary` holds, and dashed otherwise.
fn property_form_or_add(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    add_name: &str,
    primary: bool,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    if let Some(form) = pane.property_form.as_mut().filter(|form| form.thing == thing) {
        let mut close = false;
        property_form(ui, document, form, &mut close, pending);
        if close {
            pane.property_form = None;
        }
        return;
    }
    let opened = if primary {
        add_button(ui, true, filled_button(ADD_PROPERTY.to_owned()), add_name).clicked()
    } else {
        super::widgets::add_button(ui, ADD_PROPERTY, add_name, None, false).clicked()
    };
    if opened {
        pane.property_form = Some(PropertyForm::add(thing));
    }
}

/// Draws the kind of `property`: "States" with each state's swatch and name under it, "Number", "Integer", "Yes or
/// no", "Position", "Heading" or "Arrow". A state's name too long for the column wraps, or ends in an ellipsis where a
/// word of it does not fit.
fn kind_ui(ui: &mut Ui, property: &SheetProperty) {
    match property.property_type {
        PropertyType::States => {
            text_cell(ui, fitted_label(ui, STATES_KIND));
            ui.spacing_mut().item_spacing.y = 0.0;
            for state in &property.states {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = STATE_SWATCH_WIDTH - super::widgets::CHIP_SWATCH;
                    swatch_ui(ui, color32(state.color));
                    let row = ui.text_style_height(&egui::TextStyle::Body);
                    ui.vertical(|ui| {
                        ui.add_space(((CHIP_HEIGHT - row) / 2.0).max(0.0));
                        ui.add(fitted_label(ui, &state.name));
                    });
                });
            }
        }
        PropertyType::WholeNumber => {
            text_cell(ui, fitted_label(ui, INTEGER_KIND)).on_hover_text(INTEGER_KIND_TOOLTIP);
        }
        kind => {
            text_cell(ui, fitted_label(ui, kind_words(kind)));
        }
    }
}

/// Returns the word of a property's kind, such as "Number" or "Yes or no".
fn kind_words(kind: PropertyType) -> &'static str {
    match kind {
        PropertyType::States => STATES_KIND,
        PropertyType::WholeNumber => INTEGER_KIND,
        PropertyType::YesNo => YES_NO_KIND,
        PropertyType::Position => POSITION_KIND,
        PropertyType::Heading => HEADING_KIND,
        PropertyType::Arrow => ARROW_KIND,
        _ => NUMBER_KIND,
    }
}

/// Returns how `property` of `document` starts: "All Dead", "By chance", "A value", "A random number", "Anywhere at
/// random", "On a cell whose Ground is Nest", "Painted" or "All Yes".
///
/// A Heading, which takes its first value from the first move, returns [`HEADING_START`], and a property with no
/// start an empty string.
pub fn starts_text(document: &SheetDocument, property: &SheetProperty) -> String {
    let Some(starts) = &property.starts_as else {
        return if property.property_type == PropertyType::Heading {
            HEADING_START.to_owned()
        } else {
            String::new()
        };
    };
    match starts.rule {
        StartRule::ByChance => BY_CHANCE.to_owned(),
        StartRule::Anywhere => ANYWHERE_AT_RANDOM.to_owned(),
        StartRule::Painted => PAINTED.to_owned(),
        StartRule::OnGround => {
            let ground = starts.property.as_deref().and_then(|id| find_property(document, id));
            let state = ground
                .zip(starts.state.as_deref())
                .and_then(|(ground, state)| state_name(ground, state));
            match (ground, state) {
                (Some(ground), Some(state)) => on_ground_start(&ground.name, state),
                _ => String::new(),
            }
        }
        StartRule::Value => match starts.value.value() {
            Some(SheetValue::Chip(SheetChip::State(state))) => match state_name(property, state) {
                Some(name) => starts_all(name),
                None => A_VALUE.to_owned(),
            },
            Some(SheetValue::Literal(SheetLiteral::Bool(yes))) => starts_all(if *yes { YES } else { NO }),
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber => A_RANDOM_NUMBER.to_owned(),
            _ => A_VALUE.to_owned(),
        },
        _ => String::new(),
    }
}

/// Returns whether `slot` holds a random number between two numbers.
fn holds_random_number(slot: &Slot) -> bool {
    matches!(slot.value(), Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber)
}

/// Returns the name of state `state_id` of `property`.
fn state_name<'a>(property: &'a SheetProperty, state_id: &str) -> Option<&'a str> {
    property
        .states
        .iter()
        .find(|state| state.id == state_id)
        .map(|state| state.name.as_str())
}

/// A choice in the select of a Starts as.
enum StartChoice {
    /// Every element starts in the state of this id.
    All(String),
    /// Each element draws its state.
    ByChance,
    /// Every element starts at one number.
    Value,
    /// Every element draws a number between two numbers.
    RandomNumber,
    /// Every element starts at yes or at no.
    YesNo(bool),
    /// Every agent starts at a random place.
    Anywhere,
    /// Every agent starts on a cell whose ground property of the first id is the state of the second.
    OnGround(String, String),
    /// The painted start. Choosing it changes nothing.
    Painted,
}

/// Adds the select of the Starts as of `property`, showing `selected`, and returns the choice made in it.
fn start_select(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    selected: &str,
) -> Option<StartChoice> {
    let mut offered: Vec<(String, StartChoice)> = Vec::new();
    match property.property_type {
        PropertyType::States => {
            // No control paints, so a painted start shows as its own entry, and choosing it again changes nothing.
            if selected == PAINTED {
                offered.push((PAINTED.to_owned(), StartChoice::Painted));
            }
            for state in &property.states {
                offered.push((starts_all(&state.name), StartChoice::All(state.id.clone())));
            }
            offered.push((BY_CHANCE.to_owned(), StartChoice::ByChance));
        }
        PropertyType::YesNo => {
            offered.push((starts_all(YES), StartChoice::YesNo(true)));
            offered.push((starts_all(NO), StartChoice::YesNo(false)));
        }
        PropertyType::Position => {
            offered.push((ANYWHERE_AT_RANDOM.to_owned(), StartChoice::Anywhere));
            for ground in properties_of(document, GROUND)
                .iter()
                .filter(|ground| ground.property_type == PropertyType::States)
            {
                let painted = ground.starts_as.as_ref();
                for state in ground
                    .states
                    .iter()
                    .filter(|state| painted.is_some_and(|start| start.paints_single_cells(&state.id)))
                {
                    offered.push((
                        on_ground_start(&ground.name, &state.name),
                        StartChoice::OnGround(ground.id.clone(), state.id.clone()),
                    ));
                }
            }
        }
        kind => {
            offered.push((A_VALUE.to_owned(), StartChoice::Value));
            // A draw truncated to a whole number would never reach the high end.
            if kind == PropertyType::Number {
                offered.push((A_RANDOM_NUMBER.to_owned(), StartChoice::RandomNumber));
            }
        }
    }
    let items: Vec<OverlayItem<'_>> = offered
        .iter()
        .map(|(words, _)| OverlayItem {
            label: words,
            current: words == selected,
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = line(ui, |ui| {
        select(
            ui,
            ("henad_sheet_starts", &property.id),
            &starts_place(&property.name),
            selected,
            &items,
        )
    });
    let index = chosen?;
    offered.into_iter().nth(index).map(|(_, choice)| choice)
}

/// Draws the Starts as of `property`, a property of `thing`: its select and, under it, the rows of a by-chance start,
/// the numbers of a Number property or the shapes of a painted one.
///
/// A Heading shows that it starts with the first move, and an Arrow its start in words, since neither has a control
/// of its own.
fn starts_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    property: &SheetProperty,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let selected = starts_text(document, property);
    match property.property_type {
        PropertyType::Heading => {
            text_cell(ui, Label::new(RichText::new(&selected).weak()));
            return;
        }
        PropertyType::Arrow => {
            if let Some(start) = &property.starts_as {
                let words = Sentences::new(document).slot(&start.value, Some(&property.id));
                text_cell(ui, Label::new(RichText::new(words).weak()));
            }
            return;
        }
        _ => {}
    }
    if let Some(choice) = start_select(ui, document, property, &selected) {
        choose_start(property, &selected, choice, pending);
    }
    let Some(start) = &property.starts_as else {
        return;
    };
    let states = property.property_type == PropertyType::States;
    match start.rule {
        StartRule::ByChance => chance_rows(ui, document, property, start, pane, pending),
        StartRule::Painted => {
            let base = start
                .base
                .as_deref()
                .and_then(|base| state_name(property, base))
                .unwrap_or_default();
            ui.add(Label::new(RichText::new(painted_shapes(base, start.shapes.len())).weak()).wrap());
        }
        StartRule::Value if !states && property.property_type != PropertyType::YesNo => {
            let random = holds_random_number(&start.value);
            let slots: &[StartSlot] = if random {
                &[StartSlot::Low, StartSlot::High]
            } else {
                &[StartSlot::Value]
            };
            wrapped_line(ui, |ui| {
                if random {
                    ui.label(RANDOM_NUMBER_BETWEEN);
                    start_number(ui, document, property, start, StartSlot::Low, pane, pending);
                    ui.label(RANDOM_NUMBER_AND);
                    start_number(ui, document, property, start, StartSlot::High, pane, pending);
                } else {
                    ui.label(every_starts_at(&Sentences::new(document).thing_word(thing)));
                    start_number(ui, document, property, start, StartSlot::Value, pane, pending);
                }
            });
            start_messages(ui, pane.widgets, property, slots);
        }
        _ => {}
    }
}

/// Sets the start of `property` to `choice`, unless `selected`, the words of its start, already reads as it.
fn choose_start(property: &SheetProperty, selected: &str, choice: StartChoice, pending: &mut Option<Pending>) {
    let number = |value: f64| Slot::Filled(SheetValue::Literal(SheetLiteral::Number(value)));
    let start = match choice {
        StartChoice::All(state) => {
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Chip(SheetChip::State(state)));
            start
        }
        StartChoice::ByChance if selected != BY_CHANCE => {
            // A new by-chance start draws the second state, and every other cell takes the first.
            let mut start = StartsAs::new(StartRule::ByChance);
            let first = property.states.first().map(|state| state.id.clone());
            let drawn = property.states.get(1).or(property.states.first());
            start.rows = drawn
                .map(|state| StartsAsRow::new(state.id.clone(), Slot::Blank))
                .into_iter()
                .collect();
            start.otherwise = first;
            start
        }
        StartChoice::Value if selected != A_VALUE => {
            let mut start = StartsAs::new(StartRule::Value);
            start.value = number(0.0);
            start
        }
        StartChoice::RandomNumber if selected != A_RANDOM_NUMBER => {
            let mut phrase = SheetLine::new(LineKind::RandomNumber);
            phrase.low = number(0.0);
            phrase.high = number(1.0);
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Phrase(Box::new(phrase)));
            start
        }
        StartChoice::YesNo(yes) => {
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Literal(SheetLiteral::Bool(yes)));
            start
        }
        StartChoice::Anywhere if selected != ANYWHERE_AT_RANDOM => StartsAs::new(StartRule::Anywhere),
        StartChoice::OnGround(ground, state) => {
            let mut start = StartsAs::new(StartRule::OnGround);
            start.property = Some(ground);
            start.state = Some(state);
            start
        }
        _ => return,
    };
    *pending = Some(Pending::one(SheetEdit::SetStartsAs {
        property: property.id.clone(),
        starts_as: Some(Box::new(start)),
    }));
}

/// Draws the rows of the by-chance `start` of `property`, each on a line of its own, then "+ Add row" and the state
/// of every other cell.
fn chance_rows(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    start: &StartsAs,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    for (index, row) in start.rows.iter().enumerate() {
        let place = starts_row_place(&property.name, index + 1);
        wrapped_line(ui, |ui| {
            if let Some(state) = state_select(
                ui,
                property,
                row.state.as_deref(),
                ("henad_sheet_start_row", &property.id, index),
                &starts_field_name(&place, StartsField::State),
            ) {
                *pending = Some(start_edit(property.id.clone(), move |start| {
                    if let Some(row) = start.rows.get_mut(index) {
                        row.state = Some(state);
                    }
                }));
            }
            ui.add(Label::new(WITH_CHANCE).extend());
            start_number(ui, document, property, start, StartSlot::Row(index), pane, pending);
            let remove = delete_button(ui, &remove_row_name(index + 1, &property.name));
            if remove.clicked() {
                // The rows after it move up a place. Otherwise a refused text would show on the row that takes its
                // place.
                for later in index..start.rows.len() {
                    forget_field(ui, start_field_id(pane.widgets, &property.id, StartSlot::Row(later)));
                }
                *pending = Some(start_edit(property.id.clone(), move |start| {
                    if index < start.rows.len() {
                        start.rows.remove(index);
                    }
                }));
            }
        });
        start_messages(ui, pane.widgets, property, &[StartSlot::Row(index)]);
    }
    let add = line(ui, |ui| {
        super::widgets::add_button(ui, ADD_ROW, &add_row_name(&property.name), None, false)
    });
    if add.clicked() {
        // A new row draws the first state that no row draws yet.
        let drawn: Vec<Option<&str>> = start.rows.iter().map(|row| row.state.as_deref()).collect();
        let state = property
            .states
            .iter()
            .find(|state| !drawn.contains(&Some(state.id.as_str())))
            .or(property.states.first())
            .map(|state| state.id.clone());
        *pending = Some(start_edit(property.id.clone(), move |start| {
            let mut row = StartsAsRow::default();
            row.state = state;
            start.rows.push(row);
        }));
    }
    wrapped_line(ui, |ui| {
        ui.label(OTHERWISE);
        if let Some(state) = state_select(
            ui,
            property,
            start.otherwise.as_deref(),
            ("henad_sheet_start_otherwise", &property.id, 0),
            &starts_otherwise_name(&property.name),
        ) {
            *pending = Some(start_edit(property.id.clone(), move |start| {
                start.otherwise = Some(state);
            }));
        }
    });
}

/// Adds the delete button of a row, a "×" named `name` in the weak text colour.
fn delete_button(ui: &mut Ui, name: &str) -> Response {
    text_control(ui, None, REMOVE_ROW_LABEL, name, CHIP_HEIGHT)
}

/// Adds a select of the states of `property` showing `current`, named `name`, and returns the id of the state chosen.
///
/// Each state in the list shows its swatch.
fn state_select(
    ui: &mut Ui,
    property: &SheetProperty,
    current: Option<&str>,
    id_salt: (&str, &str, usize),
    name: &str,
) -> Option<String> {
    let shown = current
        .and_then(|state| state_name(property, state))
        .unwrap_or_default();
    let items: Vec<OverlayItem<'_>> = property
        .states
        .iter()
        .map(|state| OverlayItem {
            label: &state.name,
            current: current == Some(state.id.as_str()),
            swatch: Some(color32(state.color)),
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = select(ui, id_salt, name, shown, &items);
    let chosen = chosen.and_then(|index| property.states.get(index))?;
    (current != Some(chosen.id.as_str())).then(|| chosen.id.clone())
}

/// Returns the edit that changes the start of `property` by `change`, applied to the start as the sheet holds it.
fn start_edit(property: String, change: impl FnOnce(&mut StartsAs) + 'static) -> Pending {
    Pending {
        starts_only: true,
        form: None,
        run: Box::new(move |builder| {
            let mut start = start_of(builder.document(), &property)
                .ok_or_else(|| SheetEditError::Missing(format!("the property '{property}' has no start")))?;
            change(&mut start);
            builder
                .apply(SheetEdit::SetStartsAs {
                    property,
                    starts_as: Some(Box::new(start)),
                })
                .map(drop)
        }),
    }
}

/// Returns the start of property `property` of `document`.
fn start_of(document: &SheetDocument, property: &str) -> Option<StartsAs> {
    find_property(document, property)?.starts_as.clone()
}

/// Returns the number at `slot` of `start`.
fn start_slot(start: &StartsAs, slot: StartSlot) -> Option<&Slot> {
    match slot {
        StartSlot::Row(index) => start.rows.get(index).map(|row| &row.chance),
        StartSlot::Value => Some(&start.value),
        StartSlot::Low | StartSlot::High => match start.value.value() {
            Some(SheetValue::Phrase(phrase)) => Some(if slot == StartSlot::Low {
                &phrase.low
            } else {
                &phrase.high
            }),
            _ => None,
        },
    }
}

/// Puts `value` at `slot` of `start`. A slot the start does not hold is left alone.
fn set_start_slot(start: &mut StartsAs, slot: StartSlot, value: Slot) {
    match slot {
        StartSlot::Row(index) => {
            if let Some(row) = start.rows.get_mut(index) {
                row.chance = value;
            }
        }
        StartSlot::Value => start.value = value,
        StartSlot::Low | StartSlot::High => {
            if let Slot::Filled(SheetValue::Phrase(phrase)) = &mut start.value {
                if slot == StartSlot::Low {
                    phrase.low = value;
                } else {
                    phrase.high = value;
                }
            }
        }
    }
}

/// Returns the id of the field of the number at `slot` of the start of `property`, under `widgets`, the id the pane's
/// widgets are keyed under.
fn start_field_id(widgets: Id, property: &str, slot: StartSlot) -> Id {
    widgets.with(("henad_sheet_start_field", property, slot))
}

/// Draws the message under the row of the numbers at `slots` of the start of `property`, if one shows.
fn start_messages(ui: &mut Ui, widgets: Id, property: &SheetProperty, slots: &[StartSlot]) {
    if let Some(message) = slots
        .iter()
        .find_map(|slot| field_message(ui, start_field_id(widgets, &property.id, *slot)))
    {
        message_ui(ui, &message);
    }
}

/// Draws the number at `slot` of the start of `property`: a field with "Use a parameter" beside it, or the chip of the
/// parameter chosen. A click on the chip reopens the full picker.
fn start_number(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    start: &StartsAs,
    slot: StartSlot,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let chance = matches!(slot, StartSlot::Row(_));
    let place = match slot {
        StartSlot::Row(index) => starts_row_place(&property.name, index + 1),
        StartSlot::Value => starts_place(&property.name),
        StartSlot::Low => starts_low_place(&property.name),
        StartSlot::High => starts_high_place(&property.name),
    };
    let field = NumberField {
        place,
        id: start_field_id(pane.widgets, &property.id, slot),
        current: start_slot(start, slot),
        target: NumberTarget::Start {
            property: property.id.clone(),
            slot,
        },
        number_use: if chance { NumberUse::Chance } else { NumberUse::Number },
        limit: None,
        after: if chance {
            text_control_width(ui, REMOVE_ROW_LABEL) + ITEM_GAP
        } else {
            0.0
        },
    };
    number_field(ui, document, field, pane, pending);
}

/// Draws `field`: a text field with "Use a parameter" beside it, or the chip of the parameter it holds. A click on the
/// chip reopens the full picker. A typed number, a parameter picked or a new parameter fills the field's target.
fn number_field(
    ui: &mut Ui,
    document: &SheetDocument,
    field: NumberField<'_>,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let NumberField {
        place,
        id,
        current,
        target,
        number_use,
        limit,
        after,
    } = field;
    let chance = number_use == NumberUse::Chance;
    let choice = match current.and_then(Slot::value) {
        Some(SheetValue::Chip(SheetChip::Param(parameter))) => {
            let name = document
                .parameters
                .iter()
                .find(|known| known.id == *parameter)
                .map_or_else(|| parameter.clone(), |known| known.name.clone());
            let chip = SentenceChip {
                kind: ChipKind::Parameter(parameter.clone()),
                lead: "",
                name,
            };
            let width = ui.max_rect().width() - after;
            let response = chip_within(ui, Some(id.with("chip")), &chip, false, None, width);
            let blank = if chance { Blank::Chance } else { Blank::Number };
            let title = blank.picker_title();
            Picker {
                title: &title,
                groups: vec![parameters_group(document)],
                new_parameter: true,
                number_box: Some(number_use),
                empty: "",
            }
            .show(ui, &response)
        }
        value => {
            let text = match value {
                Some(SheetValue::Literal(literal)) => literal_words(*literal),
                _ => String::new(),
            };
            let word = if chance {
                StartsField::Chance
            } else {
                StartsField::Number
            };
            commit_number(ui, id, &starts_field_name(&place, word), &text, FIELD_WIDTH, |typed| {
                let value = if typed.trim().is_empty() {
                    if let Some((_, _, message)) = limit {
                        return Some(message.to_owned());
                    }
                    Slot::Blank
                } else {
                    match number_use.read(typed) {
                        Ok(literal) => {
                            if let Some((low, high, message)) = limit
                                && !literal.number().is_some_and(|number| (low..=high).contains(&number))
                            {
                                return Some(message.to_owned());
                            }
                            Slot::Filled(SheetValue::Literal(literal))
                        }
                        Err(message) if message == TYPE_ONE_NUMBER && chance => {
                            return Some(TYPE_A_CHANCE.to_owned());
                        }
                        Err(message) => return Some(message.to_owned()),
                    }
                };
                *pending = Some(target.edit(value));
                None
            });
            let button = text_control(
                ui,
                Some(id.with("use")),
                USE_A_PARAMETER,
                &starts_field_name(&place, StartsField::UseParameter),
                CHIP_HEIGHT,
            );
            Picker {
                title: CHOOSE_A_PARAMETER,
                groups: vec![parameters_group(document)],
                new_parameter: true,
                number_box: None,
                empty: "",
            }
            .show(ui, &button)
        }
    };
    pick_number(document, current, target, number_use, choice, pane, pending);
}

/// Takes `choice`, made in the picker of the number `target` that holds `current`: a parameter or a typed number goes
/// into the target, and "New parameter..." opens the New parameter dialog.
fn pick_number(
    document: &SheetDocument,
    current: Option<&Slot>,
    target: NumberTarget,
    number_use: NumberUse,
    choice: Option<PickerChoice>,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let value = match choice {
        Some(PickerChoice::Item { item, .. }) => match document.parameters.get(item) {
            Some(parameter) => Slot::Filled(SheetValue::Chip(SheetChip::Param(parameter.id.clone()))),
            None => return,
        },
        Some(PickerChoice::NewParameter) => {
            let typed = match current.and_then(Slot::value) {
                Some(SheetValue::Literal(literal)) => literal.number(),
                _ => None,
            };
            pane.new_parameter = Some((Some(target), NewParameterDialog::new(number_use, typed, None)));
            return;
        }
        Some(PickerChoice::Number(literal)) => Slot::Filled(SheetValue::Literal(literal)),
        None => return,
    };
    *pending = Some(target.edit(value));
}

/// Returns the group of the sheet's parameters in a picker, each with its value and unit in grey.
fn parameters_group(document: &SheetDocument) -> PickerGroup {
    PickerGroup {
        heading: PARAMETERS,
        items: document
            .parameters
            .iter()
            .map(|parameter| {
                let value = format_number(parameter.default).unwrap_or_default();
                PickerItem {
                    label: parameter.name.clone(),
                    detail: Some(match &parameter.unit {
                        Some(unit) => format!("{value} {unit}"),
                        None => value,
                    }),
                }
            })
            .collect(),
    }
}

/// Draws the New parameter dialog while it is open. Create adds the parameter and puts it in the number it was opened
/// from, as one edit, or adds it alone when "+ Add parameter" opened the dialog.
///
/// An edit that fills a start counts as a changed start, since only the start uses the new parameter. Past the start
/// it therefore waits for Start again, as a parameter chosen from the picker does.
fn new_parameter(ui: &Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Option<Pending>) {
    let Some((target, dialog)) = &mut pane.new_parameter else {
        return;
    };
    let taken: Vec<&str> = document.parameters.iter().map(|known| known.name.as_str()).collect();
    match new_parameter_modal(ui.ctx(), dialog, &taken) {
        DialogOutcome::Open => {}
        DialogOutcome::Canceled => pane.new_parameter = None,
        DialogOutcome::Created(parameter) => {
            let target = target.clone();
            pane.new_parameter = None;
            *pending = Some(Pending {
                starts_only: matches!(target, Some(NumberTarget::Start { .. })),
                form: None,
                run: Box::new(move |builder| {
                    let made = builder.apply(SheetEdit::AddParameter {
                        name: parameter.name,
                        parameter_type: henad_graph::sheet::document::ParameterType::Number,
                        min: parameter.min,
                        max: parameter.max,
                        default: parameter.default,
                        unit: parameter.unit,
                    })?;
                    let Some(target) = target else {
                        return Ok(());
                    };
                    let id = made.into_iter().next().unwrap_or_default();
                    (target.edit(Slot::Filled(SheetValue::Chip(SheetChip::Param(id)))).run)(builder)
                }),
            });
        }
    }
}

/// Draws the property form, and sets `close` on Cancel. Done checks the form and adds or changes the property.
fn property_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut PropertyForm,
    close: &mut bool,
    pending: &mut Option<Pending>,
) {
    let heading = match &form.editing {
        Some(_) => edit_property_heading(&form.original),
        None => NEW_PROPERTY.to_owned(),
    };
    form_heading(ui, &heading);
    let rows = Form::new(ui, &[NAME_LABEL, KIND_LABEL]);
    rows.row(ui, NAME_LABEL, |ui| {
        let field = ui.add(
            TextEdit::singleline(&mut form.name)
                .hint_text(PROPERTY_NAME_PLACEHOLDER)
                .desired_width(ui.available_width().min(STATE_FIELD_WIDTH + 60.0)),
        );
        set_name(ui, field.id, PROPERTY_NAME_FIELD);
        if std::mem::take(&mut form.focus_name) {
            field.request_focus();
        }
    });
    rows.row(ui, KIND_LABEL, |ui| {
        for kind in form_kinds(document, form) {
            let radio = ui.radio_value(&mut form.kind, kind, kind_words(kind));
            if kind == PropertyType::WholeNumber {
                radio.on_hover_text(INTEGER_KIND_TOOLTIP);
            }
        }
    });
    rows.under(ui, |ui| {
        let description = match form.kind {
            PropertyType::States => STATES_KIND_DESCRIPTION,
            PropertyType::WholeNumber => INTEGER_KIND_TOOLTIP,
            PropertyType::YesNo => YES_NO_KIND_DESCRIPTION,
            PropertyType::Position => POSITION_KIND_DESCRIPTION,
            PropertyType::Heading => HEADING_KIND_DESCRIPTION,
            PropertyType::Arrow => ARROW_KIND_DESCRIPTION,
            _ => NUMBER_KIND_DESCRIPTION,
        };
        ui.weak(description);
        if form.kind == PropertyType::States {
            state_rows(ui, form);
        }
        if let Some(message) = &form.message {
            message_ui(ui, message);
        }
    });
    form_buttons(ui, |ui| {
        if ui.add(filled_button(DONE.to_owned())).clicked() {
            match check_form(document, form) {
                Ok(()) => *pending = Some(Pending::from_form(FormKind::Property, form_edit(form.clone()))),
                Err(message) => form.message = Some(message),
            }
        }
        if ui.button(CANCEL).clicked() {
            *close = true;
        }
        if let Some(id) = &form.editing
            && ui.add(destructive_button(DELETE_PROPERTY)).clicked()
        {
            let item = SheetItem::Property(id.clone());
            *pending = Some(Pending::from_form(
                FormKind::Property,
                Box::new(move |builder| builder.apply(SheetEdit::Delete { item }).map(drop)),
            ));
        }
    });
}

/// Draws the heading of a form inside a section, such as "New property", in the semibold face.
fn form_heading(ui: &mut Ui, heading: &str) {
    ui.add_space(2.0);
    line(ui, |ui| {
        let font = semibold_font(ui.ctx(), egui::TextStyle::Body.resolve(ui.style()).size);
        ui.add(Label::new(
            RichText::new(heading)
                .font(font)
                .color(ui.visuals().strong_text_color()),
        ));
    });
}

/// Draws the row of a form's buttons, Done first, at the form's left edge, after a little space.
fn form_buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = ITEM_GAP * 2.0;
        add(ui);
    });
}

/// Draws the rows of states of the property form: each with its colour, its name and its remove button, then "+ Add
/// state". Enter in a name moves to the next state, or adds one after the last.
fn state_rows(ui: &mut Ui, form: &mut PropertyForm) {
    let mut remove = None;
    let mut entered = None;
    let focus = form.focus_state.take();
    for (index, state) in form.states.iter_mut().enumerate() {
        line(ui, |ui| {
            let color_label = state_color_name(index + 1, color_name(state.color));
            if let Some(color) = color_button(ui, &color_label, state.color) {
                state.color = color;
                state.picked = true;
            }
            let width = (ui.available_width() - DELETE_WIDTH - ITEM_GAP).clamp(48.0, STATE_FIELD_WIDTH);
            let field = ui.add(TextEdit::singleline(&mut state.name).desired_width(width));
            set_name(ui, field.id, &state_name_field(index + 1));
            if focus == Some(index) {
                field.request_focus();
            }
            if field.changed() && !state.picked {
                state.color = suggested_state_color(&state.name).unwrap_or_else(|| default_color(index));
            }
            if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                entered = Some(index);
            }
            if delete_button(ui, &remove_state_name(index + 1)).clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        form.states.remove(index);
    }
    if let Some(index) = entered {
        if index + 1 == form.states.len() {
            form.states.push(FormState::empty(index + 1));
        }
        form.focus_state = Some(index + 1);
    }
    let add = line(ui, |ui| {
        super::widgets::add_button(ui, ADD_STATE, ADD_STATE, None, false)
    });
    if add.clicked() {
        form.states.push(FormState::empty(form.states.len()));
        form.focus_state = Some(form.states.len() - 1);
    }
}

/// Returns the kinds the property form offers: States and Number for every kind of thing, Integer for cells and
/// agents, then for agents Yes or no, and a Position or a Heading while the agents have none. A property's own kind is
/// always offered.
///
/// The ground holds only numbers and states.
fn form_kinds(document: &SheetDocument, form: &PropertyForm) -> Vec<PropertyType> {
    let mut kinds = vec![PropertyType::States, PropertyType::Number];
    if form.thing != GROUND {
        kinds.push(PropertyType::WholeNumber);
    }
    if form.thing != CELLS && form.thing != GROUND {
        kinds.push(PropertyType::YesNo);
        kinds.push(PropertyType::Arrow);
        for built_in in [PropertyType::Position, PropertyType::Heading] {
            let held = properties_of(document, &form.thing)
                .iter()
                .any(|property| property.property_type == built_in && Some(&property.id) != form.editing.as_ref());
            if !held {
                kinds.push(built_in);
            }
        }
    }
    let own = form
        .editing
        .as_deref()
        .and_then(|id| find_property(document, id))
        .map(|property| property.property_type);
    if let Some(own) = own
        && !kinds.contains(&own)
    {
        kinds.push(own);
    }
    kinds
}

/// Returns the colour a new state takes at row `index`.
fn default_color(index: usize) -> Color {
    STATE_COLORS[index % STATE_COLORS.len()]
}

/// Returns the message for the first check of the property form that fails, in the order of the page.
///
/// # Errors
///
/// Returns the message on an empty or taken name, the name of a new property that Henad keeps for itself, a States
/// property with fewer than two named states, or two states of one name.
fn check_form(document: &SheetDocument, form: &PropertyForm) -> Result<(), String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(PROPERTY_NAME_MISSING.to_owned());
    }
    let taken = properties_of(document, &form.thing)
        .iter()
        .filter(|property| Some(&property.id) != form.editing.as_ref())
        .any(|property| property.name.to_lowercase() == name.to_lowercase());
    if taken {
        return Err(property_name_taken(name));
    }
    // A rename keeps the id, so only a new property mints one.
    if form.editing.is_none() && is_reserved_name(name, IdSpace::Item) {
        return Err(name_reserved(name));
    }
    if form.kind == PropertyType::States {
        let names: Vec<String> = form
            .states
            .iter()
            .map(|state| state.name.trim().to_lowercase())
            .filter(|state| !state.is_empty())
            .collect();
        if names.len() < 2 {
            return Err(TOO_FEW_STATES.to_owned());
        }
        if (1..names.len()).any(|index| names[..index].contains(&names[index])) {
            return Err(STATE_NAMES_REPEATED.to_owned());
        }
    }
    Ok(())
}

/// Returns the edit Done makes from `form`: a new property with its states and its first start, or the changes to a
/// property the form edits.
///
/// A new property starts in its first state, at 0, at no, anywhere at random for a Position, and with no start for a
/// Heading. A changed kind clears every reference to the property and starts it afresh. A removed state clears its
/// references, its measures and its start rows.
fn form_edit(form: PropertyForm) -> EditRun {
    Box::new(move |builder| {
        let states: Vec<FormState> = form
            .states
            .into_iter()
            .filter(|state| !state.name.trim().is_empty())
            .collect();
        let name = form.name.trim().to_owned();
        let id = match &form.editing {
            None => builder
                .apply(SheetEdit::AddProperty {
                    thing: form.thing.clone(),
                    name,
                    property_type: form.kind,
                })?
                .into_iter()
                .next()
                .unwrap_or_default(),
            Some(id) => {
                if name != form.original {
                    builder.apply(SheetEdit::Rename {
                        item: SheetItem::Property(id.clone()),
                        name,
                    })?;
                }
                id.clone()
            }
        };
        let current = find_property(builder.document(), &id)
            .cloned()
            .ok_or_else(|| SheetEditError::Missing(format!("the sheet has no property '{id}'")))?;
        let fresh = form.editing.is_none() || current.property_type != form.kind;
        if !fresh {
            if form.kind == PropertyType::States {
                change_states(builder, &current, &states)?;
            }
            return Ok(());
        }
        if form.editing.is_some() {
            builder.apply(SheetEdit::SetPropertyType {
                property: id.clone(),
                property_type: form.kind,
            })?;
        }
        let mut first = None;
        if form.kind == PropertyType::States {
            for state in &states {
                let made = builder.apply(SheetEdit::AddState {
                    property: id.clone(),
                    name: state.name.trim().to_owned(),
                    color: Some(state.color),
                })?;
                if first.is_none() {
                    first = made.into_iter().next();
                }
            }
        }
        let start = match form.kind {
            PropertyType::Heading => None,
            PropertyType::Position => Some(StartsAs::new(StartRule::Anywhere)),
            PropertyType::Arrow => {
                // An arrow starts in a random direction of length 1, a start no control of the sheet changes.
                let mut direction = SheetLine::new(LineKind::RandomDirection);
                direction.length = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0)));
                let mut start = StartsAs::new(StartRule::Value);
                start.value = Slot::Filled(SheetValue::Phrase(Box::new(direction)));
                Some(start)
            }
            kind => {
                let mut start = StartsAs::new(StartRule::Value);
                start.value = Slot::Filled(match first {
                    Some(state) => SheetValue::Chip(SheetChip::State(state)),
                    None if kind == PropertyType::YesNo => SheetValue::Literal(SheetLiteral::Bool(false)),
                    None => SheetValue::Literal(SheetLiteral::Number(0.0)),
                });
                Some(start)
            }
        };
        builder.apply(SheetEdit::SetStartsAs {
            property: id.clone(),
            starts_as: start.map(Box::new),
        })?;
        // The first property made colours the cells. A States property does so through its command.
        let uncolored = form.thing == CELLS
            && !builder.document().colors.contains_key(CELLS)
            && !properties_of(builder.document(), CELLS)
                .iter()
                .any(|property| property.property_type == PropertyType::States);
        if form.editing.is_none() && uncolored {
            builder.apply(SheetEdit::SetColorRule {
                thing: CELLS.to_owned(),
                rule: Some(shaded_rule(&id, None)),
            })?;
        }
        Ok(())
    })
}

/// Changes the states of `property` to the rows `states` of the property form: removed states go, the others take
/// their new names and colours, and new rows become new states.
fn change_states(
    builder: &mut SheetBuilder,
    property: &SheetProperty,
    states: &[FormState],
) -> Result<(), SheetEditError> {
    let kept: Vec<&str> = states.iter().filter_map(|state| state.id.as_deref()).collect();
    for state in &property.states {
        if !kept.contains(&state.id.as_str()) {
            builder.apply(SheetEdit::Delete {
                item: SheetItem::State {
                    property: property.id.clone(),
                    state: state.id.clone(),
                },
            })?;
        }
    }
    for row in states {
        let name = row.name.trim().to_owned();
        let Some(id) = &row.id else {
            builder.apply(SheetEdit::AddState {
                property: property.id.clone(),
                name,
                color: Some(row.color),
            })?;
            continue;
        };
        let Some(state) = property.states.iter().find(|state| state.id == *id) else {
            continue;
        };
        let item = || SheetItem::State {
            property: property.id.clone(),
            state: id.clone(),
        };
        if state.name != name {
            builder.apply(SheetEdit::Rename { item: item(), name })?;
        }
        if state.color != row.color {
            builder.apply(SheetEdit::Recolor {
                item: item(),
                color: row.color,
            })?;
        }
    }
    Ok(())
}

/// Returns the numbers of the lines that change property `property_id` on the cards that run each tick: a Become, a
/// Set, a Steer or a Keep between of it, a Leave on it, and for an agent's Position or Heading a Move.
fn changing_lines(document: &SheetDocument, property_id: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    for card in document.cards.iter().filter(|card| card.runs == CardRuns::EachTick) {
        let moved = properties_of(document, &card.every).iter().any(|property| {
            property.id == property_id
                && matches!(property.property_type, PropertyType::Position | PropertyType::Heading)
        });
        let relations = CardRelations::new(document, card);
        for &line_id in relations.order() {
            let changes = relations.line(line_id).is_some_and(|line| match line.kind {
                LineKind::Become | LineKind::Set | LineKind::Steer | LineKind::KeepBetween => {
                    line.property.as_deref() == Some(property_id)
                }
                LineKind::Leave => line.ground_property.as_deref() == Some(property_id),
                LineKind::Move => moved,
                _ => false,
            });
            if changes && let Some(number) = relations.number(line_id) {
                numbers.push(number);
            }
        }
    }
    numbers
}

/// Returns the properties of the kind of thing `thing`: `cells`, `ground` or an agent kind's id. A kind of thing the
/// sheet lacks has none.
fn properties_of<'a>(document: &'a SheetDocument, thing: &str) -> &'a [SheetProperty] {
    match thing {
        CELLS => document.cells.as_deref().unwrap_or_default(),
        GROUND => document
            .ground
            .as_ref()
            .map(|ground| ground.properties.as_slice())
            .unwrap_or_default(),
        _ => document
            .agents
            .iter()
            .find(|kind| kind.id == thing)
            .map(|kind| kind.properties.as_slice())
            .unwrap_or_default(),
    }
}

/// Returns every property of `document`: those of the cells, of each kind of agent, then of the ground.
pub(super) fn all_properties(document: &SheetDocument) -> impl Iterator<Item = &SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
}

/// Returns the property `id` of any kind of thing of `document`.
fn find_property<'a>(document: &'a SheetDocument, id: &str) -> Option<&'a SheetProperty> {
    all_properties(document).find(|property| property.id == id)
}

/// Returns the kind of thing the Measures and Buttons sections act on: the cells of a grid, or the first kind of agent.
fn measured_thing(document: &SheetDocument) -> &str {
    match document.world.made_of {
        MadeOf::Agents => document.agents.first().map_or(CELLS, |kind| kind.id.as_str()),
        _ => CELLS,
    }
}

/// Returns the step of a slider over `range`: about a 200th of it, rounded down to 1, 2 or 5 times a power of ten.
pub fn slider_step(range: f64) -> f64 {
    if !(range.is_finite() && range > 0.0) {
        return 0.01;
    }
    let raw = range / 200.0;
    let power = 10_f64.powf(raw.log10().floor());
    let ratio = raw / power;
    let step = if ratio >= 5.0 {
        5.0
    } else if ratio >= 2.0 {
        2.0
    } else {
        1.0
    } * power;
    // Three significant figures drop the float noise of the power of ten.
    format!("{step:.2e}").parse().unwrap_or(step)
}

/// End of a parameter's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeEnd {
    /// The smallest value of the slider.
    Min,
    /// The largest value of the slider.
    Max,
}

/// Returns the range and the value of `parameter` once `end` of its range is `typed`, as `(min, max, default)`.
///
/// A minimum above the maximum swaps the two, equal ends make the maximum one more, and the value moves into the
/// range.
pub fn with_range_end(parameter: &SheetParameter, end: RangeEnd, typed: f64) -> (f64, f64, f64) {
    let (mut min, mut max) = match end {
        RangeEnd::Min => (typed, parameter.max),
        RangeEnd::Max => (parameter.min, typed),
    };
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }
    if min == max {
        max = min + 1.0;
    }
    (min, max, parameter.default.clamp(min, max))
}

/// Returns the number typed as `text`, a percent read as its hundredth, or the message for text that is no number.
fn typed_number(text: &str) -> Result<f64, String> {
    read_typed_number(text)
        .and_then(SheetLiteral::number)
        .ok_or_else(|| TYPE_ONE_NUMBER.to_owned())
}

/// Draws the table of the parameters of `document`: each parameter's name, value as a slider and a field, Min, Max and
/// Unit, and delete button, with the value the world tries under a parameter in `trials`. "+ Add parameter" opens the
/// New parameter dialog.
///
/// A table with room for every column at its comfortable width draws each parameter on one line, and Value takes the
/// room left over. A narrower table draws Min, Max and Unit on a second line under each parameter, each after its small
/// label, and short of room Value gives up width before Name.
fn parameters(
    ui: &mut Ui,
    document: &SheetDocument,
    pane: &mut PaneState,
    trials: &[SheetTrial],
    pending: &mut Option<Pending>,
) {
    if !document.parameters.is_empty() {
        let names = document
            .parameters
            .iter()
            .map(|parameter| text_width(ui, &parameter.name) + 12.0)
            .fold(0.0, f32::max)
            .min(PARAMETER_NAME_WIDTH + 20.0);
        let value = Column::new(
            PARAMETER_COLUMNS[1],
            SLIDER_WIDTH + ITEM_GAP + FIELD_WIDTH,
            36.0 + ITEM_GAP + 48.0,
        );
        let name = Column::new(PARAMETER_COLUMNS[0], names, 64.0);
        let one_line = names + 120.0 + 2.0 * RANGE_WIDTH + UNIT_WIDTH + DELETE_WIDTH + 5.0 * layout::COLUMN_GAP;
        let wide = ui.available_width() >= one_line;
        let (columns, give_up): (Vec<Column>, &[(usize, f32)]) = if wide {
            (
                vec![
                    name,
                    value,
                    Column::new(PARAMETER_COLUMNS[2], RANGE_WIDTH, 36.0),
                    Column::new(PARAMETER_COLUMNS[3], RANGE_WIDTH, 36.0),
                    Column::new(PARAMETER_COLUMNS[4], UNIT_WIDTH, 44.0),
                    Column::fixed(DELETE_WIDTH),
                ],
                &[(4, 0.0), (1, 0.0), (0, 0.0), (2, 0.0), (3, 0.0)],
            )
        } else {
            (vec![name, value, Column::fixed(DELETE_WIDTH)], &[(1, 0.0), (0, 0.0)])
        };
        let id = ui.make_persistent_id("henad_sheet_parameters");
        Table::new(id, columns, 1, give_up).show(ui, |ui, rows| {
            for parameter in &document.parameters {
                rows.row(ui, &parameter.id, |row| {
                    parameter_row(row, document, parameter, wide, pane, pending);
                    if let Some(trial) = trials.iter().find(|trial| trial.parameter == parameter.id) {
                        let kept = format_number(parameter.default).unwrap_or_default();
                        let tried = format_number(trial.value).unwrap_or_default();
                        row.below(0, |ui| {
                            ui.add(Label::new(RichText::new(sheet_trying_value(&tried, &kept)).weak()).wrap());
                        });
                    }
                });
            }
        });
        ui.add_space(6.0);
    }
    if super::widgets::add_button(ui, ADD_PARAMETER, ADD_PARAMETER, None, false).clicked() {
        pane.new_parameter = Some((None, NewParameterDialog::new(NumberUse::Number, None, None)));
    }
}

/// Width in points of a parameter's Min or Max field while its table has room.
const RANGE_WIDTH: f32 = 48.0;

/// The fields of one parameter's row, each keyed under the pane's widgets, and the edits they commit.
struct ParameterFields<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// The parameter.
    parameter: &'a SheetParameter,
    /// Id the pane's widgets are keyed under.
    widgets: Id,
}

impl ParameterFields<'_> {
    /// Returns the id of the field `part` of the parameter.
    fn id(&self, part: &str) -> Id {
        self.widgets
            .with(("henad_sheet_parameter", self.parameter.id.as_str(), part))
    }

    /// Returns the edit that gives the parameter the range `min` to `max`, the value `default` and the unit `unit`.
    fn set(&self, min: f64, max: f64, default: f64, unit: Option<String>) -> Pending {
        Pending::one(SheetEdit::SetParameter {
            parameter: self.parameter.id.clone(),
            min,
            max,
            default,
            step: self.parameter.step,
            unit,
            means: self.parameter.means.clone(),
        })
    }

    /// Adds the name field, as wide as `ui`. An empty or taken name puts the old name back and sets `message`.
    fn name(&self, ui: &mut Ui, message: &mut Option<(String, String)>, pending: &mut Option<Pending>) {
        let parameter = self.parameter;
        commit_field(
            ui,
            self.id("name"),
            &sheet_parameter_name_field(&parameter.name),
            &parameter.name,
            ui.available_width(),
            |text| {
                let name = text.trim();
                let taken = self
                    .document
                    .parameters
                    .iter()
                    .any(|other| other.id != parameter.id && other.name.to_lowercase() == name.to_lowercase());
                // A refused name puts the old name back, with the reason under the row.
                *message = if name.is_empty() {
                    Some((parameter.id.clone(), PARAMETER_NAME_MISSING.to_owned()))
                } else if taken {
                    Some((parameter.id.clone(), parameter_name_taken(name)))
                } else {
                    *pending = Some(Pending::one(SheetEdit::Rename {
                        item: SheetItem::Parameter(parameter.id.clone()),
                        name: name.to_owned(),
                    }));
                    None
                };
                None
            },
        );
    }

    /// Adds the value as a slider and a field, together as wide as `ui`.
    fn value(&self, ui: &mut Ui, pending: &mut Option<Pending>) {
        let parameter = self.parameter;
        let field = FIELD_WIDTH.min(ui.available_width() - 36.0 - ITEM_GAP).max(36.0);
        let slider = ui.available_width() - field - ITEM_GAP;
        let set = |min, max, default, unit| self.set(min, max, default, unit);
        let shown = value_slider(ui, parameter, self.id("slider"), slider, pending, &set);
        commit_number(
            ui,
            self.id("value"),
            &value_field_name(&parameter.name),
            &format_number(shown).unwrap_or_default(),
            field,
            |text| match typed_number(text) {
                Ok(value) => {
                    // A typed value outside the range widens it.
                    *pending = Some(self.set(
                        parameter.min.min(value),
                        parameter.max.max(value),
                        value,
                        parameter.unit.clone(),
                    ));
                    None
                }
                Err(message) => Some(message),
            },
        );
    }

    /// Adds the field of the end `end` of the range, `width` points wide.
    fn range(&self, ui: &mut Ui, end: RangeEnd, width: f32, pending: &mut Option<Pending>) {
        let parameter = self.parameter;
        let (label, name, current) = match end {
            RangeEnd::Min => (MIN_LABEL, parameter_min_name(&parameter.name), parameter.min),
            RangeEnd::Max => (MAX_LABEL, parameter_max_name(&parameter.name), parameter.max),
        };
        commit_number(
            ui,
            self.id(label),
            &name,
            &format_number(current).unwrap_or_default(),
            width,
            |text| match typed_number(text) {
                Ok(typed) => {
                    let (min, max, default) = with_range_end(parameter, end, typed);
                    *pending = Some(self.set(min, max, default, parameter.unit.clone()));
                    None
                }
                Err(message) => Some(message),
            },
        );
    }

    /// Adds the unit field, `width` points wide.
    fn unit(&self, ui: &mut Ui, width: f32, pending: &mut Option<Pending>) {
        let parameter = self.parameter;
        suggestion_field(
            ui,
            self.id("unit"),
            &parameter_unit_name(&parameter.name),
            parameter.unit.as_deref().unwrap_or_default(),
            UNIT_FIELD_PLACEHOLDER,
            width,
            &unit_suggestions(),
            |text| {
                let unit = text.trim();
                *pending = Some(self.set(
                    parameter.min,
                    parameter.max,
                    parameter.default,
                    (!unit.is_empty()).then(|| unit.to_owned()),
                ));
                None
            },
        );
    }

    /// Adds Min, Max and Unit on wrapping lines, each field after its small label. A label stays on its field's line.
    fn range_line(&self, ui: &mut Ui, pending: &mut Option<Pending>) {
        wrapped_line(ui, |ui| {
            let mut first = true;
            for (header, end) in [
                (MIN_LABEL, Some(RangeEnd::Min)),
                (MAX_LABEL, Some(RangeEnd::Max)),
                (UNIT_LABEL, None),
            ] {
                let field = if end.is_some() { RANGE_WIDTH } else { UNIT_WIDTH };
                let width = caption_width(ui, header) + ITEM_GAP + field;
                // egui wraps no nested group. A label and its field move to the next line by hand.
                if !first && width > ui.available_size_before_wrap().x {
                    ui.end_row();
                }
                first = false;
                caption(ui, header);
                match end {
                    Some(end) => self.range(ui, end, field, pending),
                    None => self.unit(ui, field, pending),
                }
                ui.add_space(ITEM_GAP);
            }
        });
    }
}

/// Draws the cells of `parameter` in `row`: its name, its value as a slider and a field, its Min, Max and Unit, and its
/// delete button, with the message of a refused field under the field. Min, Max and Unit take columns of their own
/// while `wide` holds, and a second line after their small labels otherwise.
fn parameter_row(
    row: &mut layout::TableRow<'_>,
    document: &SheetDocument,
    parameter: &SheetParameter,
    wide: bool,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let fields = ParameterFields {
        document,
        parameter,
        widgets: pane.widgets,
    };
    let message = &mut pane.parameter_message;
    row.cell(0, |ui| line(ui, |ui| fields.name(ui, message, pending)));
    row.cell(1, |ui| line(ui, |ui| fields.value(ui, pending)));
    let last = if wide { 5 } else { 2 };
    if wide {
        row.cell(2, |ui| {
            line(ui, |ui| fields.range(ui, RangeEnd::Min, ui.available_width(), pending));
        });
        row.cell(3, |ui| {
            line(ui, |ui| fields.range(ui, RangeEnd::Max, ui.available_width(), pending));
        });
        row.cell(4, |ui| line(ui, |ui| fields.unit(ui, ui.available_width(), pending)));
    }
    row.cell(last, |ui| {
        line(ui, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if delete_button(ui, &delete_parameter_name(&parameter.name)).clicked() {
                    *pending = Some(Pending::one(SheetEdit::Delete {
                        item: SheetItem::Parameter(parameter.id.clone()),
                    }));
                }
            });
        });
    });
    if !wide {
        row.below(0, |ui| fields.range_line(ui, pending));
    }
    // The message of a refused field sits under that field's column, the name's under the name.
    let refused = [(1, "value"), (2, MIN_LABEL), (3, MAX_LABEL), (4, "unit")]
        .into_iter()
        .find_map(|(column, part)| row.message(fields.id(part)).map(|text| (column, text)))
        .or_else(|| {
            message
                .as_ref()
                .filter(|(id, _)| *id == parameter.id)
                .map(|(_, text)| (0, text.clone()))
        });
    if let Some((column, refused)) = refused {
        // On two lines, Min, Max and Unit start under the name, and so do their messages.
        let column = if wide || column == 1 { column } else { 0 };
        row.below(column, |ui| message_ui(ui, &refused));
    }
}

/// Adds the slider of the value of `parameter`, `width` points long, named by its value slider name and keyed by `id`,
/// and returns the value it shows.
///
/// A drag holds its value until the slider is let go, and then commits it as one edit, the command `set` returns. A
/// click or a key commits at once. A parameter with no range to slide over shows no slider.
fn value_slider(
    ui: &mut Ui,
    parameter: &SheetParameter,
    id: Id,
    width: f32,
    pending: &mut Option<Pending>,
    set: &impl Fn(f64, f64, f64, Option<String>) -> Pending,
) -> f64 {
    let held: Option<f64> = ui.data(|data| data.get_temp(id));
    let mut value = held.unwrap_or(parameter.default);
    if parameter.min.partial_cmp(&parameter.max) != Some(std::cmp::Ordering::Less) {
        return value;
    }
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = width;
            ui.add(
                Slider::new(&mut value, parameter.min..=parameter.max)
                    .step_by(slider_step(parameter.max - parameter.min))
                    .show_value(false),
            )
        })
        .inner;
    set_name(ui, response.id, &value_slider_name(&parameter.name));
    if response.is_pointer_button_down_on() {
        if response.changed() || held.is_some() {
            ui.data_mut(|data| data.insert_temp(id, value));
        }
        return value;
    }
    ui.data_mut(|data| data.remove::<f64>(id));
    // The slider rounds a default off its steps as it draws. egui reports that as a change. Only the author's click,
    // drag or key edits the default, and otherwise the slider shows the stored default.
    let touched = response.has_focus() || response.clicked() || response.drag_stopped();
    let released = if response.changed() && touched {
        Some(value)
    } else {
        held
    };
    if let Some(released) = released
        && released != parameter.default
    {
        *pending = Some(set(parameter.min, parameter.max, released, parameter.unit.clone()));
    }
    released.unwrap_or(parameter.default)
}

/// Returns how `state` of `property` is named on its own: its name, or the property's name and its name when another
/// property of the kind of thing measured holds a state of that name.
fn state_label(document: &SheetDocument, property: &SheetProperty, state: &SheetState) -> String {
    let shared = properties_of(document, measured_thing(document)).iter().any(|other| {
        other.id != property.id
            && other
                .states
                .iter()
                .any(|known| known.name.eq_ignore_ascii_case(&state.name))
    });
    if shared {
        format!("{} {}", property.name, state.name)
    } else {
        state.name.clone()
    }
}

/// Returns the measures of `document` that count the cells whose property `property` is `state`.
fn counts_of<'a>(document: &'a SheetDocument, property: &str, state: &str) -> Vec<&'a SheetMeasure> {
    document
        .measures
        .iter()
        .filter(|measure| {
            measure.rule == MeasureRule::CountInState
                && measure.property.as_deref() == Some(property)
                && measure.state.as_deref() == Some(state)
        })
        .collect()
}

/// Returns the measures of `document` that average the property `property`.
fn averages_of<'a>(document: &'a SheetDocument, property: &str) -> Vec<&'a SheetMeasure> {
    document
        .measures
        .iter()
        .filter(|measure| {
            measure.rule == MeasureRule::Average
                && matches!(measure.value.value(), Some(SheetValue::Chip(SheetChip::My(id))) if id == property)
        })
        .collect()
}

/// Returns the command that adds a measure counting the cells or agents whose `property` is `state`, named by the
/// state.
fn add_count(document: &SheetDocument, property: &SheetProperty, state: &SheetState) -> SheetEdit {
    let taken = document
        .measures
        .iter()
        .any(|measure| measure.name.eq_ignore_ascii_case(&state.name));
    let name = if taken {
        format!("{} {}", property.name, state.name)
    } else {
        state_label(document, property, state)
    };
    SheetEdit::AddMeasure {
        name,
        rule: MeasureRule::CountInState,
        over: Some(measured_thing(document).to_owned()),
        property: Some(property.id.clone()),
        state: Some(state.id.clone()),
        event: None,
        value: Slot::Blank,
        color: None,
    }
}

/// Returns the words of `measure` in the Plotted list, such as "Number of cells whose Status is Alive", or its name for
/// a measure no form of the pane makes.
fn measure_words(document: &SheetDocument, measure: &SheetMeasure) -> String {
    let property = |id: &str| find_property(document, id);
    match measure.rule {
        MeasureRule::CountInState => {
            let found = measure.property.as_deref().and_then(property);
            let state = found
                .zip(measure.state.as_deref())
                .and_then(|(found, state)| found.states.iter().find(|known| known.id == state));
            match (found, state) {
                (Some(found), Some(state)) if measure.over.as_deref() == Some(CELLS) => {
                    count_measure(&found.name, &state.name)
                }
                (Some(found), Some(state)) if measure.over.as_deref() != Some(GROUND) => {
                    count_agents_measure(&found.name, &state.name)
                }
                _ => measure.name.clone(),
            }
        }
        MeasureRule::Average => match measure.value.value() {
            Some(SheetValue::Chip(SheetChip::My(id))) => {
                property(id).map_or_else(|| measure.name.clone(), |found| average_measure(&found.name))
            }
            _ => measure.name.clone(),
        },
        _ => measure.name.clone(),
    }
}

/// Draws the Measures section: a Plot box beside each state, the table of measures plotted with each measure's colour
/// and delete button, and the measure form or "+ Add measure".
fn measures(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Option<Pending>) {
    let states: Vec<&SheetProperty> = properties_of(document, measured_thing(document))
        .iter()
        .filter(|property| property.property_type == PropertyType::States)
        .collect();
    let labels: Vec<&str> = states
        .iter()
        .flat_map(|property| property.states.iter().map(|state| state.name.as_str()))
        .collect();
    let swatch_room = STATE_SWATCH_WIDTH;
    let form = Form::new(ui, &labels).wider(swatch_room);
    for property in states {
        text_cell(ui, Label::new(RichText::new(&property.name).weak()));
        // One row per state. egui wraps no nested group, so a row of groups would run past a narrow pane.
        for state in &property.states {
            form.row_with(
                ui,
                |ui| {
                    ui.spacing_mut().item_spacing.x = swatch_room - super::widgets::CHIP_SWATCH;
                    swatch_ui(ui, color32(state.color));
                    ui.add(Label::new(&state.name).truncate());
                },
                |ui| {
                    let plotted = counts_of(document, &property.id, &state.id);
                    let mut on = !plotted.is_empty();
                    let response = ui.checkbox(&mut on, PLOT);
                    set_name(ui, response.id, &plot_state(&state_label(document, property, state)));
                    if response.changed() {
                        *pending = Some(if on {
                            Pending::one(add_count(document, property, state))
                        } else {
                            let items: Vec<SheetItem> = plotted
                                .iter()
                                .map(|measure| SheetItem::Measure(measure.id.clone()))
                                .collect();
                            Pending {
                                starts_only: false,
                                form: None,
                                run: Box::new(move |builder| {
                                    items
                                        .into_iter()
                                        .try_for_each(|item| builder.apply(SheetEdit::Delete { item }).map(drop))
                                }),
                            }
                        });
                    }
                },
            );
        }
        ui.add_space(4.0);
    }
    if !document.measures.is_empty() {
        let columns = vec![
            Column::new(PLOTTED, 18.0, 18.0),
            Column::new("", 160.0, 80.0),
            Column::fixed(DELETE_WIDTH),
        ];
        let id = ui.make_persistent_id("henad_sheet_plotted");
        Table::new(id, columns, 1, &[(1, 0.0)]).show(ui, |ui, rows| {
            for measure in &document.measures {
                rows.row(ui, &measure.id, |row| {
                    row.cell(0, |ui| {
                        line(ui, |ui| {
                            let name = measure_color_name(&measure.name, color_name(measure.color));
                            if let Some(color) = color_button(ui, &name, measure.color) {
                                *pending = Some(Pending::one(SheetEdit::Recolor {
                                    item: SheetItem::Measure(measure.id.clone()),
                                    color,
                                }));
                            }
                        });
                    });
                    row.cell(1, |ui| {
                        text_cell(ui, Label::new(measure_words(document, measure)));
                    });
                    row.cell(2, |ui| {
                        line(ui, |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if delete_button(ui, &remove_measure_name(&measure.name)).clicked() {
                                    *pending = Some(Pending::one(SheetEdit::Delete {
                                        item: SheetItem::Measure(measure.id.clone()),
                                    }));
                                }
                            });
                        });
                    });
                });
            }
        });
    }
    ui.add_space(6.0);
    if let Some(form) = &mut pane.measure_form {
        let mut close = false;
        measure_form(ui, document, form, &mut close, pending);
        if close {
            pane.measure_form = None;
        }
    } else if super::widgets::add_button(ui, ADD_MEASURE, ADD_MEASURE, None, false).clicked() {
        pane.measure_form = Some(MeasureForm::add(document));
    }
}

/// Adds a select named `name` of `properties` showing the one of id `current`, or `empty` while none is chosen, and
/// returns the id of the property chosen.
fn property_select(
    ui: &mut Ui,
    id_salt: &str,
    name: &str,
    properties: &[&SheetProperty],
    current: Option<&str>,
    empty: &str,
) -> Option<String> {
    let shown = current
        .and_then(|id| properties.iter().find(|property| property.id == id))
        .map_or(empty, |property| property.name.as_str());
    let items: Vec<OverlayItem<'_>> = properties
        .iter()
        .map(|property| OverlayItem {
            label: &property.name,
            current: current == Some(property.id.as_str()),
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = select(ui, id_salt, name, shown, &items);
    let chosen = chosen.and_then(|index| properties.get(index))?;
    (current != Some(chosen.id.as_str())).then(|| chosen.id.clone())
}

/// Draws the measure form, and sets `close` on Cancel. Done checks the form and adds the measure.
fn measure_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut MeasureForm,
    close: &mut bool,
    pending: &mut Option<Pending>,
) {
    let properties = properties_of(document, measured_thing(document));
    let agents = measured_thing(document) != CELLS;
    let of_kind = |kinds: &[PropertyType]| -> Vec<&SheetProperty> {
        properties
            .iter()
            .filter(|property| kinds.contains(&property.property_type))
            .collect()
    };
    let states = of_kind(&[PropertyType::States]);
    // An average of agents' positions is their centre.
    let numbers = of_kind(&[PropertyType::Number, PropertyType::WholeNumber, PropertyType::Position]);
    form_heading(ui, NEW_MEASURE);
    let average = form.average;
    line(ui, |ui| {
        ui.radio_value(
            &mut form.average,
            false,
            if agents { COUNT_AGENTS_MEASURE } else { COUNT_MEASURE },
        );
    });
    line(ui, |ui| {
        ui.radio_value(&mut form.average, true, AVERAGE_MEASURE);
    });
    if form.average != average {
        *form = MeasureForm {
            average: form.average,
            ..MeasureForm::default()
        };
    }
    wrapped_line(ui, |ui| {
        if form.average {
            ui.label(AVERAGE_MEASURE_LEAD);
            if let Some(chosen) = property_select(
                ui,
                "henad_sheet_measure_number",
                MEASURE_PROPERTY_NAME,
                &numbers,
                form.property.as_deref(),
                MEASURE_NUMBER_PROPERTY_EMPTY,
            ) {
                form.property = Some(chosen);
            }
        } else {
            ui.label(if agents { COUNT_AGENTS_LEAD } else { COUNT_MEASURE_LEAD });
            if let Some(chosen) = property_select(
                ui,
                "henad_sheet_measure_property",
                MEASURE_PROPERTY_NAME,
                &states,
                form.property.as_deref(),
                MEASURE_PROPERTY_EMPTY,
            ) {
                form.property = Some(chosen);
                form.state = None;
            }
            ui.label(COUNT_MEASURE_IS);
            let property = form
                .property
                .as_deref()
                .and_then(|id| states.iter().find(|property| property.id == id));
            let shown = property
                .zip(form.state.as_deref())
                .and_then(|(property, state)| state_name(property, state))
                .unwrap_or(MEASURE_STATE_EMPTY);
            let offered = property.map(|property| property.states.as_slice()).unwrap_or_default();
            let items: Vec<OverlayItem<'_>> = offered
                .iter()
                .map(|state| OverlayItem {
                    label: &state.name,
                    current: form.state.as_deref() == Some(state.id.as_str()),
                    swatch: Some(color32(state.color)),
                    ..OverlayItem::default()
                })
                .collect();
            let (_, chosen) = select(ui, "henad_sheet_measure_state", MEASURE_STATE_NAME, shown, &items);
            if let Some(state) = chosen.and_then(|index| offered.get(index)) {
                form.state = Some(state.id.clone());
            }
        }
    });
    if let Some(message) = &form.message {
        message_ui(ui, message);
    }
    form_buttons(ui, |ui| {
        if ui.add(filled_button(DONE.to_owned())).clicked() {
            match measure_edit(document, form) {
                Ok(edit) => {
                    *pending = Some(Pending::from_form(
                        FormKind::Measure,
                        Box::new(move |builder| builder.apply(edit).map(drop)),
                    ));
                }
                Err(message) => form.message = Some(message),
            }
        }
        if ui.button(CANCEL).clicked() {
            *close = true;
        }
    });
}

/// Returns the command Done of the measure form makes, or the message for the first check that fails.
///
/// # Errors
///
/// Returns the message on a count with no property or no state, an average with no number property, or a measure
/// already plotted.
fn measure_edit(document: &SheetDocument, form: &MeasureForm) -> Result<SheetEdit, String> {
    let property = form.property.as_deref().and_then(|id| {
        properties_of(document, measured_thing(document))
            .iter()
            .find(|known| known.id == id)
    });
    if form.average {
        let Some(property) = property else {
            return Err(MEASURE_NEEDS_NUMBER_PROPERTY.to_owned());
        };
        if !averages_of(document, &property.id).is_empty() {
            return Err(MEASURE_ALREADY_PLOTTED.to_owned());
        }
        return Ok(SheetEdit::AddMeasure {
            name: average_measure(&property.name),
            rule: MeasureRule::Average,
            over: Some(measured_thing(document).to_owned()),
            property: None,
            state: None,
            event: None,
            value: Slot::Filled(SheetValue::Chip(SheetChip::My(property.id.clone()))),
            color: None,
        });
    }
    let state = property.zip(form.state.as_deref()).and_then(|(property, state)| {
        property
            .states
            .iter()
            .find(|known| known.id == state)
            .map(|state| (property, state))
    });
    let Some((property, state)) = state else {
        return Err(MEASURE_NEEDS_STATE.to_owned());
    };
    if !counts_of(document, &property.id, &state.id).is_empty() {
        return Err(MEASURE_ALREADY_PLOTTED.to_owned());
    }
    Ok(add_count(document, property, state))
}

/// Returns the colour a number shaded in `hue` takes at `share` of the way from its darkest value to its lightest.
fn shade_color(hue: ShadeHue, share: f32) -> egui::Color32 {
    let [dark, light] = hue.ends();
    let channel = |index: usize| {
        let (low, high) = (f32::from(dark[index]), f32::from(light[index]));
        // The channels lie in 0 to 255, and so does every value between two of them.
        (low + (high - low) * share).round() as u8
    };
    egui::Color32::from_rgb(channel(0), channel(1), channel(2))
}

/// Returns the rule that shades the cells by `property`, keeping the hue and the range of `previous` when it shades
/// too, and blue from 0 to 1 otherwise.
fn shaded_rule(property: &str, previous: Option<&ColorRule>) -> ColorRule {
    let mut rule = ColorRule::new(ColorRuleKind::Shaded);
    rule.property = Some(property.to_owned());
    let previous = previous.filter(|previous| previous.rule == ColorRuleKind::Shaded);
    rule.hue = Some(previous.and_then(|previous| previous.hue).unwrap_or(ShadeHue::Blue));
    rule.darkest = Some(previous.and_then(|previous| previous.darkest).unwrap_or(0.0));
    rule.lightest = Some(previous.and_then(|previous| previous.lightest).unwrap_or(1.0));
    rule
}

/// Returns the command that colours `thing` by `property`: by its states' colours for a States property, by heading
/// for a Heading, and shaded as [`shaded_rule`] says otherwise.
fn color_rule_edit(thing: &str, property: &SheetProperty, previous: Option<&ColorRule>) -> SheetEdit {
    let rule = match property.property_type {
        PropertyType::States => {
            let mut rule = ColorRule::new(ColorRuleKind::ByState);
            rule.property = Some(property.id.clone());
            rule
        }
        PropertyType::Heading => ColorRule::new(ColorRuleKind::ByHeading),
        _ => shaded_rule(&property.id, previous),
    };
    SheetEdit::SetColorRule {
        thing: thing.to_owned(),
        rule: Some(rule),
    }
}

/// Draws the Colours section: the property that colours the cells, or in a world of agents the property that colours
/// the agents and the one that colours the ground. `widgets` is the id the pane's widgets are keyed under.
fn colors(ui: &mut Ui, document: &SheetDocument, widgets: Id, pending: &mut Option<Pending>) {
    let form = Form::new(ui, &[COLOR_CELLS_BY, COLOR_AGENTS_BY, COLOR_GROUND_BY, SHADE]);
    if document.world.made_of != MadeOf::Agents {
        colors_of(ui, document, CELLS, COLOR_CELLS_BY, &form, widgets, pending);
        return;
    }
    colors_of(
        ui,
        document,
        measured_thing(document),
        COLOR_AGENTS_BY,
        &form,
        widgets,
        pending,
    );
    if !properties_of(document, GROUND).is_empty() {
        ui.add_space(4.0);
        colors_of(ui, document, GROUND, COLOR_GROUND_BY, &form, widgets, pending);
    }
}

/// Draws what colours `thing` as rows of `form`: `label` and a select of its properties, with the states' colours under
/// a States property, the note on a heading, and the hue and the range of the shading of a Number property.
///
/// With no rule, or a rule over no property of `thing`, the select shows the first States property, or else the first
/// property that can colour.
fn colors_of(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    label: &str,
    form: &Form,
    widgets: Id,
    pending: &mut Option<Pending>,
) {
    let properties: Vec<&SheetProperty> = properties_of(document, thing)
        .iter()
        .filter(|property| {
            matches!(
                property.property_type,
                PropertyType::States | PropertyType::Number | PropertyType::WholeNumber | PropertyType::Heading
            )
        })
        .collect();
    let rule = document.colors.get(thing);
    let ruled = rule.and_then(|rule| {
        if rule.rule == ColorRuleKind::ByHeading {
            return properties
                .iter()
                .find(|property| property.property_type == PropertyType::Heading);
        }
        let id = rule.property.as_deref()?;
        properties.iter().find(|property| property.id == id)
    });
    let chosen = ruled
        .or_else(|| {
            properties
                .iter()
                .find(|property| property.property_type == PropertyType::States)
        })
        .or(properties.first());
    let Some(&chosen) = chosen else {
        return;
    };
    form.row(ui, label, |ui| {
        let picked = property_select(
            ui,
            &format!("henad_sheet_color_by_{thing}"),
            label,
            &properties,
            ruled.map(|property| property.id.as_str()),
            &chosen.name,
        );
        if let Some(picked) = picked
            && let Some(property) = properties.iter().find(|property| property.id == picked)
        {
            *pending = Some(Pending::one(color_rule_edit(thing, property, rule)));
        }
    });
    match chosen.property_type {
        PropertyType::States => {
            form.under(ui, |ui| {
                wrapped_line(ui, |ui| {
                    // A state's swatch and name stay on one line. egui wraps no nested group, and the pair moves to
                    // the next line by hand.
                    let mut first = true;
                    for state in &chosen.states {
                        let width = STATE_SWATCH_WIDTH + text_width(ui, &state.name);
                        if !first && width > ui.available_size_before_wrap().x {
                            ui.end_row();
                        }
                        first = false;
                        swatch_ui(ui, color32(state.color));
                        ui.weak(&state.name);
                        ui.add_space(ITEM_GAP);
                    }
                });
            });
        }
        PropertyType::Heading => {
            form.under(ui, |ui| ui.weak(BY_HEADING_NOTE));
        }
        _ => {
            let shading = shaded_rule(
                &chosen.id,
                rule.filter(|rule| rule.property.as_deref() == Some(&chosen.id)),
            );
            shading_ui(ui, thing, &shading, form, widgets, pending);
        }
    }
}

/// Draws the hue and the range of `shading`, the rule that shades `thing` by a Number property, as rows of `form`. A
/// new hue or end commits the rule with it. `widgets` is the id the pane's widgets are keyed under.
fn shading_ui(ui: &mut Ui, thing: &str, shading: &ColorRule, form: &Form, widgets: Id, pending: &mut Option<Pending>) {
    let hue = shading.hue.unwrap_or(ShadeHue::Blue);
    let hue_words = |hue: ShadeHue| {
        let index = ShadeHue::ALL.iter().position(|known| *known == hue).unwrap_or_default();
        SHADE_COLORS.get(index).copied().unwrap_or_default()
    };
    form.row(ui, SHADE, |ui| {
        let items: Vec<OverlayItem<'_>> = ShadeHue::ALL
            .iter()
            .map(|known| OverlayItem {
                label: hue_words(*known),
                current: *known == hue,
                ..OverlayItem::default()
            })
            .collect();
        let (_, chosen) = select(ui, ("henad_sheet_shade", thing), SHADE, hue_words(hue), &items);
        if let Some(&picked) = chosen.and_then(|index| ShadeHue::ALL.get(index))
            && picked != hue
        {
            let mut rule = shading.clone();
            rule.hue = Some(picked);
            *pending = Some(Pending::one(SheetEdit::SetColorRule {
                thing: thing.to_owned(),
                rule: Some(rule),
            }));
        }
        let (rect, _) = ui.allocate_exact_size(vec2(GRADIENT_WIDTH, SWATCH_SIDE), Sense::hover());
        let band = rect.width() / GRADIENT_BANDS as f32;
        for index in 0..GRADIENT_BANDS {
            let left = rect.left() + band * index as f32;
            let share = index as f32 / (GRADIENT_BANDS - 1) as f32;
            ui.painter().rect_filled(
                Rect::from_min_size(egui::pos2(left, rect.top()), vec2(band.ceil(), rect.height())),
                CornerRadius::ZERO,
                shade_color(hue, share),
            );
        }
    });
    let darkest_id = widgets.with(("henad_sheet_shade_darkest", thing));
    let lightest_id = widgets.with(("henad_sheet_shade_lightest", thing));
    // The ends left out read as 0 and 1, as the lowering reads them.
    let (darkest_at, lightest_at) = (shading.darkest.unwrap_or(0.0), shading.lightest.unwrap_or(1.0));
    form.under(ui, |ui| {
        wrapped_line(ui, |ui| {
            ui.label(SHADE_FROM);
            for (id, name, current, other, darkest) in [
                (darkest_id, DARKEST_NAME, darkest_at, lightest_at, true),
                (lightest_id, LIGHTEST_NAME, lightest_at, darkest_at, false),
            ] {
                commit_number(
                    ui,
                    id,
                    name,
                    &format_number(current).unwrap_or_default(),
                    FIELD_WIDTH,
                    |text| match typed_number(text) {
                        // The lowering refuses two equal ends, so the field refuses them first.
                        Ok(value) if value == other => Some(SHADE_ENDS_EQUAL.to_owned()),
                        Ok(value) => {
                            let mut rule = shading.clone();
                            if darkest {
                                rule.darkest = Some(value);
                            } else {
                                rule.lightest = Some(value);
                            }
                            *pending = Some(Pending::one(SheetEdit::SetColorRule {
                                thing: thing.to_owned(),
                                rule: Some(rule),
                            }));
                            None
                        }
                        Err(message) => Some(message),
                    },
                );
                ui.label(if darkest { SHADE_DARK_TO } else { SHADE_LIGHT });
            }
        });
        if let Some(message) = field_message(ui, darkest_id).or_else(|| field_message(ui, lightest_id)) {
            message_ui(ui, &message);
        }
    });
}

/// Returns what `button` does in words, such as "Start again: Status", "Start again: every property", "Run once,
/// every cell" or "Set To food and To home to 0 everywhere".
///
/// Steps one after another that set properties to the same value everywhere read as one.
fn button_does(document: &SheetDocument, button: &SheetButton) -> String {
    let sentences = Sentences::new(document);
    let name_of = |id: &str| find_property(document, id).map_or_else(|| id.to_owned(), |known| known.name.clone());
    let mut does: Vec<String> = Vec::new();
    let mut set: Option<(Vec<String>, String)> = None;
    for step in &button.does {
        if step.rule == ButtonStepRule::SetEverywhere {
            let property = step.property.as_deref().unwrap_or_default();
            let value = sentences.slot(&step.value, Some(property));
            match &mut set {
                Some((properties, held)) if *held == value => properties.push(name_of(property)),
                _ => {
                    if let Some((properties, value)) = set.take() {
                        does.push(set_everywhere(&properties, &value));
                    }
                    set = Some((vec![name_of(property)], value));
                }
            }
            continue;
        }
        if let Some((properties, value)) = set.take() {
            does.push(set_everywhere(&properties, &value));
        }
        does.push(if step.rule == ButtonStepRule::StartAgain {
            match step.properties.as_slice() {
                [] => start_again_for(EVERY_PROPERTY),
                properties => {
                    let names: Vec<String> = properties.iter().map(|id| name_of(id)).collect();
                    start_again_for(&names.join(", "))
                }
            }
        } else {
            let thing = step
                .card
                .as_deref()
                .and_then(|card| document.cards.iter().find(|known| known.id == card))
                .map_or(CELLS, |card| card.every.as_str());
            run_once(&sentences.thing_word(thing))
        });
    }
    if let Some((properties, value)) = set {
        does.push(set_everywhere(&properties, &value));
    }
    does.join(", ")
}

/// Draws the Buttons section: the table of buttons, each with what it does and its Edit and delete buttons, the button
/// form under the button it changes, and the card of a button that runs once. "+ Add button" opens the form for a new
/// button.
///
/// The columns give up width in the order Does, Name, and Does takes the room left over.
fn buttons(
    ui: &mut Ui,
    document: &SheetDocument,
    report: &SheetReport,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let sentences = Sentences::new(document);
    if !document.buttons.is_empty() {
        let names = document
            .buttons
            .iter()
            .map(|button| text_width(ui, &button.name))
            .fold(caption_width(ui, NAME_LABEL), f32::max);
        let edit = text_width(ui, EDIT) + 6.0;
        let columns = vec![
            Column::new(NAME_LABEL, names.min(140.0), 56.0),
            Column::new(BUTTON_DOES, 160.0, 80.0),
            Column::fixed(edit),
            Column::fixed(DELETE_WIDTH),
        ];
        let id = ui.make_persistent_id("henad_sheet_buttons");
        Table::new(id, columns, 1, &[(1, 0.0), (0, 0.0)]).show(ui, |ui, rows| {
            for button in &document.buttons {
                rows.row(ui, &button.id, |row| {
                    row.cell(0, |ui| {
                        text_cell(ui, Label::new(RichText::new(&button.name).strong()));
                    });
                    row.cell(1, |ui| {
                        text_cell(ui, Label::new(RichText::new(button_does(document, button)).weak()));
                    });
                    row.cell(2, |ui| {
                        line(ui, |ui| {
                            let edit = text_control(ui, None, EDIT, &edit_button_name(&button.name), CHIP_HEIGHT);
                            if edit.clicked() {
                                pane.button_form = Some(ButtonForm::edit(button));
                            }
                        });
                    });
                    row.cell(3, |ui| {
                        line(ui, |ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if delete_button(ui, &delete_button_name(&button.name)).clicked() {
                                    *pending = Some(Pending::one(SheetEdit::Delete {
                                        item: SheetItem::Button(button.id.clone()),
                                    }));
                                }
                            });
                        });
                    });
                });
                if let Some(form) = pane
                    .button_form
                    .as_mut()
                    .filter(|form| form.editing.as_deref() == Some(button.id.as_str()))
                {
                    let mut close = false;
                    button_form(ui, document, form, &mut close, pending);
                    if close {
                        pane.button_form = None;
                    }
                    ui.add_space(6.0);
                    hairline(ui);
                }
                for card in button.does.iter().filter_map(|step| step.card.as_deref()) {
                    ui.add_space(6.0);
                    run_once_card(ui, document, &sentences, report, card);
                    ui.add_space(6.0);
                    hairline(ui);
                }
            }
        });
        ui.add_space(6.0);
    }
    match &mut pane.button_form {
        Some(form) if form.editing.is_none() => {
            let mut close = false;
            button_form(ui, document, form, &mut close, pending);
            if close {
                pane.button_form = None;
            }
        }
        _ => {
            if super::widgets::add_button(ui, ADD_BUTTON, ADD_BUTTON, None, false).clicked() {
                pane.button_form = Some(ButtonForm::add());
            }
        }
    }
}

/// Draws the card `card` that a button runs once, read-only, as a region inside the Buttons section: its heading, then
/// each line's number and sentence, or the text of a card with no line.
fn run_once_card(ui: &mut Ui, document: &SheetDocument, sentences: &Sentences<'_>, report: &SheetReport, card: &str) {
    let Some(index) = document.cards.iter().position(|known| known.id == card) else {
        return;
    };
    region(ui, |ui| {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                if let Some(card) = document.cards.get(index) {
                    ui.label(RichText::new(sentences.card_heading(card)).small());
                }
                let lines = card_lines(document, sentences, report, index);
                if lines.is_empty() {
                    ui.weak(BUTTON_CARD_EMPTY);
                }
                for line in lines {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add_space(20.0 * line.depth as f32);
                        ui.label(small_label(line.number.to_string()));
                        ui.vertical(|ui| {
                            ui.set_max_width(ui.available_width());
                            ui.add(Label::new(&line.sentence).wrap());
                        });
                    });
                }
            });
    });
}

/// Draws the button form, and sets `close` on Cancel. Done checks the form and adds or changes the button.
fn button_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut ButtonForm,
    close: &mut bool,
    pending: &mut Option<Pending>,
) {
    let thing = measured_thing(document);
    // Agents' buttons start the ground's properties again too.
    let properties: Vec<&SheetProperty> = properties_of(document, thing)
        .iter()
        .chain(if thing == CELLS {
            &[][..]
        } else {
            properties_of(document, GROUND)
        })
        .filter(|property| property.starts_as.is_some())
        .collect();
    let thing_word = Sentences::new(document).thing_word(thing);
    let heading = match &form.editing {
        Some(_) => edit_button_name(&form.original),
        None => NEW_BUTTON.to_owned(),
    };
    form_heading(ui, &heading);
    let rows = Form::new(ui, &[NAME_LABEL, BUTTON_DOES]);
    let mut done = false;
    rows.row(ui, NAME_LABEL, |ui| {
        let field = ui.add(
            TextEdit::singleline(&mut form.name)
                .hint_text(BUTTON_NAME_PLACEHOLDER)
                .desired_width(ui.available_width().min(STATE_FIELD_WIDTH + 60.0)),
        );
        set_name(ui, field.id, BUTTON_NAME_FIELD);
        if std::mem::take(&mut form.focus_name) {
            field.request_focus();
        }
        done |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    });
    rows.row(ui, BUTTON_DOES, |ui| {
        ui.radio_value(&mut form.run_once, Some(false), START_AGAIN);
        ui.add_enabled_ui(form.run_once == Some(false), |ui| {
            let shown = form
                .property
                .as_deref()
                .and_then(|id| properties.iter().find(|property| property.id == id))
                .map_or(EVERY_PROPERTY, |property| property.name.as_str());
            let choices: Vec<Option<&SheetProperty>> = std::iter::once(None)
                .chain(properties.iter().copied().map(Some))
                .collect();
            let items: Vec<OverlayItem<'_>> = choices
                .iter()
                .map(|choice| OverlayItem {
                    label: choice.map_or(EVERY_PROPERTY, |property| property.name.as_str()),
                    current: form.property.as_deref() == choice.map(|property| property.id.as_str()),
                    ..OverlayItem::default()
                })
                .collect();
            let (_, chosen) = select(ui, "henad_sheet_button_property", START_AGAIN_FOR, shown, &items);
            if let Some(choice) = chosen.and_then(|index| choices.get(index)) {
                form.property = choice.map(|property| property.id.clone());
            }
        });
    });
    rows.under(ui, |ui| {
        ui.radio_value(&mut form.run_once, Some(true), run_once(&thing_word));
        // The steps that set a property everywhere, which no control of the form changes and Done keeps.
        if let Some(sets) = set_steps(document, form.editing.as_deref()) {
            ui.weak(button_does(document, &sets));
        }
        if let Some(message) = &form.message {
            message_ui(ui, message);
        }
    });
    form_buttons(ui, |ui| {
        done |= ui.add(filled_button(DONE.to_owned())).clicked();
        if ui.button(CANCEL).clicked() {
            *close = true;
        }
    });
    if done {
        match check_button_form(document, form) {
            Ok(()) => {
                *pending = Some(Pending::from_form(
                    FormKind::Button,
                    button_edit(form.clone(), thing.to_owned()),
                ));
            }
            Err(message) => form.message = Some(message),
        }
    }
}

/// Returns the message for the first check of the button form that fails.
///
/// # Errors
///
/// Returns the message on an empty name, a name another button holds, or the name of a new button that Henad keeps for
/// itself.
fn check_button_form(document: &SheetDocument, form: &ButtonForm) -> Result<(), String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(BUTTON_NAME_MISSING.to_owned());
    }
    let taken = document
        .buttons
        .iter()
        .filter(|button| Some(&button.id) != form.editing.as_ref())
        .any(|button| button.name.to_lowercase() == name.to_lowercase());
    if taken {
        return Err(button_name_taken(name));
    }
    if form.editing.is_none() && is_reserved_name(name, IdSpace::Button) {
        return Err(name_reserved(name));
    }
    Ok(())
}

/// Returns button `id` with its steps that set a property everywhere alone, `None` for a button with none or no
/// button.
fn set_steps(document: &SheetDocument, id: Option<&str>) -> Option<SheetButton> {
    let mut button = document
        .buttons
        .iter()
        .find(|button| Some(button.id.as_str()) == id)?
        .clone();
    button.does.retain(|step| step.rule == ButtonStepRule::SetEverywhere);
    (!button.does.is_empty()).then_some(button)
}

/// Returns the step of a button that starts `property` again, or every property of `thing` for `None`.
fn start_again_step(property: Option<String>, thing: &str) -> ButtonStep {
    let mut step = ButtonStep::new(ButtonStepRule::StartAgain);
    match property {
        Some(property) => step.properties = vec![property],
        None => step.thing = Some(thing.to_owned()),
    }
    step
}

/// Returns the edit Done of the button form makes from `form` for a button of `thing`: a new button, or the changes to
/// the button it edits.
///
/// A button whose kind changes, from Start again to Run once or back, is deleted and added again under its new name,
/// and a Run once button's card goes with it. The steps that set a property everywhere, which no control of the form
/// sets, stay in their order, and the step of the new kind takes the place of the old one. A Start again button's
/// new start-again step takes the place of the old ones. A button that only sets properties everywhere and was given no
/// kind keeps its steps and takes the new name.
fn button_edit(form: ButtonForm, thing: String) -> EditRun {
    Box::new(move |builder| {
        let name = form.name.trim().to_owned();
        let current = form.editing.as_ref().and_then(|id| {
            builder
                .document()
                .buttons
                .iter()
                .find(|button| button.id == *id)
                .cloned()
        });
        let Some(run_once) = form.run_once else {
            if let Some(current) = current.filter(|current| current.name != name) {
                builder.apply(SheetEdit::Rename {
                    item: SheetItem::Button(current.id),
                    name,
                })?;
            }
            return Ok(());
        };
        let rule = if run_once {
            ButtonStepRule::RunOnce
        } else {
            ButtonStepRule::StartAgain
        };
        let is_set = |step: &ButtonStep| step.rule == ButtonStepRule::SetEverywhere;
        // The set steps of the button before the edit, and the place of its first other step among its steps.
        let sets: Vec<ButtonStep> = current
            .iter()
            .flat_map(|button| button.does.iter().filter(|step| is_set(step)).cloned())
            .collect();
        let kind_index = current
            .as_ref()
            .and_then(|button| button.does.iter().position(|step| !is_set(step)))
            .unwrap_or_default();
        let id = match current {
            Some(current) if current.does.iter().any(|step| step.rule == rule) => {
                if current.name != name {
                    builder.apply(SheetEdit::Rename {
                        item: SheetItem::Button(current.id.clone()),
                        name,
                    })?;
                }
                current.id
            }
            current => {
                if let Some(current) = current {
                    builder.apply(SheetEdit::Delete {
                        item: SheetItem::Button(current.id),
                    })?;
                }
                builder
                    .apply(SheetEdit::AddButton {
                        name,
                        rule,
                        thing: thing.clone(),
                    })?
                    .into_iter()
                    .next()
                    .unwrap_or_default()
            }
        };
        let held: Vec<ButtonStep> = builder
            .document()
            .buttons
            .iter()
            .find(|button| button.id == id)
            .map(|button| button.does.clone())
            .unwrap_or_default();
        let mut own: Vec<ButtonStep> = held.iter().filter(|step| !is_set(step)).cloned().collect();
        if !run_once {
            let start_again_index = own
                .iter()
                .position(|step| step.rule == ButtonStepRule::StartAgain)
                .unwrap_or_default();
            own.retain(|step| step.rule != ButtonStepRule::StartAgain);
            own.insert(
                start_again_index.min(own.len()),
                start_again_step(form.property, &thing),
            );
        }
        let mut does = sets;
        for (offset, step) in own.into_iter().enumerate() {
            does.insert((kind_index + offset).min(does.len()), step);
        }
        if does == held {
            return Ok(());
        }
        builder.apply(SheetEdit::SetButtonSteps { button: id, does }).map(drop)
    })
}
