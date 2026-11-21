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
//! text it cannot read stays in the field with a red message under its row. Escape puts back the value the sheet
//! holds. A property is added or changed through
//! the *property form* under the table of its kind of thing, a measure through the *measure form* and a button through
//! the *button form*. A Starts as number is a plain field with "Use a parameter" beside it, and a chosen parameter
//! shows as its chip. A parameter's slider commits once, when it is let go.

use std::hash::Hash;

use egui::{Id, Key, Label, Response, RichText, TextEdit, Ui};
use henad_core::metadata::TimeUnit;
use henad_graph::limits::MAX_GRID_SIDE;
use henad_graph::sheet::document::{
    ButtonStepRule, CELLS_ID, Edges, GROUND_ID, MadeOf, PropertyType, SheetButton, SheetDocument, SheetLiteral,
    SheetProperty, SheetTime, Slot,
};
use henad_graph::sheet::edit::{SheetBuilder, SheetEdit, SheetEditError};
use henad_graph::sheet::format_number;
use henad_graph::sheet::rates::chance_line_numbers;
use henad_graph::sheet::sentence::{AGENT_WALLS_SENTENCE, AGENT_WRAP_TOOLTIP};
use henad_graph::sheet::sentence::{
    AGENTS_IN_OPEN_SPACE, CANCEL, CELLS_ON_A_GRID, DONE, EDGES_LABEL, HEIGHT_NAME, ONE_TICK_IS, PURPOSE_PLACEHOLDER,
    SIZE_BY, SIZE_LABEL, SheetSection, TICK_LENGTH_NAME, TICK_UNIT_NAME, TYPE_ONE_NUMBER, WALLS, WALLS_SENTENCE,
    WIDTH_NAME, WRAP_AROUND, WRAP_SENTENCE, WRAP_TOOLTIP, chances_per_tick_note, read_typed_number, tick_unit_choice,
    world_side_out_of_range,
};
use henad_graph::value::Color;

use super::help::Help;
use super::widgets::{
    NewParameterDialog, NumberUse, OverlayItem, REGION_GAP, commit_area, commit_number, field_message, message_ui,
    notice_ui, refusal_text, select_list, select_within, set_name,
};
use super::{SheetEditor, all_properties, properties_of};
use crate::init::semibold_font;
use crate::ui::sweep::layout::filled_button;
use buttons::buttons;
use colors::colors;
use layout::{Form, ITEM_GAP, line, section, text_cell, text_width};
use measures::measures;
use parameters::parameters;
#[cfg(test)]
pub use parameters::slider_step;
use property_form::default_color;
#[cfg(test)]
pub use starts::starts_text;
use starts::{cells, new_parameter, number_field, property_form_or_add, property_table, set_start_slot, start_edit};

mod agents;
mod buttons;
mod colors;
mod layout;
mod measures;
mod parameters;
mod property_form;
mod starts;

/// A value the world tries for a parameter of the sheet while the model keeps its own.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetTrial {
    /// Id of the parameter.
    pub parameter: String,
    /// Value the world tries.
    pub value: f64,
}

/// Width in points of a number field.
const FIELD_WIDTH: f32 = 56.0;

/// Width in points of a state's name field in the property form.
const STATE_FIELD_WIDTH: f32 = 140.0;

/// Widest a form's Name field grows in points.
const NAME_FIELD_WIDTH: f32 = 200.0;

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

/// An edit made during the frame. Every edit of a frame runs once the pane is drawn, in the order the pane drew their
/// controls, each as one step of Undo.
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
/// and runs every edit made in it.
///
/// A field that loses the focus to a click commits in the frame of the click, so one frame can hold two edits. Each
/// runs on the sheet as the edit before it left it.
///
/// `past_start` is whether the preview has passed tick 0 or a button was pressed in it. A changed Starts as then says
/// "Press Start again to see the change." `trials` holds the values the world tries, each shown under its parameter.
pub fn sheet_pane(ui: &mut Ui, sheet: &mut SheetEditor, past_start: bool, trials: &[SheetTrial]) {
    let stale: Vec<String> = all_properties(sheet.document())
        .filter(|property| past_start && sheet.start_changed(&property.id))
        .map(|property| property.id.clone())
        .collect();
    let mut pending = Vec::new();
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
    if pending.is_empty() {
        return;
    }
    let time = sheet.document().time;
    let mut kept = false;
    let mut restart_notice = None;
    sheet.pane.refusal = None;
    for Pending { starts_only, form, run } in pending {
        let outcome = sheet.edit(past_start, starts_only, run);
        restart_notice = restart_notice.or(sheet.notice);
        let pane = &mut sheet.pane;
        match outcome {
            Ok(()) => {
                kept = true;
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
    // An edit of starts alone, made after an edit that restarted the world in the same frame, leaves no notice of its
    // own. The world still starts again from tick 0.
    if sheet.notice.is_none() && !sheet.has_errors() && !sheet.shows_nothing() {
        sheet.notice = restart_notice;
    }
    let time_changed = sheet.document().time.is_some() && sheet.document().time != time;
    if time_changed {
        // The notice of a time a rate's unit set no longer states the length of one tick.
        sheet.card.drop_tick_notice();
    }
    if kept {
        sheet.pane.time_changed = time_changed;
    }
}

/// Draws what the model is for as a field of several lines that commits on losing focus. `widgets` is the id the
/// pane's widgets are keyed under.
fn purpose(ui: &mut Ui, document: &SheetDocument, widgets: Id, pending: &mut Vec<Pending>) {
    let current = document.sheet.purpose.as_deref().unwrap_or_default();
    commit_area(
        ui,
        widgets.with("henad_sheet_purpose"),
        SheetSection::Purpose.heading(),
        current,
        PURPOSE_PLACEHOLDER,
        |text| {
            let purpose = text.trim();
            pending.push(Pending::one(SheetEdit::SetPurpose {
                purpose: (!purpose.is_empty()).then(|| purpose.to_owned()),
            }));
            None
        },
    );
}

/// Draws the world's kind, then its size, its edges and the length of one tick as form rows, and the notice of `pane`
/// under them.
fn world(ui: &mut Ui, document: &SheetDocument, pane: &PaneState, pending: &mut Vec<Pending>) {
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
            pending.push(Pending::one(SheetEdit::SetWorld {
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
    pending: &mut Vec<Pending>,
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
        pending.push(Pending::one(edit(side)));
        None
    });
}

/// Draws the Time line of `form`, "One tick is [number] [unit]", with the number hidden while the tick has no unit.
fn time_line(ui: &mut Ui, document: &SheetDocument, form: &Form, widgets: Id, pending: &mut Vec<Pending>) {
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
                pending.push(Pending::one(SheetEdit::SetTime {
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
            pending.push(Pending::one(SheetEdit::SetTime {
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

/// Draws the row of a form's buttons at the form's left edge, after a little space: Done, Cancel, then what `add`
/// adds. Returns whether Done and whether Cancel were pressed.
fn form_buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> (bool, bool) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = ITEM_GAP * 2.0;
        let done = ui.add(filled_button(DONE.to_owned())).clicked();
        let cancel = ui.button(CANCEL).clicked();
        add(ui);
        (done, cancel)
    })
    .inner
}

/// Draws the Name field of a form over `text`, as wide as [`NAME_FIELD_WIDTH`] or the room left, showing `hint` while
/// empty and named `name`. It takes the focus once when `focus` holds, and clears `focus`.
///
/// Returns whether Enter left the field.
fn form_name_field(ui: &mut Ui, text: &mut String, hint: &str, name: &str, focus: &mut bool) -> bool {
    let field = ui.add(
        TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(ui.available_width().min(NAME_FIELD_WIDTH)),
    );
    set_name(ui, field.id, name);
    if std::mem::take(focus) {
        field.request_focus();
    }
    field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
}

/// Returns the kind of thing the Measures and Buttons sections act on: the cells of a grid, or the first kind of agent.
fn measured_thing(document: &SheetDocument) -> &str {
    match document.world.made_of {
        MadeOf::Agents => document.agents.first().map_or(CELLS_ID, |kind| kind.id.as_str()),
        _ => CELLS_ID,
    }
}
