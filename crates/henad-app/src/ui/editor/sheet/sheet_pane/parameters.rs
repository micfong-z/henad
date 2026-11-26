//! The Parameters section of the Sheet pane: a row per parameter with its name, value slider, range and unit, and the
//! slider step and range rules the fields follow.

use egui::accesskit::Action;
use egui::{Id, Key, Label, RichText, Slider, Ui};
use henad_graph::sheet::document::{ParameterType, SheetDocument, SheetLiteral, SheetParameter};
use henad_graph::sheet::edit::{SheetEdit, SheetItem};
use henad_graph::sheet::format_number;
use henad_graph::sheet::sentence::{
    ADD_PARAMETER, DELETE_PARAMETER_HEADING, MAX_LABEL, MIN_LABEL, PARAMETER_COLUMNS, PARAMETER_NAME_MISSING,
    TYPE_ONE_NUMBER, UNIT_FIELD_PLACEHOLDER, UNIT_LABEL, delete_parameter_name, parameter_max_name, parameter_min_name,
    parameter_name_taken, parameter_unit_name, read_typed_number, sheet_parameter_name_field, sheet_trying_value,
    unit_suggestions, value_field_name, value_slider_name,
};

use super::layout::{Column, ITEM_GAP, Table, line, text_width, wrapped_line};
use super::{
    DELETE_WIDTH, FIELD_WIDTH, PARAMETER_NAME_WIDTH, PaneState, Pending, SLIDER_WIDTH, SheetTrial, UNIT_WIDTH,
    delete_or_ask,
};
use crate::ui::editor::sheet::set_parameter_edit;
use crate::ui::editor::sheet::widgets::{
    NewParameterDialog, NumberUse, TypeSymbol, caption, caption_width, commit_field, commit_number, message_ui,
    name_taken, set_name, suggestion_field, type_symbol_ui, type_symbol_width,
};
use crate::ui::params::snapped;

/// Returns the step of the value slider of `parameter`: [`slider_step`] of its range, and at least 1 for a whole number.
pub fn parameter_step(parameter: &SheetParameter) -> f64 {
    let step = slider_step(parameter.max - parameter.min);
    if parameter.parameter_type == ParameterType::WholeNumber {
        step.max(1.0).round()
    } else {
        step
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
pub(super) fn typed_number(text: &str) -> Result<f64, String> {
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
pub(super) fn parameters(
    ui: &mut Ui,
    document: &SheetDocument,
    pane: &mut PaneState,
    trials: &[SheetTrial],
    pending: &mut Vec<Pending>,
) {
    if !document.parameters.is_empty() {
        let names = document
            .parameters
            .iter()
            .map(|parameter| text_width(ui, &parameter.name) + 12.0)
            .fold(0.0, f32::max)
            .min(PARAMETER_NAME_WIDTH + 20.0)
            + type_symbol_width(ui.ctx());
        let value = Column::new(
            PARAMETER_COLUMNS[1],
            SLIDER_WIDTH + ITEM_GAP + FIELD_WIDTH,
            36.0 + ITEM_GAP + 48.0,
        );
        let name = Column::new(PARAMETER_COLUMNS[0], names, 64.0);
        let one_line = names + 120.0 + 2.0 * RANGE_WIDTH + UNIT_WIDTH + DELETE_WIDTH + 5.0 * super::layout::COLUMN_GAP;
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
    if crate::ui::editor::sheet::widgets::add_row_button(ui, ADD_PARAMETER, ADD_PARAMETER, None, false).clicked() {
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

    /// Returns the edit that changes the parameter's range, value and unit by `change`, applied to the parameter as the
    /// sheet holds it when the edit runs. An earlier edit of the same frame, such as a new Max, then stays.
    fn set(&self, change: impl FnOnce(&mut SheetParameter) + 'static) -> Pending {
        let drawn = self.parameter.clone();
        Pending {
            starts_only: false,
            form: None,
            run: Box::new(move |builder| {
                let mut parameter = builder
                    .document()
                    .parameters
                    .iter()
                    .find(|known| known.id == drawn.id)
                    .cloned()
                    .unwrap_or(drawn);
                change(&mut parameter);
                let (min, max, default, unit) =
                    (parameter.min, parameter.max, parameter.default, parameter.unit.clone());
                builder
                    .apply(set_parameter_edit(&parameter, min, max, default, unit))
                    .map(drop)
            }),
        }
    }

    /// Adds the name field, as wide as `ui`. An empty or taken name puts the old name back and sets `message`.
    fn name(&self, ui: &mut Ui, message: &mut Option<(String, String)>, pending: &mut Vec<Pending>) {
        let parameter = self.parameter;
        commit_field(
            ui,
            self.id("name"),
            &sheet_parameter_name_field(&parameter.name),
            &parameter.name,
            ui.available_width(),
            |text| {
                let name = text.trim();
                let others = self.document.parameters.iter().filter(|other| other.id != parameter.id);
                let taken = name_taken(name, others.map(|other| other.name.as_str()));
                // A refused name puts the old name back, with the reason under the row.
                *message = if name.is_empty() {
                    Some((parameter.id.clone(), PARAMETER_NAME_MISSING.to_owned()))
                } else if taken {
                    Some((parameter.id.clone(), parameter_name_taken(name)))
                } else {
                    pending.push(Pending::one(SheetEdit::Rename {
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
    fn value(&self, ui: &mut Ui, pending: &mut Vec<Pending>) {
        let parameter = self.parameter;
        let field = FIELD_WIDTH.min(ui.available_width() - 36.0 - ITEM_GAP).max(36.0);
        let slider = ui.available_width() - field - ITEM_GAP;
        // A range changed earlier in the frame can leave the slider's value outside it.
        let set =
            |released: f64| self.set(move |parameter| parameter.default = released.clamp(parameter.min, parameter.max));
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
                    pending.push(self.set(move |parameter| {
                        parameter.min = parameter.min.min(value);
                        parameter.max = parameter.max.max(value);
                        parameter.default = value;
                    }));
                    None
                }
                Err(message) => Some(message),
            },
        );
    }

    /// Adds the field of the end `end` of the range, `width` points wide.
    fn range(&self, ui: &mut Ui, end: RangeEnd, width: f32, pending: &mut Vec<Pending>) {
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
                    pending.push(self.set(move |parameter| {
                        (parameter.min, parameter.max, parameter.default) = with_range_end(parameter, end, typed);
                    }));
                    None
                }
                Err(message) => Some(message),
            },
        );
    }

    /// Adds the unit field, `width` points wide.
    fn unit(&self, ui: &mut Ui, width: f32, pending: &mut Vec<Pending>) {
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
                let unit = (!unit.is_empty()).then(|| unit.to_owned());
                pending.push(self.set(move |parameter| parameter.unit = unit));
                None
            },
        );
    }

    /// Adds Min, Max and Unit on wrapping lines, each field after its small label. A label stays on its field's line.
    fn range_line(&self, ui: &mut Ui, pending: &mut Vec<Pending>) {
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
    row: &mut super::layout::TableRow<'_>,
    document: &SheetDocument,
    parameter: &SheetParameter,
    wide: bool,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let fields = ParameterFields {
        document,
        parameter,
        widgets: pane.widgets,
    };
    let message = &mut pane.parameter_message;
    row.cell(0, |ui| {
        line(ui, |ui| {
            type_symbol_ui(ui, TypeSymbol::of_parameter(parameter.parameter_type));
            fields.name(ui, message, pending);
        });
    });
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
    if row.delete_cell(last, &delete_parameter_name(&parameter.name)) {
        let edit = SheetEdit::Delete {
            item: SheetItem::Parameter(parameter.id.clone()),
        };
        let asked = (DELETE_PARAMETER_HEADING, parameter.name.as_str());
        delete_or_ask(document, edit, asked, None, &mut pane.delete_question, pending);
    }
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
/// A drag holds its value until the slider is let go, and then commits it as one edit, the edit `set` returns for the
/// new value. A click or a key commits at once. A parameter with no range to slide over shows no slider.
fn value_slider(
    ui: &mut Ui,
    parameter: &SheetParameter,
    id: Id,
    width: f32,
    pending: &mut Vec<Pending>,
    set: &impl Fn(f64) -> Pending,
) -> f64 {
    let held: Option<f64> = ui.data(|data| data.get_temp(id));
    let mut value = held.unwrap_or(parameter.default);
    if parameter.min.partial_cmp(&parameter.max) != Some(std::cmp::Ordering::Less) {
        return value;
    }
    let (min, max, step) = (parameter.min, parameter.max, parameter_step(parameter));
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = width;
            let slider = Slider::from_get_set(min..=max, |edited| {
                if let Some(edited) = edited {
                    value = snapped(edited, min, max, step);
                }
                value
            });
            ui.add(slider.step_by(step).show_value(false))
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
    // drag, arrow key or assistive request edits the default, and otherwise the slider shows the stored default. The
    // keyboard focus alone edits nothing.
    let focused = response.has_focus();
    let keyed = ui.input(|input| {
        let arrow = input.key_pressed(Key::ArrowLeft) || input.key_pressed(Key::ArrowRight);
        let assisted = [Action::Increment, Action::Decrement, Action::SetValue]
            .into_iter()
            .any(|action| input.num_accesskit_action_requests(response.id, action) > 0);
        (focused && arrow) || assisted
    });
    let touched = keyed || response.clicked() || response.drag_stopped();
    let released = if response.changed() && touched {
        Some(value)
    } else {
        held
    };
    if let Some(released) = released
        && released != parameter.default
    {
        pending.push(set(released));
    }
    released.unwrap_or(parameter.default)
}
