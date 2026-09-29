//! Parameters section of the Sweep tab: a row per parameter, the editor of a row's values, and the menu of a checkbox
//! or dropdown parameter's options.
//!
//! A row's text is the source of truth, in the form `henad-cli --vary` takes. The editor and the menu only write it.

use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{Button, Checkbox, DragValue, Id, Label, Layout, Popup, PopupCloseBehavior, RichText, TextEdit, vec2};
use henad_core::explore::factor::LevelSpec;
use henad_core::explore::value::{format_value, parse_value};
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};

use crate::icons::material_design_icons::{MDI_TABLE_COLUMN, MDI_TUNE_VERTICAL};
use crate::ui::params::display_value;
use crate::ui::plural;
use crate::ui::sweep::builder::FormInput;
use crate::ui::sweep::draft::{
    DraftDesign, DraftIssue, DraftMode, DraftSite, FactorDraft, IssueKind, LevelPreview, levels_example, number_text,
    parse_levels, round_step, whole_range_text,
};
use crate::ui::sweep::layout::{FormRows, Note, column, icon_button, note_label, note_line, segmented, slot};

/// Header of the column of Parameters tab values.
pub const BASELINE_HEADER: &str = "Parameters tab";

/// Width of the values editor, in points.
const EDITOR_WIDTH: f32 = 280.0;

/// Width a row's text field leaves for the button that opens the values editor, in points.
const EDITOR_BUTTON_WIDTH: f32 = 28.0;

/// Most values a note lists before it skips to the last one.
const MAX_NOTE_VALUES: usize = 6;

/// Values a note lists before the ellipsis, once it skips to the last one.
const NOTE_HEAD_VALUES: usize = 3;

pub const LEVELS_TOOLTIP: &str = "Enter comma-separated values or min:max:step, both ends included. Use min:max \
                                  under Latin hypercube or Uniform random.";

pub const SEARCH_LEVELS_TOOLTIP: &str = "Enter a range as min:max to search, or comma-separated values to select from";

/// Kind of values a row holds: numbers, options of a checkbox or dropdown, or an action's ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelNoun {
    Value,
    Option,
    Tick,
}

impl LevelNoun {
    /// Returns the noun of the values of a parameter of `kind`.
    pub fn of(kind: &ParamKind) -> Self {
        match kind {
            ParamKind::Bool { .. } | ParamKind::Choice { .. } => Self::Option,
            ParamKind::F32 { .. } | ParamKind::U32 { .. } => Self::Value,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Value | Self::Option => "value",
            Self::Tick => "tick",
        }
    }
}

/// Returns the note of a row whose values are `preview`, as in "5 values: 0.1, 0.2, 0.3, 0.4, 0.5".
///
/// Past [`MAX_NOTE_VALUES`] values the note lists the first few and the last. In a search, listed values are picked
/// among and a range is searched whole. Elsewhere a design draws from a range.
pub fn preview_note(preview: &LevelPreview, noun: LevelNoun, search: bool) -> String {
    match preview {
        LevelPreview::Listed { count, values, last } => {
            let listed = if *count <= MAX_NOTE_VALUES {
                values.join(", ")
            } else {
                let head: Vec<&str> = values.iter().take(NOTE_HEAD_VALUES).map(String::as_str).collect();
                format!("{}, …, {last}", head.join(", "))
            };
            let count_noun = match noun {
                LevelNoun::Option => plural(*count as u64, "option"),
                LevelNoun::Value | LevelNoun::Tick => plural(*count as u64, noun.word()),
            };
            let among = if search && noun != LevelNoun::Option {
                " to select from"
            } else {
                ""
            };
            format!("{count} {count_noun}{among}: {listed}")
        }
        LevelPreview::Drawn { min, max } if search => format!("Any {} from {min} to {max}", noun.word()),
        LevelPreview::Drawn { min, max } => {
            format!("Any {} from {min} to {max}, drawn by the design", noun.word())
        }
    }
}

/// Returns the hover text of a note that skips values: up to 50 of them, and how many more there are.
pub fn preview_tooltip(preview: &LevelPreview) -> Option<String> {
    let LevelPreview::Listed { count, values, .. } = preview else {
        return None;
    };
    if *count <= MAX_NOTE_VALUES {
        return None;
    }
    let mut text = values.join(", ");
    if *count > values.len() {
        text.push_str(&format!(" and {} more", count - values.len()));
    }
    Some(text)
}

/// Returns the note of a row's text field while the field holds back the issue of its text: the form the text takes.
pub fn format_hint(noun: LevelNoun, draws_ranges: bool) -> String {
    let example = match noun {
        LevelNoun::Tick => "100, 200",
        LevelNoun::Value | LevelNoun::Option => "0.1, 0.2",
    };
    if draws_ranges {
        format!("Format: min:max or {example}")
    } else {
        format!("Format: {example} or min:max:step")
    }
}

/// Returns the note of a varied row: its issue, the form its text takes while its field holds the issue back, or a
/// preview of its values.
pub fn row_note(
    issue: Option<&DraftIssue>,
    preview: Option<&LevelPreview>,
    noun: LevelNoun,
    search: bool,
    draws_ranges: bool,
    held_back: bool,
) -> Note {
    match issue {
        Some(issue) if issue.kind == IssueKind::Invalid && held_back => Note::Weak(format_hint(noun, draws_ranges)),
        Some(issue) => Note::Issue(issue.kind, issue.message.clone()),
        None => preview.map_or(Note::Empty, |preview| Note::Weak(preview_note(preview, noun, search))),
    }
}

/// Returns the legend over the parameter rows, for a draft in `mode` with `design`.
pub fn legend(mode: DraftMode, design: DraftDesign) -> &'static str {
    match (mode, design) {
        (DraftMode::Search, _) => {
            "Enter a range as 0.1:0.5 to search all of it, or values as 0.1, 0.2 to select from. Checkbox and \
             dropdown parameters use all options by default."
        }
        (DraftMode::Sweep, DraftDesign::Table) => {
            "The design table sets the parameters it names. Other parameters use values from the Parameters tab."
        }
        (DraftMode::Sweep, DraftDesign::LatinHypercube | DraftDesign::UniformRandom) => {
            "Enter a range as 0.1:0.5 for the design to draw from, or values as 0.1, 0.2."
        }
        (DraftMode::Sweep, DraftDesign::EveryCombination | DraftDesign::Zip | DraftDesign::VaryEachAlone) => {
            "Enter values as 0.1, 0.2, or a range as 0.1:0.5:0.1 with both ends included."
        }
    }
}

/// Draws the Parameters section: its legend, and a row for each of the model's parameters.
pub fn parameters_section(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    draft: &mut crate::ui::sweep::draft::SweepDraft,
    input: &FormInput<'_>,
) {
    let schema = input.schema;
    if schema.params.is_empty() {
        ui.weak("This model has no parameters.");
        return;
    }
    ui.add(Label::new(RichText::new(legend(draft.mode, draft.design)).weak()).wrap());
    let layout = rows.layout;
    if let Some(baseline_width) = layout.baseline_width {
        ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui| {
            let weak = |text: &str| RichText::new(text).weak();
            slot(ui, layout.label_width, |ui| ui.label(weak("Parameter")));
            slot(ui, baseline_width, |ui| ui.label(weak(BASELINE_HEADER)));
            slot(ui, layout.values_width(), |ui| ui.label(weak("Values")));
        });
    }
    let table_columns = draft
        .runs_table()
        .then(|| draft.table.as_ref().map(|table| table.columns()).unwrap_or_default());
    let context = RowContext {
        search: draft.mode == DraftMode::Search,
        draws_ranges: draft.draws_ranges(),
    };
    for (index, descriptor) in schema.params.iter().enumerate() {
        let Some(factor) = draft.factors.get_mut(index) else {
            continue;
        };
        let row = ParameterRow {
            index,
            descriptor,
            panel_value: input.panel_values.get(index),
            in_table: table_columns
                .as_ref()
                .map(|columns| columns.iter().any(|column| column == descriptor.id)),
            preview: input.summary.param_previews.get(index).and_then(Option::as_ref),
            issue: input.summary.issues_at(DraftSite::Factor(index)).next(),
        };
        parameter_row(ui, rows, context, &row, factor);
    }
}

/// Mode of the draft the rows belong to.
#[derive(Debug, Clone, Copy)]
struct RowContext {
    search: bool,
    /// Whether a range with no step is drawn from, as in a sampled design or a search.
    draws_ranges: bool,
}

/// Everything a parameter row shows besides the draft's factor.
struct ParameterRow<'a> {
    index: usize,
    descriptor: &'a ParamDescriptor,
    panel_value: Option<&'a ParamValue>,
    /// Whether a design table sets the parameter, `None` for a draft that runs no design table.
    in_table: Option<bool>,
    preview: Option<&'a LevelPreview>,
    issue: Option<&'a DraftIssue>,
}

/// Returns the id of the text field of parameter row `index`.
fn values_field_id(index: usize) -> Id {
    Id::new(("henad_sweep_values", index))
}

/// Draws one parameter row: its checkbox, its Parameters tab value, and its values while varied.
fn parameter_row(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    context: RowContext,
    row: &ParameterRow<'_>,
    factor: &mut FactorDraft,
) {
    let layout = rows.layout;
    let descriptor = row.descriptor;
    let baseline = row
        .panel_value
        .map(|value| display_value(descriptor, value))
        .unwrap_or_default();
    let vary_tooltip = match (context.search, layout.baseline_width.is_some()) {
        (true, true) => {
            "Search over this parameter. Unchecked parameters use values from the Parameters tab.".to_owned()
        }
        (true, false) => format!(
            "Search over this parameter. Unchecked parameters use values from the Parameters tab. Current value: \
             {baseline}."
        ),
        (false, true) => "Vary this parameter. Unchecked parameters use values from the Parameters tab.".to_owned(),
        (false, false) => format!(
            "Vary this parameter. Unchecked parameters use values from the Parameters tab. Current value: {baseline}."
        ),
    };
    let add_label = |ui: &mut egui::Ui, factor: &mut FactorDraft| match row.in_table {
        Some(true) => ui
            .add(Label::new(format!("{MDI_TABLE_COLUMN} {}", descriptor.label)).truncate())
            .on_hover_text("Set by design table"),
        Some(false) => ui
            .add(Label::new(descriptor.label).truncate())
            .on_hover_text("Not in design table. Uses value from the Parameters tab."),
        None => ui
            .add(Checkbox::new(&mut factor.vary, descriptor.label))
            .on_hover_text(vary_tooltip.as_str()),
    };
    let varied = row.in_table.is_none();

    if layout.stacked {
        let label = slot(ui, layout.cell_width, |ui| add_label(ui, factor));
        ticked(ui, rows, context, row, factor, &label);
        values_cell(ui, rows, context, row, factor, &label, layout.cell_width, &baseline);
        return;
    }
    ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui| {
        let label = slot(ui, layout.label_width, |ui| add_label(ui, factor));
        ticked(ui, rows, context, row, factor, &label);
        if let Some(width) = layout.baseline_width {
            slot(ui, width, |ui| {
                let text = if varied && factor.vary {
                    RichText::new(&baseline).weak()
                } else {
                    RichText::new(&baseline)
                };
                ui.add(Label::new(text).truncate())
                    .on_hover_text("Value from the Parameters tab, used for unchecked parameters");
            });
        }
        values_cell(ui, rows, context, row, factor, &label, layout.values_width(), &baseline);
    });
}

/// Fills in or focuses the values of a parameter whose checkbox `label` was just ticked.
///
/// A search starts a number at its whole range. A sweep focuses the empty field, and fills in nothing.
fn ticked(
    ui: &egui::Ui,
    rows: &mut FormRows,
    context: RowContext,
    row: &ParameterRow<'_>,
    factor: &mut FactorDraft,
    label: &egui::Response,
) {
    if !(label.changed() && factor.vary) || LevelNoun::of(&row.descriptor.kind) == LevelNoun::Option {
        return;
    }
    if context.search && factor.levels_text.trim().is_empty() {
        factor.levels_text = whole_range_text(&row.descriptor.kind);
    } else if !context.search {
        rows.focus_later(ui, values_field_id(row.index));
    }
}

/// Draws the values cell of a parameter row `width` wide: the values and their note while varied, and otherwise the
/// Parameters tab value `baseline` when no column shows it.
#[expect(clippy::too_many_arguments, reason = "the row's parts, each drawn in the cell")]
fn values_cell(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    context: RowContext,
    row: &ParameterRow<'_>,
    factor: &mut FactorDraft,
    label: &egui::Response,
    width: f32,
    baseline: &str,
) {
    let layout = rows.layout;
    column(ui, width, |ui| {
        if row.in_table == Some(true) {
            slot(ui, width, |ui| ui.weak("From design table"));
            return;
        }
        if row.in_table.is_some() || !factor.vary {
            if layout.baseline_width.is_none() {
                slot(ui, width, |ui| ui.weak(format!("at {baseline}")));
            }
            return;
        }
        let noun = LevelNoun::of(&row.descriptor.kind);
        let (control, text_len) = if noun == LevelNoun::Option {
            let menu = options_menu(ui, &row.descriptor.kind, &mut factor.levels_text, width);
            (menu.labelled_by(label.id), None)
        } else {
            let field = values_field(ui, context, row, &mut factor.levels_text, width);
            rows.take_focus(ui, &field);
            let len = factor.levels_text.chars().count();
            (field.labelled_by(label.id), Some(len))
        };
        rows.reveal_row(ui, DraftSite::Factor(row.index), &control, text_len);
        let held_back = text_len.is_some() && rows.holds_back_issue(ui, &control);
        let note = row_note(
            row.issue,
            row.preview,
            noun,
            context.search,
            context.draws_ranges,
            held_back,
        );
        let tooltip = row.preview.filter(|_| row.issue.is_none()).and_then(preview_tooltip);
        if let (Some(response), Some(tooltip)) = (note_line(ui, width, &note), tooltip) {
            response.on_hover_text(tooltip);
        }
    });
}

/// Draws the text field of a varied number's values, `width` wide with the editor's button after it.
fn values_field(
    ui: &mut egui::Ui,
    context: RowContext,
    row: &ParameterRow<'_>,
    text: &mut String,
    width: f32,
) -> egui::Response {
    let (hint, tooltip) = if context.search {
        (whole_range_text(&row.descriptor.kind), SEARCH_LEVELS_TOOLTIP)
    } else if context.draws_ranges {
        ("min:max or values".to_owned(), LEVELS_TOOLTIP)
    } else {
        ("Values or min:max:step".to_owned(), LEVELS_TOOLTIP)
    };
    ui.horizontal(|ui| {
        let field_width = (width - EDITOR_BUTTON_WIDTH - ui.spacing().item_spacing.x).max(0.0);
        let field = ui
            .add(
                TextEdit::singleline(text)
                    .id(values_field_id(row.index))
                    .hint_text(hint)
                    .desired_width(field_width),
            )
            .on_hover_text(tooltip);
        let name = format!("Edit values of {}", row.descriptor.label);
        let button = icon_button(ui, MDI_TUNE_VERTICAL, &name, "Edit values");
        Popup::from_toggle_button_response(&button)
            .id(Id::new(("henad_sweep_values_editor", row.index)))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .width(EDITOR_WIDTH)
            .show(|ui| {
                let editor = ValuesEditor {
                    descriptor: row.descriptor,
                    panel_value: row.panel_value,
                    search: context.search,
                    draws_ranges: context.draws_ranges,
                    note: row_note(
                        row.issue,
                        row.preview,
                        LevelNoun::Value,
                        context.search,
                        context.draws_ranges,
                        false,
                    ),
                };
                values_editor(ui, &editor, text);
            });
        field
    })
    .inner
}

/// Form the values editor edits a row's text in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSegment {
    /// Values listed one by one.
    Values,
    /// Values from a minimum to a maximum, a step apart.
    Range,
    /// A whole range, for a sampled design or a search to draw from.
    Drawn,
}

impl EditorSegment {
    /// Returns the form `text` takes in a draft that draws from a range when `draws_ranges` is set.
    ///
    /// Where no design draws, a range with no step is a stepped range. A whole number's range steps by 1 there.
    pub fn of(text: &str, draws_ranges: bool) -> Self {
        match parse_levels(text) {
            Ok(LevelSpec::Range { step: None, .. }) if draws_ranges => Self::Drawn,
            Ok(LevelSpec::Range { .. }) => Self::Range,
            _ => Self::Values,
        }
    }
}

/// Minimum, maximum and step of a range, as the editor shows them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeFields {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl RangeFields {
    /// Returns the range `text` writes for a parameter of `kind`, or the ends of what it lists.
    ///
    /// Text with no numbers gives the parameter's bounds. A range with no step gives a round quarter of it.
    pub fn of(text: &str, kind: &ParamKind) -> Self {
        let (low, high) = kind_bounds(kind);
        let default_step = |min: f64, max: f64| {
            let step = round_step((max - min) / 4.0);
            let step = if matches!(kind, ParamKind::U32 { .. }) {
                step.round().max(1.0)
            } else {
                step
            };
            if step.is_finite() && step > 0.0 { step } else { 1.0 }
        };
        match parse_levels(text) {
            Ok(LevelSpec::Range { min, max, step }) => Self {
                min,
                max,
                step: step.unwrap_or_else(|| default_step(min, max)),
            },
            Ok(LevelSpec::Values(values)) => {
                let numbers: Vec<f64> = values.iter().filter_map(|value| value.trim().parse().ok()).collect();
                let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
                let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if min.is_finite() && max.is_finite() && max > min {
                    Self {
                        min,
                        max,
                        step: default_step(min, max),
                    }
                } else {
                    Self {
                        min: low,
                        max: high,
                        step: default_step(low, high),
                    }
                }
            }
            _ => Self {
                min: low,
                max: high,
                step: default_step(low, high),
            },
        }
    }

    /// Returns the range as a parameter of `kind` writes it: `min:max:step`, or `min:max` for one drawn from.
    pub fn text(self, kind: &ParamKind, drawn: bool) -> String {
        let (min, max) = (number_text(kind, self.min), number_text(kind, self.max));
        if drawn {
            format!("{min}:{max}")
        } else {
            format!("{min}:{max}:{}", number_text(kind, self.step))
        }
    }
}

/// Returns the lowest and highest value of a parameter of `kind`, 0 and 1 for one that is not a number.
fn kind_bounds(kind: &ParamKind) -> (f64, f64) {
    match *kind {
        ParamKind::F32 { min, max, .. } => (f64::from(min), f64::from(max)),
        ParamKind::U32 { min, max, .. } => (f64::from(min), f64::from(max)),
        ParamKind::Bool { .. } | ParamKind::Choice { .. } => (0.0, 1.0),
    }
}

/// Returns the bounds of a parameter of `kind` as the editor shows them, as in "1 to 16384, integers".
pub fn bounds_text(kind: &ParamKind) -> String {
    let (low, high) = kind_bounds(kind);
    let text = format!("{} to {}", number_text(kind, low), number_text(kind, high));
    match kind {
        ParamKind::U32 { .. } => format!("{text}, integers"),
        _ => text,
    }
}

/// Returns the text `text` becomes when the editor switches it to `segment`, for a parameter of `kind`.
///
/// Listed values keep their ends as a range, and a range lists its ends.
pub fn segment_text(segment: EditorSegment, text: &str, kind: &ParamKind) -> String {
    let fields = RangeFields::of(text, kind);
    match segment {
        EditorSegment::Values => match parse_levels(text) {
            Ok(LevelSpec::Values(_)) => text.to_owned(),
            Ok(LevelSpec::Range { .. }) => {
                format!("{}, {}", number_text(kind, fields.min), number_text(kind, fields.max))
            }
            _ => String::new(),
        },
        EditorSegment::Range => fields.text(kind, false),
        EditorSegment::Drawn => fields.text(kind, true),
    }
}

/// Returns the quick fill that steps across a parameter's whole range, or takes it whole where a range is drawn from.
pub fn whole_range_fill(kind: &ParamKind, draws_ranges: bool) -> String {
    if draws_ranges {
        whole_range_text(kind)
    } else {
        levels_example(kind)
    }
}

/// Returns the quick fill of a parameter's minimum and maximum only.
pub fn both_ends_fill(kind: &ParamKind) -> String {
    let (low, high) = kind_bounds(kind);
    format!("{}, {}", number_text(kind, low), number_text(kind, high))
}

/// Returns the quick fill of half, one and one and a half times `value`, each held within a parameter's bounds, with
/// repeats dropped.
pub fn around_fill(kind: &ParamKind, value: &ParamValue) -> String {
    let center = match *value {
        // The decimal the value is written as. Widened in binary, 0.3 times 1.5 lands a hair past 0.45.
        ParamValue::F32(number) => number.to_string().parse().unwrap_or(f64::from(number)),
        ParamValue::U32(number) => f64::from(number),
        ParamValue::Bool(_) | ParamValue::Choice(_) => return format_value(kind, value),
    };
    let (low, high) = kind_bounds(kind);
    let mut texts: Vec<String> = Vec::new();
    for factor in [0.5, 1.0, 1.5] {
        let text = number_text(kind, (center * factor).clamp(low, high));
        if !texts.contains(&text) {
            texts.push(text);
        }
    }
    texts.join(", ")
}

/// Everything the values editor shows besides the row's text.
struct ValuesEditor<'a> {
    descriptor: &'a ParamDescriptor,
    panel_value: Option<&'a ParamValue>,
    search: bool,
    draws_ranges: bool,
    /// Preview of the row's values, the same note the row shows.
    note: Note,
}

/// Draws the values editor of a number's row, writing each edit into `text` at once.
fn values_editor(ui: &mut egui::Ui, editor: &ValuesEditor<'_>, text: &mut String) {
    let kind = &editor.descriptor.kind;
    ui.strong(editor.descriptor.label);
    ui.weak(bounds_text(kind));
    let mut segment = EditorSegment::of(text, editor.draws_ranges);
    let drawn_label = if editor.search {
        "Range to search"
    } else {
        "Range to draw from"
    };
    let choices = [
        (EditorSegment::Values, "Values", "List values one by one"),
        (EditorSegment::Range, "Range", "Step from a minimum to a maximum"),
        (
            EditorSegment::Drawn,
            drawn_label,
            "Allow any value from a minimum to a maximum",
        ),
    ];
    let draws_ranges = editor.draws_ranges;
    let switched = segmented(ui, &mut segment, &choices, |choice| {
        if choice == EditorSegment::Drawn && !draws_ranges {
            Err("Select Latin hypercube or Uniform random under Design to draw from a range")
        } else {
            Ok(())
        }
    });
    if switched {
        *text = segment_text(segment, text, kind);
    }
    match segment {
        EditorSegment::Values => {
            ui.weak("Enter comma-separated values, such as 0.1, 0.2.");
        }
        EditorSegment::Range | EditorSegment::Drawn => range_fields(ui, kind, segment, text),
    }
    ui.horizontal_wrapped(|ui| {
        let whole = ui
            .small_button("Whole range")
            .on_hover_text("Step across the whole range");
        if whole.clicked() {
            *text = whole_range_fill(kind, editor.draws_ranges);
        }
        if ui
            .small_button("Both ends")
            .on_hover_text("Minimum and maximum only")
            .clicked()
        {
            *text = both_ends_fill(kind);
        }
        if let Some(value) = editor.panel_value {
            let around = ui
                .small_button("Around current")
                .on_hover_text("0.5×, 1× and 1.5× the Parameters tab value, within bounds");
            if around.clicked() {
                *text = around_fill(kind, value);
            }
            let current = ui
                .small_button("Current value")
                .on_hover_text("Parameters tab value only");
            if current.clicked() {
                *text = format_value(kind, value);
            }
        }
    });
    note_label(ui, &editor.note);
    ui.weak("Same format as henad-cli --vary");
}

/// Draws the Minimum, Maximum and, for a stepped range, Step fields of the editor, writing a change into `text`.
fn range_fields(ui: &mut egui::Ui, kind: &ParamKind, segment: EditorSegment, text: &mut String) {
    let mut fields = RangeFields::of(text, kind);
    let (low, high) = kind_bounds(kind);
    let whole = matches!(kind, ParamKind::U32 { .. });
    let speed = ((high - low) / 200.0).max(if whole { 1.0 } else { 0.001 });
    fn number(value: &mut f64, range: std::ops::RangeInclusive<f64>, speed: f64, whole: bool) -> DragValue<'_> {
        let drag = DragValue::new(value).range(range).speed(speed);
        if whole { drag.fixed_decimals(0) } else { drag }
    }
    let mut changed = false;
    egui::Grid::new(("henad_sweep_range_fields", segment == EditorSegment::Drawn))
        .num_columns(2)
        .show(ui, |ui| {
            let label = ui.label("Minimum");
            changed |= ui
                .add(number(&mut fields.min, low..=high, speed, whole))
                .labelled_by(label.id)
                .changed();
            ui.end_row();
            let label = ui.label("Maximum");
            changed |= ui
                .add(number(&mut fields.max, low..=high, speed, whole))
                .labelled_by(label.id)
                .changed();
            ui.end_row();
            if segment == EditorSegment::Range {
                let smallest = if whole { 1.0 } else { f64::from(f32::EPSILON) };
                let label = ui.label("Step");
                changed |= ui
                    .add(number(
                        &mut fields.step,
                        smallest..=(high - low).max(smallest),
                        speed,
                        whole,
                    ))
                    .labelled_by(label.id)
                    .changed();
                ui.end_row();
            }
        });
    if changed {
        *text = fields.text(kind, segment == EditorSegment::Drawn);
    }
}

/// Returns the name of each option of a parameter of `kind`, as its values are written.
pub fn option_names(kind: &ParamKind) -> Vec<String> {
    match kind {
        ParamKind::Bool { .. } => [false, true]
            .map(|flag| format_value(kind, &ParamValue::Bool(flag)))
            .to_vec(),
        ParamKind::Choice { options, .. } => (0..options.len())
            .map(|index| format_value(kind, &ParamValue::Choice(index)))
            .collect(),
        ParamKind::F32 { .. } | ParamKind::U32 { .. } => Vec::new(),
    }
}

/// Returns whether `text` picks each option of a parameter of `kind`. `all` picks every one, and a value that is not
/// an option picks none.
pub fn picked_options(kind: &ParamKind, text: &str) -> Vec<bool> {
    let names = option_names(kind);
    if text.trim() == "all" {
        return vec![true; names.len()];
    }
    let mut picked = vec![false; names.len()];
    for part in text.split(',') {
        let index = match parse_value(kind, part.trim()) {
            Ok(ParamValue::Bool(flag)) => usize::from(flag),
            Ok(ParamValue::Choice(index)) => index,
            _ => continue,
        };
        if let Some(pick) = picked.get_mut(index) {
            *pick = true;
        }
    }
    picked
}

/// Returns the text of the options `picked` of `names`: `all` for every one, or the names joined by commas.
pub fn options_text(names: &[String], picked: &[bool]) -> String {
    if !picked.is_empty() && picked.iter().all(|pick| *pick) {
        return "all".to_owned();
    }
    let chosen: Vec<&str> = names
        .iter()
        .zip(picked)
        .filter(|(_, pick)| **pick)
        .map(|(name, _)| name.as_str())
        .collect();
    chosen.join(", ")
}

/// Returns the text of the options menu's button: "All options", the options picked, or "None".
pub fn options_button_text(names: &[String], picked: &[bool]) -> String {
    match options_text(names, picked).as_str() {
        "all" => "All options".to_owned(),
        "" => "None".to_owned(),
        text => text.to_owned(),
    }
}

/// Draws the menu of a checkbox or dropdown parameter's options `width` wide, writing the options picked into `text`.
///
/// The menu stays open while options are ticked, and closes on a click outside it.
fn options_menu(ui: &mut egui::Ui, kind: &ParamKind, text: &mut String, width: f32) -> egui::Response {
    let names = option_names(kind);
    let mut picked = picked_options(kind, text);
    let button = Button::new(options_button_text(&names, &picked))
        .truncate()
        .min_size(vec2(width, 0.0));
    let mut changed = false;
    let (response, _) = ui
        .scope(|ui| {
            ui.set_max_width(width);
            MenuButton::from_button(button)
                .config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
                .ui(ui, |ui| {
                    let mut all = picked.iter().all(|pick| *pick);
                    if ui.checkbox(&mut all, "All options").changed() {
                        picked.fill(all);
                        changed = true;
                    }
                    for (name, pick) in names.iter().zip(&mut picked) {
                        changed |= ui.checkbox(pick, name.as_str()).changed();
                    }
                })
        })
        .inner;
    if changed {
        *text = options_text(&names, &picked);
    }
    response.on_hover_text("Select options to run")
}

#[cfg(test)]
mod tests {
    use henad_core::params::{ParamKind, ParamValue};

    use super::{
        EditorSegment, LevelNoun, RangeFields, around_fill, both_ends_fill, bounds_text, format_hint,
        options_button_text, options_text, picked_options, preview_note, preview_tooltip, row_note, segment_text,
        whole_range_fill,
    };
    use crate::ui::sweep::draft::{DraftIssue, DraftSite, IssueKind, LevelPreview};
    use crate::ui::sweep::layout::Note;

    const RATE: ParamKind = ParamKind::F32 {
        min: 0.0,
        max: 1.0,
        default: 0.3,
        step: None,
    };

    const WIDTH: ParamKind = ParamKind::U32 {
        min: 1,
        max: 16384,
        default: 1024,
    };

    const NETWORK: ParamKind = ParamKind::Choice {
        options: &["Random", "Geometric"],
        default: 0,
    };

    fn listed(values: &[&str]) -> LevelPreview {
        LevelPreview::Listed {
            count: values.len(),
            values: values.iter().map(|&value| value.to_owned()).collect(),
            last: values.last().map_or_else(String::new, |&value| value.to_owned()),
        }
    }

    #[test]
    fn a_note_lists_up_to_six_values_and_skips_to_the_last_past_them() {
        let five = listed(&["0.1", "0.2", "0.3", "0.4", "0.5"]);
        assert_eq!(
            preview_note(&five, LevelNoun::Value, false),
            "5 values: 0.1, 0.2, 0.3, 0.4, 0.5"
        );
        assert_eq!(preview_tooltip(&five), None, "every value shows already");
        assert_eq!(preview_note(&listed(&["0.3"]), LevelNoun::Value, false), "1 value: 0.3");

        let twelve: Vec<String> = (1..=12).map(|tenth| format!("{}", f64::from(tenth) / 10.0)).collect();
        let twelve: Vec<&str> = twelve.iter().map(String::as_str).collect();
        let twelve = listed(&twelve);
        assert_eq!(
            preview_note(&twelve, LevelNoun::Value, false),
            "12 values: 0.1, 0.2, 0.3, …, 1.2"
        );
        assert_eq!(
            preview_tooltip(&twelve).as_deref(),
            Some("0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.1, 1.2")
        );
        let many = LevelPreview::Listed {
            count: 60,
            values: (0..50).map(|value| value.to_string()).collect(),
            last: "59".to_owned(),
        };
        assert!(
            preview_tooltip(&many).is_some_and(|text| text.ends_with("49 and 10 more")),
            "the hover text stops at 50"
        );
    }

    #[test]
    fn a_note_names_options_ticks_and_ranges_in_the_mode_s_words() {
        let options = listed(&["Random", "Geometric"]);
        assert_eq!(
            preview_note(&options, LevelNoun::Option, false),
            "2 options: Random, Geometric"
        );
        assert_eq!(
            preview_note(&options, LevelNoun::Option, true),
            "2 options: Random, Geometric"
        );
        assert_eq!(
            preview_note(&listed(&["100", "200", "300"]), LevelNoun::Tick, false),
            "3 ticks: 100, 200, 300"
        );
        assert_eq!(
            preview_note(&listed(&["0.1", "0.2"]), LevelNoun::Value, true),
            "2 values to select from: 0.1, 0.2"
        );
        let range = LevelPreview::Drawn {
            min: "0.1".to_owned(),
            max: "0.5".to_owned(),
        };
        assert_eq!(
            preview_note(&range, LevelNoun::Value, false),
            "Any value from 0.1 to 0.5, drawn by the design"
        );
        assert_eq!(
            preview_note(&range, LevelNoun::Value, true),
            "Any value from 0.1 to 0.5"
        );
        let ticks = LevelPreview::Drawn {
            min: "0".to_owned(),
            max: "500".to_owned(),
        };
        assert_eq!(preview_note(&ticks, LevelNoun::Tick, true), "Any tick from 0 to 500");
    }

    #[test]
    fn a_field_being_typed_in_shows_the_format_until_its_issue_is_due() {
        let invalid = DraftIssue::new(DraftSite::Factor(2), "'0.x' is not a number");
        let preview = listed(&["0.1"]);
        assert_eq!(
            row_note(Some(&invalid), None, LevelNoun::Value, false, false, true),
            Note::Weak("Format: 0.1, 0.2 or min:max:step".to_owned())
        );
        assert_eq!(
            row_note(Some(&invalid), None, LevelNoun::Value, false, false, false),
            Note::Issue(IssueKind::Invalid, "'0.x' is not a number".to_owned())
        );
        let missing = DraftIssue::missing(DraftSite::Factor(2), "Enter values, such as 0:1:0.2");
        assert_eq!(
            row_note(Some(&missing), None, LevelNoun::Value, false, false, true),
            Note::Issue(IssueKind::Missing, "Enter values, such as 0:1:0.2".to_owned()),
            "missing input is never held back"
        );
        assert_eq!(
            row_note(None, Some(&preview), LevelNoun::Value, false, false, false),
            Note::Weak("1 value: 0.1".to_owned())
        );
        assert_eq!(format_hint(LevelNoun::Tick, true), "Format: min:max or 100, 200");
    }

    #[test]
    fn the_editor_reads_the_form_of_a_row_s_text() {
        assert_eq!(EditorSegment::of("0.1, 0.2", false), EditorSegment::Values);
        assert_eq!(EditorSegment::of("", false), EditorSegment::Values);
        assert_eq!(EditorSegment::of("0:1:0.2", true), EditorSegment::Range);
        assert_eq!(EditorSegment::of("0:1", true), EditorSegment::Drawn);
        assert_eq!(
            EditorSegment::of("1:10", false),
            EditorSegment::Range,
            "a range with no step steps where nothing draws"
        );
    }

    #[test]
    fn the_editor_switches_text_between_its_forms() {
        assert_eq!(segment_text(EditorSegment::Range, "0.1, 0.5", &RATE), "0.1:0.5:0.1");
        assert_eq!(segment_text(EditorSegment::Drawn, "0.1:0.5:0.1", &RATE), "0.1:0.5");
        assert_eq!(segment_text(EditorSegment::Values, "0.1:0.5:0.1", &RATE), "0.1, 0.5");
        assert_eq!(segment_text(EditorSegment::Range, "", &WIDTH), "1:16384:5000");
        assert_eq!(
            RangeFields {
                min: 0.1,
                max: 0.9,
                step: 0.2
            }
            .text(&RATE, false),
            "0.1:0.9:0.2"
        );
        assert_eq!(bounds_text(&RATE), "0 to 1");
        assert_eq!(bounds_text(&WIDTH), "1 to 16384, integers");
    }

    #[test]
    fn quick_fills_write_the_text_a_command_line_takes() {
        assert_eq!(whole_range_fill(&RATE, false), "0:1:0.2");
        assert_eq!(whole_range_fill(&RATE, true), "0:1");
        assert_eq!(both_ends_fill(&WIDTH), "1, 16384");
        assert_eq!(around_fill(&RATE, &ParamValue::F32(0.3)), "0.15, 0.3, 0.45");
        assert_eq!(
            around_fill(&RATE, &ParamValue::F32(0.8)),
            "0.4, 0.8, 1",
            "held within the bounds"
        );
        assert_eq!(
            around_fill(&WIDTH, &ParamValue::U32(1)),
            "1, 2",
            "a whole number rounds, and a repeat is dropped"
        );
    }

    #[test]
    fn the_options_menu_reads_and_writes_all_or_a_list() {
        let names = super::option_names(&NETWORK);
        assert_eq!(names, ["Random", "Geometric"]);
        assert_eq!(picked_options(&NETWORK, "all"), [true, true]);
        assert_eq!(picked_options(&NETWORK, "Geometric"), [false, true]);
        assert_eq!(picked_options(&NETWORK, "Random, Geometric"), [true, true]);
        assert_eq!(picked_options(&NETWORK, "Square"), [false, false]);
        assert_eq!(options_text(&names, &[true, true]), "all");
        assert_eq!(options_text(&names, &[false, true]), "Geometric");
        assert_eq!(options_text(&names, &[false, false]), "");
        assert_eq!(options_button_text(&names, &[true, true]), "All options");
        assert_eq!(options_button_text(&names, &[false, false]), "None");

        let flag = ParamKind::Bool { default: false };
        assert_eq!(picked_options(&flag, "true"), [false, true]);
        assert_eq!(options_text(&super::option_names(&flag), &[true, false]), "false");
    }
}
