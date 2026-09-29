//! Form of the Sweep tab while no sweep is running: one collapsible section per group of settings.
//!
//! Every row lays out through one [`FormLayout`], so the sections share a label column. A row that can show a note
//! reserves the note's line before any note comes, so nothing moves when an issue comes or goes.

use std::num::NonZeroUsize;
use std::path::Path;

use egui::containers::Sides;
use egui::{Align, Button, ComboBox, DragValue, Id, Label, Layout, RadioButton, RichText, TextEdit, TextStyle, vec2};
use henad_core::explore::plan::ModelSchema;
use henad_core::explore::reducer::{ReducerKind, ReducerSpec};
use henad_core::explore::stop::{Comparator, Comparison};
use henad_core::helpers::fmt_bytes;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;
use henad_explore::exec::Concurrency;
use henad_models::registry::ModelEntry;

use crate::icons::material_design_icons::{
    MDI_DELETE_OUTLINE, MDI_DICE_5, MDI_FILE_DELIMITED_OUTLINE, MDI_FOLDER_OUTLINE, MDI_PLUS,
};
use crate::ui::params::{display_value, draw_seed, seed_field_width};
use crate::ui::plural;
use crate::ui::results::store::{ResultsStore, column_output_label, reducer_label};
use crate::ui::sweep::draft::{
    ActionDraft, DesignTableDraft, DraftDesign, DraftIssue, DraftMode, DraftSite, GridAxis, IssueKind, LevelPreview,
    MAX_MEMORY_SERIES_BYTES, StopDraft, SweepDraft, TICKS_EXAMPLE, comparator_words,
};
use crate::ui::sweep::layout::{
    FormLayout, FormRows, FormState, Note, SectionHeading, SweepSection, add_button, checkbox_indent, icon_button,
    icon_button_width, note_block, note_label, note_line, segmented, slot, text_width, widest_label,
};
use crate::ui::sweep::parameters::{
    BASELINE_HEADER, LEVELS_TOOLTIP, LevelNoun, SEARCH_LEVELS_TOOLTIP, parameters_section, preview_tooltip, row_note,
};
use crate::ui::sweep::plan::{
    NOT_COUNTED, PlanSummary, concurrency_text, folder_name, sample_count, seconds_text, stop_text,
};
use crate::ui::sweep::search::{algorithm_label, search_labels, search_section};
use crate::ui::sweep::{CheckSummary, DraftCheck, SweepRequest};

/// Most samples the Samples field takes.
const MAX_SAMPLES: usize = 1 << 20;

/// Seconds a run is given once Timeout is ticked, until the user sets another number.
const DEFAULT_TIMEOUT_SECONDS: f64 = 600.0;

/// Width of an action's tick field, and of the text field that takes its place while the tick varies, in points.
const TICKS_FIELD_WIDTH: f32 = 160.0;

/// Width of a comparator combo box, in points.
const COMPARATOR_WIDTH: f32 = 110.0;

/// Every comparator, in the order a comparator combo box lists them.
const COMPARATORS: [Comparator; 6] = [
    Comparator::Less,
    Comparator::LessOrEqual,
    Comparator::Equal,
    Comparator::NotEqual,
    Comparator::GreaterOrEqual,
    Comparator::Greater,
];

/// Every output kind, in the order the output combo box lists them.
const REDUCER_KINDS: [ReducerKind; 8] = [
    ReducerKind::Final,
    ReducerKind::Min,
    ReducerKind::Max,
    ReducerKind::Mean,
    ReducerKind::ArgMax,
    ReducerKind::ArgMin,
    ReducerKind::FirstCrossing(Comparison {
        comparator: Comparator::LessOrEqual,
        threshold: 0.0,
    }),
    ReducerKind::WindowMean { start: 0, end: 0 },
];

/// Everything the form reads besides the draft it edits.
pub struct FormInput<'a> {
    pub entry: &'a ModelEntry,
    pub schema: &'a ModelSchema<'a>,
    pub panel_values: &'a [ParamValue],
    pub summary: &'a CheckSummary,
    pub sections: &'a SectionSummaries,
    pub plan: &'a PlanSummary,
    /// Results the Results tab holds of the draft's model, `None` when it holds none.
    pub results: Option<&'a ResultsStore>,
}

/// One-line summary of each section's settings, as its header shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionSummaries {
    pub design: String,
    pub search: String,
    pub parameters: String,
    pub actions: String,
    pub seeds: String,
    pub run_length: String,
    pub outputs: String,
    pub execution: String,
    pub plan: String,
}

impl SectionSummaries {
    /// Returns the summaries of the draft `check` holds, a draft of `schema`'s model, with the totals of `summary`.
    pub(super) fn new(check: &DraftCheck, schema: &ModelSchema<'_>, summary: &CheckSummary) -> Self {
        let draft = &check.draft;
        Self {
            design: design_summary(draft, summary.counts.map(|counts| counts.0)),
            search: format!(
                "{} · {} {}",
                algorithm_label(draft.search.algorithm),
                draft.search.max_evaluations,
                plural(draft.search.max_evaluations, "evaluation")
            ),
            parameters: parameters_summary(draft, schema),
            actions: actions_summary(draft, schema),
            seeds: seeds_summary(draft),
            run_length: run_length_summary(draft),
            outputs: outputs_summary(draft),
            execution: execution_summary(draft),
            plan: match summary.counts {
                Some((_, _, runs)) => format!("{runs} {}", plural(runs, "run")),
                None => NOT_COUNTED.to_owned(),
            },
        }
    }

    /// Returns the summary of `section`.
    pub fn text(&self, section: SweepSection) -> &str {
        match section {
            SweepSection::Design => &self.design,
            SweepSection::Search => &self.search,
            SweepSection::Parameters => &self.parameters,
            SweepSection::Actions => &self.actions,
            SweepSection::Seeds => &self.seeds,
            SweepSection::RunLength => &self.run_length,
            SweepSection::Outputs => &self.outputs,
            SweepSection::Execution => &self.execution,
            SweepSection::Plan => &self.plan,
        }
    }
}

/// Returns the Design section's summary, with `configs` configurations once counted, as in "Every combination · 15
/// configurations".
fn design_summary(draft: &SweepDraft, configs: Option<u64>) -> String {
    let label = design_label(draft.design);
    match draft.design {
        DraftDesign::LatinHypercube | DraftDesign::UniformRandom => {
            let samples = draft.samples as u64;
            format!("{label} · {samples} {}", plural(samples, "sample"))
        }
        DraftDesign::Table => match &draft.table {
            Some(table) => {
                let rows = table.row_count() as u64;
                format!("{label} · {rows} {}", plural(rows, "row"))
            }
            None => format!("{label} · no table loaded"),
        },
        DraftDesign::EveryCombination | DraftDesign::Zip | DraftDesign::VaryEachAlone => match configs {
            Some(configs) => format!("{label} · {configs} {}", plural(configs, "configuration")),
            None => label.to_owned(),
        },
    }
}

/// Returns the Parameters section's summary, as in "2 of 5 varied".
fn parameters_summary(draft: &SweepDraft, schema: &ModelSchema<'_>) -> String {
    let total = schema.params.len();
    if total == 0 {
        return "None".to_owned();
    }
    if draft.runs_table() {
        return "Set by design table".to_owned();
    }
    let verb = match draft.mode {
        DraftMode::Sweep => "varied",
        DraftMode::Search => "searched",
    };
    match draft.factors.iter().filter(|factor| factor.vary).count() {
        0 => format!("None {verb}"),
        varied => format!("{varied} of {total} {verb}"),
    }
}

/// Returns the Actions section's summary, as in "Seed outbreak at tick 10" or "2 actions, 1 tick varied".
fn actions_summary(draft: &SweepDraft, schema: &ModelSchema<'_>) -> String {
    let verb = match draft.mode {
        DraftMode::Sweep => "varied",
        DraftMode::Search => "searched",
    };
    let varied = draft.actions.iter().filter(|action| action.vary_tick).count() as u64;
    match draft.actions.as_slice() {
        [] => "None".to_owned(),
        [action] => {
            let label = action.label(schema);
            if action.vary_tick {
                format!("{label}, tick {verb}")
            } else {
                format!("{label} at tick {}", action.tick)
            }
        }
        actions => {
            let mut text = format!("{} actions", actions.len());
            if varied > 0 {
                text.push_str(&format!(", {varied} {} {verb}", plural(varied, "tick")));
            }
            text
        }
    }
}

/// Returns the Replicates and seeds section's summary, as in "3 replicates · root 0 · common random numbers".
fn seeds_summary(draft: &SweepDraft) -> String {
    let scheme = if draft.common_random_numbers {
        "common random numbers"
    } else {
        "independent seeds"
    };
    let mut text = format!(
        "{} {} · root {} · {scheme}",
        draft.replicates,
        plural(draft.replicates, "replicate"),
        draft.root_seed_text.trim()
    );
    let design_seed = draft.design_seed_text.trim();
    if draft.mode == DraftMode::Sweep && draft.design.is_sampled() && !design_seed.is_empty() {
        text.push_str(&format!(" · design seed {design_seed}"));
    }
    text
}

/// Returns the Run length section's summary, as in "1000 steps, 100 warm-up · stops when Infected is at most 0".
fn run_length_summary(draft: &SweepDraft) -> String {
    let mut text = format!("{} {}", draft.steps, plural(draft.steps, "step"));
    if draft.warmup > 0 {
        text.push_str(&format!(", {} warm-up", draft.warmup));
    }
    if let Some(stop) = &draft.stop {
        text.push_str(&format!(" · stops when {}", stop_text(stop)));
    }
    if let Some(seconds) = draft.timeout_s {
        text.push_str(&format!(" · {} timeout", seconds_text(seconds)));
    }
    text
}

/// Returns the Outputs section's summary, as in "4 per stat and 1 more · a sample every tick".
fn outputs_summary(draft: &SweepDraft) -> String {
    let added = draft.reducers.len() as u64;
    let outputs = match (draft.default_reducers, added) {
        (true, 0) => "4 per stat".to_owned(),
        (true, added) => format!("4 per stat and {added} more"),
        (false, 0) => "None".to_owned(),
        (false, added) => format!("{added} {}", plural(added, "output")),
    };
    let sampling = match draft.stats_every {
        1 => "a sample every tick".to_owned(),
        ticks => format!("a sample every {ticks} ticks"),
    };
    let series = match draft.series_every {
        0 => " · no series".to_owned(),
        ticks if ticks == draft.stats_every => String::new(),
        ticks => format!(", series every {ticks}"),
    };
    format!("{outputs} · {sampling}{series}")
}

/// Returns the Execution section's summary, as in "Auto · results in memory" or "4 at once · results to sweep-01".
fn execution_summary(draft: &SweepDraft) -> String {
    let concurrency = match (draft.concurrency, cfg!(target_arch = "wasm32")) {
        (Concurrency::Auto, false) => "Auto".to_owned(),
        _ => format!("{} at once", concurrency_text(draft.concurrency)),
    };
    let results = match draft.output_folder() {
        _ if draft.holds_results_in_memory() => "results in memory".to_owned(),
        Some(folder) => format!("results to {}", folder_name(Path::new(folder))),
        None => "no folder selected".to_owned(),
    };
    format!("{concurrency} · {results}")
}

/// Returns each label the form's rows can show for `draft`, paired with the width a checkbox or an indent adds before
/// it.
///
/// Labels of collapsed sections count too, so opening one moves no column. A design's own rows count only under that
/// design.
fn form_labels(ui: &egui::Ui, draft: &SweepDraft, schema: &ModelSchema<'_>) -> Vec<(&'static str, f32)> {
    let indent = checkbox_indent(ui);
    let mut labels: Vec<(&'static str, f32)> = Vec::new();
    match draft.mode {
        DraftMode::Sweep => {
            labels.push(("Design", 0.0));
            if draft.design.is_sampled() {
                labels.extend([("Samples", 0.0), ("Design seed", 0.0)]);
            }
            if draft.design == DraftDesign::Table {
                labels.push(("Table", 0.0));
            }
        }
        DraftMode::Search => labels.extend(search_labels(ui, draft.search.algorithm)),
    }
    labels.extend(schema.params.iter().map(|descriptor| (descriptor.label, indent)));
    if !draft.actions.is_empty() {
        labels.extend([("Action", 0.0), ("Tick", 0.0)]);
    }
    // The rows under a checkbox or an output count while hidden too, so ticking one moves no column.
    labels.extend([
        ("Replicates", 0.0),
        ("Root seed", 0.0),
        ("Steps", 0.0),
        ("Warm-up", 0.0),
        ("Stop when", indent),
        ("Stat", indent),
        ("Condition", indent),
        ("From tick", indent),
        ("Timeout", indent),
        ("Seconds per run", indent),
        ("Sample every", 0.0),
        ("Series every", 0.0),
        ("Output", 0.0),
        ("Crossing", indent),
        ("To tick", indent),
        ("Concurrent runs", 0.0),
        ("Results", 0.0),
        ("Folder", 0.0),
    ]);
    labels
}

/// Returns the layout of a form `form_width` wide for `draft`, stacked when `stacked` is set.
fn form_layout(ui: &egui::Ui, draft: &SweepDraft, input: &FormInput<'_>, form_width: f32, stacked: bool) -> FormLayout {
    let widest_baseline = input
        .schema
        .params
        .iter()
        .zip(input.panel_values)
        .map(|(descriptor, value)| text_width(ui, &display_value(descriptor, value)))
        .fold(text_width(ui, BASELINE_HEADER), f32::max);
    FormLayout::new(
        form_width,
        widest_label(ui, form_labels(ui, draft, input.schema)),
        widest_baseline,
        stacked,
        ui.spacing().item_spacing.x,
    )
}

/// Draws the form of `draft`, one section per group of settings, with the layout flags of `form`.
pub fn form_ui(
    ui: &mut egui::Ui,
    draft: &mut SweepDraft,
    form: &mut FormState,
    input: &FormInput<'_>,
    request: &mut Option<SweepRequest>,
) {
    let form_width = ui.available_width();
    form.fit_form(form_width);
    form.reveal = form.reveal.map(|reveal| reveal.in_mode(draft.mode));
    let target = form.reveal;
    let mut rows = FormRows {
        layout: form_layout(ui, draft, input, form_width, form.stacked),
        reveal: None,
        revealed: false,
        typing: form.typing,
        focus_field: form.focus_field,
        time: ui.input(|input| input.time),
        pending: false,
    };
    let mut drawn = Vec::new();
    let mut settled = false;
    {
        let mut show =
            |ui: &mut egui::Ui, section: SweepSection, add_body: &mut dyn FnMut(&mut egui::Ui, &mut FormRows, Id)| {
                if !drawn.is_empty() {
                    ui.separator();
                }
                drawn.push(section);
                let heading = SectionHeading {
                    title: section.title(),
                    issues: input.summary.section_issues(section),
                    summary: input.sections.text(section),
                };
                let revealing = target.filter(|reveal| reveal.section == section);
                let outcome = crate::ui::sweep::layout::section(
                    ui,
                    section.id_salt(),
                    heading,
                    section.default_open(),
                    revealing.is_some(),
                    |ui, body| {
                        rows.reveal = revealing.and_then(|reveal| reveal.site).filter(|_| body.settled);
                        rows.revealed = false;
                        rows.pending = false;
                        add_body(ui, &mut rows, body.heading);
                    },
                );
                if outcome.settled && rows.pending {
                    // The row sits in a nested section that is still opening.
                    ui.ctx().request_repaint();
                } else if outcome.settled {
                    // A reveal of a row the section does not draw, such as the design seed of a design that has none,
                    // shows the section instead.
                    if !rows.revealed {
                        outcome.header.scroll_to_me(Some(Align::Min));
                    }
                    settled = true;
                }
                rows.reveal = None;
                rows.pending = false;
            };

        match draft.mode {
            DraftMode::Sweep => show(ui, SweepSection::Design, &mut |ui, rows, _| {
                design_section(ui, rows, draft, input, request);
            }),
            DraftMode::Search => show(ui, SweepSection::Search, &mut |ui, rows, _| {
                search_section(ui, rows, draft, input);
            }),
        }
        show(ui, SweepSection::Parameters, &mut |ui, rows, _| {
            parameters_section(ui, rows, draft, input);
        });
        if !input.schema.actions.is_empty() {
            show(ui, SweepSection::Actions, &mut |ui, rows, _| {
                actions_section(ui, rows, draft, input);
            });
        }
        show(ui, SweepSection::Seeds, &mut |ui, rows, _| {
            seeds_section(ui, rows, draft, input);
        });
        show(ui, SweepSection::RunLength, &mut |ui, rows, _| {
            run_length_section(ui, rows, draft, input);
        });
        show(ui, SweepSection::Outputs, &mut |ui, rows, _| {
            outputs_section(ui, rows, draft, input);
        });
        show(ui, SweepSection::Execution, &mut |ui, rows, _| {
            execution_section(ui, rows, draft, input, request);
        });
        if !form.plan_fits {
            show(ui, SweepSection::Plan, &mut |ui, _, _| {
                crate::ui::sweep::plan::plan_body(ui, input.plan, true, request);
            });
        }
    }
    form.typing = rows.typing;
    form.focus_field = rows.focus_field;
    // A reveal of a section this form does not hold, such as Actions for a model with none, has nothing to open.
    if settled || target.is_some_and(|reveal| !drawn.contains(&reveal.section)) {
        form.reveal = None;
    }
}

/// Draws a label and the widget it names on one form row, both with the tooltip `tooltip`.
pub(super) fn labeled(ui: &mut egui::Ui, layout: &FormLayout, label: &str, tooltip: &str, widget: impl egui::Widget) {
    let (label, widget) = layout.row(ui, label, |ui| ui.add(widget));
    let label = label.on_hover_text(tooltip);
    widget.on_hover_text(tooltip).labelled_by(label.id);
}

/// Returns the note of the first issue of `site` in `summary`, or `fallback` when it has none.
fn site_note(summary: &CheckSummary, site: DraftSite, fallback: Note) -> Note {
    Note::issue_or(summary.issues_at(site), fallback)
}

/// Returns the name the Sweep tab gives `design`.
pub fn design_label(design: DraftDesign) -> &'static str {
    match design {
        DraftDesign::EveryCombination => "Every combination",
        DraftDesign::Zip => "Zip",
        DraftDesign::VaryEachAlone => "One at a time",
        DraftDesign::LatinHypercube => "Latin hypercube",
        DraftDesign::UniformRandom => "Uniform random",
        DraftDesign::Table => "Design table",
    }
}

fn design_tooltip(design: DraftDesign) -> &'static str {
    match design {
        DraftDesign::EveryCombination => "Run every combination of listed values",
        DraftDesign::Zip => {
            "Pair the first values of every parameter, then the second, and so on. Use lists of equal length."
        }
        DraftDesign::VaryEachAlone => {
            "Vary one parameter at a time. Other parameters use values from the Parameters tab."
        }
        DraftDesign::LatinHypercube => "Spread samples evenly across each range",
        DraftDesign::UniformRandom => "Draw each sample independently",
        DraftDesign::Table => "Run each row of a CSV file as a configuration",
    }
}

/// Returns the line under the Design field that says what `design` runs.
pub fn design_description(design: DraftDesign) -> &'static str {
    match design {
        DraftDesign::EveryCombination => "Runs every combination of the listed values.",
        DraftDesign::Zip => "Pairs the first values of every parameter, then the second, and so on.",
        DraftDesign::VaryEachAlone => {
            "Varies one parameter at a time. Other parameters use values from the Parameters tab."
        }
        DraftDesign::LatinHypercube => "Spreads samples evenly across each range. Enter a range as min:max.",
        DraftDesign::UniformRandom => "Draws each sample independently. Enter a range as min:max.",
        DraftDesign::Table => "Runs each row of a CSV file as a configuration.",
    }
}

/// Returns how the design of `draft` makes its `configs` configurations from the values of its rows.
///
/// `param_previews` and `tick_previews` are the values of the parameter and action rows. The text reads as in
/// "Infection Rate (5) × Recovery Rate (3) = 15 configurations" or "5 + 3 = 8 configurations".
pub fn design_formula(
    draft: &SweepDraft,
    schema: &ModelSchema<'_>,
    param_previews: &[Option<LevelPreview>],
    tick_previews: &[Option<LevelPreview>],
    configs: u64,
) -> String {
    let configs_text = format!("{configs} {}", plural(configs, "configuration"));
    if draft.design == DraftDesign::Table {
        return format!("{configs_text}, one per row");
    }
    let params = schema
        .params
        .iter()
        .zip(&draft.factors)
        .zip(param_previews)
        .filter(|((_, factor), _)| factor.vary)
        .map(|((descriptor, _), preview)| (descriptor.label.to_owned(), preview.as_ref()));
    let ticks = draft
        .actions
        .iter()
        .zip(tick_previews)
        .filter(|(action, _)| action.vary_tick)
        .map(|(action, preview)| (format!("{} tick", action.label(schema)), preview.as_ref()));
    let varied: Vec<(String, Option<&LevelPreview>)> = params.chain(ticks).collect();
    if varied.is_empty() {
        return format!("Nothing varies, so {configs_text} will run.");
    }
    let listed = |preview: Option<&LevelPreview>| match preview {
        Some(LevelPreview::Listed { count, .. }) => Some(*count),
        _ => None,
    };
    match draft.design {
        DraftDesign::EveryCombination => {
            let factors: Vec<String> = varied
                .iter()
                .map(|(label, preview)| match listed(*preview) {
                    Some(count) => format!("{label} ({count})"),
                    None => label.clone(),
                })
                .collect();
            format!("{} = {configs_text}", factors.join(" × "))
        }
        DraftDesign::VaryEachAlone if varied.len() > 1 => {
            let counts: Vec<String> = varied
                .iter()
                .filter_map(|(_, preview)| listed(*preview))
                .map(|count| count.to_string())
                .collect();
            format!("{} = {configs_text}", counts.join(" + "))
        }
        DraftDesign::VaryEachAlone => configs_text,
        DraftDesign::Zip => format!("{configs_text}. Use lists of equal length."),
        DraftDesign::LatinHypercube | DraftDesign::UniformRandom | DraftDesign::Table => {
            let ranges = varied.iter().filter(|(_, preview)| listed(*preview).is_none()).count() as u64;
            let lists = varied.len() as u64 - ranges;
            let mut parts = Vec::new();
            if ranges > 0 {
                parts.push(format!("{ranges} {}", plural(ranges, "range")));
            }
            if lists > 0 {
                parts.push(format!("{lists} {}", plural(lists, "list")));
            }
            format!("{configs_text} from {}", parts.join(" and "))
        }
    }
}

fn design_section(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    draft: &mut SweepDraft,
    input: &FormInput<'_>,
    request: &mut Option<SweepRequest>,
) {
    let layout = rows.layout;
    // The rows of a design table report the table's own issues.
    let formula = match input.summary.issues_at(DraftSite::Design).next() {
        Some(issue) if !draft.runs_table() => Note::Issue(issue.kind, issue.message.clone()),
        _ => match input.summary.counts {
            Some((configs, _, _)) => Note::Weak(design_formula(
                draft,
                input.schema,
                &input.summary.param_previews,
                &input.summary.tick_previews,
                configs,
            )),
            None => Note::Weak(NOT_COUNTED.to_owned()),
        },
    };
    let (label, combo) = layout.row(ui, "Design", |ui| {
        let combo = ComboBox::from_id_salt("sweep_design")
            .width(layout.control_width())
            .truncate()
            .selected_text(design_label(draft.design))
            .show_ui(ui, |ui| {
                for design in DraftDesign::ALL {
                    ui.selectable_value(&mut draft.design, design, design_label(design))
                        .on_hover_text(design_tooltip(design));
                }
            })
            .response;
        let description = design_description(draft.design);
        note_block(ui, layout.cell_width, description, &Note::Weak(description.to_owned()));
        note_line(ui, layout.cell_width, &formula);
        combo
    });
    let combo = combo.on_hover_text(design_tooltip(draft.design)).labelled_by(label.id);
    rows.reveal_row(ui, DraftSite::Design, &combo, None);

    if draft.design.is_sampled() {
        labeled(
            ui,
            &layout,
            "Samples",
            "Configurations to draw",
            DragValue::new(&mut draft.samples).range(1..=MAX_SAMPLES),
        );
    }
    if draft.design == DraftDesign::Table {
        table_row(ui, &layout, draft, input, request);
    }
}

/// Returns the note of a loaded design table: the parameters and ticks its columns set, as in "Sets Infection Rate
/// and Recovery Rate".
pub fn table_note(table: &DesignTableDraft, draft: &SweepDraft, schema: &ModelSchema<'_>) -> String {
    let names: Vec<String> = table
        .columns()
        .iter()
        .filter_map(|column| {
            if let Some(name) = column.strip_prefix("action.") {
                let action = draft
                    .actions
                    .iter()
                    .find(|action| action.name == name && action.action_index < schema.actions.len())?;
                return Some(format!("{} tick", action.label(schema)));
            }
            schema
                .params
                .iter()
                .find(|descriptor| descriptor.id == column)
                .map(|descriptor| descriptor.label.to_owned())
        })
        .collect();
    match names.as_slice() {
        [] => "Sets no parameter of this model".to_owned(),
        [only] => format!("Sets {only}"),
        [rest @ .., last] => format!("Sets {} and {last}", rest.join(", ")),
    }
}

fn table_row(
    ui: &mut egui::Ui,
    layout: &FormLayout,
    draft: &mut SweepDraft,
    input: &FormInput<'_>,
    request: &mut Option<SweepRequest>,
) {
    let note = match &draft.table {
        Some(table) => site_note(
            input.summary,
            DraftSite::Design,
            Note::Weak(table_note(table, draft, input.schema)),
        ),
        None => site_note(input.summary, DraftSite::Design, Note::Empty),
    };
    let mut removed = false;
    let (label, control) = layout.row(ui, "Table", |ui| {
        let control = ui
            .horizontal(|ui| match &draft.table {
                None => add_button(
                    ui,
                    true,
                    Button::new(format!("{MDI_FILE_DELIMITED_OUTLINE} Load design CSV")),
                    "Load design CSV",
                )
                .on_hover_text(
                    "Select CSV file. Each column is a parameter id or action.NAME. Each row is a configuration.",
                ),
                Some(table) => {
                    let rows = table.row_count() as u64;
                    let file = ui
                        .add(Label::new(format!("{} · {rows} {}", table.file_name, plural(rows, "row"))).truncate())
                        .on_hover_text(format!("Columns: {}", table.columns().join(", ")));
                    let replace = ui.button("Replace").on_hover_text("Load another CSV file");
                    removed = ui.button("Remove").on_hover_text("Remove design table").clicked();
                    if replace.clicked() {
                        *request = Some(SweepRequest::LoadTable);
                    }
                    file
                }
            })
            .inner;
        note_line(ui, layout.cell_width, &note);
        control
    });
    if draft.table.is_none() && control.clicked() {
        *request = Some(SweepRequest::LoadTable);
    }
    control.labelled_by(label.id);
    if removed {
        draft.table = None;
    }
}

fn actions_section(ui: &mut egui::Ui, rows: &mut FormRows, draft: &mut SweepDraft, input: &FormInput<'_>) {
    let layout = rows.layout;
    let schema = input.schema;
    let context = TickContext {
        search: draft.mode == DraftMode::Search,
        runs_table: draft.runs_table(),
        draws_ranges: draft.draws_ranges(),
    };
    if draft.actions.is_empty() {
        ui.add(Label::new(RichText::new("Press Add action to schedule a model action in every run.").weak()).wrap());
    }
    let mut chosen = None;
    let mut removed = None;
    for (position, action) in draft.actions.iter_mut().enumerate() {
        if position > 0 {
            ui.add(egui::Separator::default().spacing(2.0));
        }
        let selected = schema
            .actions
            .get(action.action_index)
            .map_or("", |declared| declared.label);
        let (label, combo) = layout.row(ui, "Action", |ui| {
            ui.horizontal(|ui| {
                let mut action_index = action.action_index;
                let icon_width = icon_button_width(ui, MDI_DELETE_OUTLINE) + ui.spacing().item_spacing.x;
                let combo_width = (layout.control_width() - icon_width).max(0.0);
                let combo = slot(ui, combo_width, |ui| {
                    ComboBox::from_id_salt(("sweep_action", position))
                        .width(combo_width)
                        .truncate()
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (index, declared) in schema.actions.iter().enumerate() {
                                ui.selectable_value(&mut action_index, index, declared.label);
                            }
                        })
                        .response
                })
                .on_hover_text("Runs after the step on the corresponding tick");
                if action_index != action.action_index {
                    chosen = Some((position, action_index));
                }
                let name = format!("Remove {selected}");
                if icon_button(ui, MDI_DELETE_OUTLINE, &name, "Remove action").clicked() {
                    removed = Some(position);
                }
                combo
            })
            .inner
        });
        let combo = combo.labelled_by(label.id);
        let (field, varied) = tick_row(ui, rows, input, context, position, action);
        // A row with a fixed tick reveals its action, as for an action the model does not declare.
        let site = DraftSite::Action(position);
        if varied {
            rows.reveal_row(ui, site, &field, Some(action.ticks_text.chars().count()));
        } else {
            rows.reveal_row(ui, site, &combo, None);
        }
    }
    if let Some((position, action_index)) = chosen {
        draft.set_action(schema, position, action_index);
    }
    if let Some(position) = removed {
        draft.actions.remove(position);
    }
    let add = add_button(ui, true, Button::new(format!("{MDI_PLUS} Add action")), "Add action")
        .on_hover_text("Schedule a model action in every run");
    if add.clicked() {
        draft.add_action(schema, 0, 0);
    }
}

/// Mode of the draft an action's Tick row belongs to.
#[derive(Debug, Clone, Copy)]
struct TickContext {
    search: bool,
    /// Whether a design table sets the ticks.
    runs_table: bool,
    /// Whether a range with no step is drawn from, as in a sampled design or a search.
    draws_ranges: bool,
}

/// Draws the Tick row of action row `position`: its tick, or while varied its ticks and their note.
///
/// Returns the field holding the tick or the ticks, and whether the ticks vary.
fn tick_row(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    input: &FormInput<'_>,
    context: TickContext,
    position: usize,
    action: &mut ActionDraft,
) -> (egui::Response, bool) {
    let layout = rows.layout;
    let (vary_label, vary_tooltip) = if context.search {
        ("Search tick", "Search over this action's tick")
    } else {
        ("Vary tick", "Try the action at several ticks")
    };
    let warning = input
        .summary
        .action_warnings
        .iter()
        .find(|(warned, _)| *warned == position)
        .map(|(_, text)| text.clone());
    let varied = action.vary_tick && !context.runs_table;
    let (label, field) = layout.row(ui, "Tick", |ui| {
        let field = ui
            .horizontal(|ui| {
                let field_width = TICKS_FIELD_WIDTH.min(layout.cell_width * 0.5);
                let field = slot(ui, field_width, |ui| {
                    if varied {
                        let tooltip = if context.search {
                            SEARCH_LEVELS_TOOLTIP
                        } else {
                            LEVELS_TOOLTIP
                        };
                        let field = TextEdit::singleline(&mut action.ticks_text)
                            .hint_text(TICKS_EXAMPLE)
                            .desired_width(field_width);
                        return ui.add(field).on_hover_text(tooltip);
                    }
                    let drag = ui
                        .add(DragValue::new(&mut action.tick))
                        .on_hover_text("Tick to run the action on");
                    // The ticks start as the fixed tick once they vary.
                    if drag.changed() {
                        action.ticks_text = action.tick.to_string();
                    }
                    drag
                });
                let was_varied = action.vary_tick;
                ui.add_enabled(
                    !context.runs_table,
                    egui::Checkbox::new(&mut action.vary_tick, vary_label),
                )
                .on_hover_text(vary_tooltip)
                .on_disabled_hover_text("Ticks set by design table");
                if action.vary_tick && !was_varied && action.ticks_text.trim().is_empty() {
                    action.ticks_text = action.tick.to_string();
                }
                // A fixed tick has no note line. Its issue or warning follows the checkbox.
                if !varied {
                    let fallback = warning.clone().map_or(Note::Empty, Note::Warn);
                    note_label(ui, &site_note(input.summary, DraftSite::Action(position), fallback));
                }
                field
            })
            .inner;
        if varied {
            let held_back = rows.holds_back_issue(ui, &field);
            let issue = input.summary.issues_at(DraftSite::Action(position)).next();
            let preview = input.summary.tick_previews.get(position).and_then(Option::as_ref);
            let note = row_note(
                issue,
                preview,
                LevelNoun::Tick,
                context.search,
                context.draws_ranges,
                held_back,
            );
            let note = match (note, &warning) {
                (Note::Weak(_), Some(warning)) => Note::Warn(warning.clone()),
                (note, _) => note,
            };
            let tooltip = preview.filter(|_| issue.is_none()).and_then(preview_tooltip);
            if let (Some(response), Some(tooltip)) = (note_line(ui, layout.cell_width, &note), tooltip) {
                response.on_hover_text(tooltip);
            }
        }
        field
    });
    (field.labelled_by(label.id), varied)
}

/// Returns the feedback of the Replicates field for a draft in `mode` with `counts` of configurations or evaluations,
/// replicates and runs once counted, as in "15 × 3 = 45 runs".
pub fn replicates_feedback(mode: DraftMode, counts: Option<(u64, u64, u64)>) -> String {
    match (counts, mode) {
        (Some((configs, replicates, runs)), _) => {
            format!("{configs} × {replicates} = {runs} {}", plural(runs, "run"))
        }
        (None, DraftMode::Sweep) => "per configuration".to_owned(),
        (None, DraftMode::Search) => "per evaluation".to_owned(),
    }
}

/// Label, dice name and tooltip of a seed field.
#[derive(Debug, Clone, Copy)]
struct SeedField {
    label: &'static str,
    /// Name of the dice button for assistive technology.
    dice_name: &'static str,
    tooltip: &'static str,
    hint: &'static str,
    site: DraftSite,
}

/// Returns the note of a seed field: its issue, without the field's name the field's label already shows.
fn seed_note(summary: &CheckSummary, field: SeedField) -> Note {
    match site_note(summary, field.site, Note::Empty) {
        Note::Issue(kind, message) => {
            let prefix = format!("{}: ", field.label);
            let message = message
                .strip_prefix(&prefix)
                .map_or_else(|| message.clone(), str::to_owned);
            Note::Issue(kind, message)
        }
        note => note,
    }
}

/// Draws a seed text field with a dice button that fills in a random seed, and its issue after them, or on a reserved
/// line below them in a stacked form.
fn seed_row(ui: &mut egui::Ui, rows: &mut FormRows, summary: &CheckSummary, field: SeedField, text: &mut String) {
    let layout = rows.layout;
    let width = seed_field_width(ui);
    let (label, edit) = layout.row(ui, field.label, |ui| {
        let (edit, note) = ui
            .horizontal(|ui| {
                let edit = ui.add(TextEdit::singleline(text).hint_text(field.hint).desired_width(width));
                if icon_button(ui, MDI_DICE_5, field.dice_name, "Generate random seed").clicked() {
                    *text = draw_seed(text.trim().parse().ok()).to_string();
                }
                let note = if rows.holds_back_issue(ui, &edit) {
                    Note::Empty
                } else {
                    seed_note(summary, field)
                };
                if !layout.stacked {
                    note_label(ui, &note);
                }
                (edit, note)
            })
            .inner;
        if layout.stacked {
            note_line(ui, layout.cell_width, &note);
        }
        edit
    });
    let label = label.on_hover_text(field.tooltip);
    let edit = edit.on_hover_text(field.tooltip).labelled_by(label.id);
    rows.reveal_row(ui, field.site, &edit, Some(text.chars().count()));
}

fn seeds_section(ui: &mut egui::Ui, rows: &mut FormRows, draft: &mut SweepDraft, input: &FormInput<'_>) {
    let layout = rows.layout;
    let summary = input.summary;
    let replicates_tooltip = match draft.mode {
        DraftMode::Sweep => "Runs per configuration, each with its own seed",
        DraftMode::Search => "Runs per evaluation, each with its own seed",
    };
    let feedback = site_note(
        summary,
        DraftSite::Replicates,
        Note::Weak(replicates_feedback(draft.mode, summary.counts)),
    );
    let (label, replicates) = layout.row(ui, "Replicates", |ui| {
        layout.with_feedback(ui, &feedback, |ui| {
            ui.add(DragValue::new(&mut draft.replicates).range(1..=u64::from(u32::MAX)))
        })
    });
    let label = label.on_hover_text(replicates_tooltip);
    let replicates = replicates.on_hover_text(replicates_tooltip).labelled_by(label.id);
    rows.reveal_row(ui, DraftSite::Replicates, &replicates, None);

    seed_row(
        ui,
        rows,
        summary,
        SeedField {
            label: "Root seed",
            dice_name: "Generate random root seed",
            tooltip: "Every run's seed derives from this one",
            hint: "",
            site: DraftSite::Seed,
        },
        &mut draft.root_seed_text,
    );
    layout.blank_row(ui, |ui| {
        ui.checkbox(&mut draft.common_random_numbers, "Common random numbers")
            .on_hover_text(
                "Use the same seed for replicate n in every configuration, to reduce seed noise in comparisons",
            );
    });
    if draft.mode == DraftMode::Sweep && draft.design.is_sampled() {
        seed_row(
            ui,
            rows,
            summary,
            SeedField {
                label: "Design seed",
                dice_name: "Generate random design seed",
                tooltip: "Seed for drawing samples.",
                hint: "From root seed",
                site: DraftSite::DesignSeed,
            },
            &mut draft.design_seed_text,
        );
    }
}

/// Returns the feedback of the Steps field: the ticks a run steps, warm-up included, as in "1100 ticks per run".
pub fn steps_feedback(steps: u64, warmup: u64) -> String {
    let ticks = steps.saturating_add(warmup);
    format!("{ticks} {} per run", plural(ticks, "tick"))
}

fn run_length_section(ui: &mut egui::Ui, rows: &mut FormRows, draft: &mut SweepDraft, input: &FormInput<'_>) {
    let layout = rows.layout;
    let feedback = site_note(
        input.summary,
        DraftSite::RunLength,
        Note::Weak(steps_feedback(draft.steps, draft.warmup)),
    );
    let (label, steps) = layout.row(ui, "Steps", |ui| {
        layout.with_feedback(ui, &feedback, |ui| {
            ui.add(DragValue::new(&mut draft.steps).range(1..=u64::MAX))
        })
    });
    let tooltip = "Ticks measured per run, after warm-up";
    let label = label.on_hover_text(tooltip);
    let steps = steps.on_hover_text(tooltip).labelled_by(label.id);
    rows.reveal_row(ui, DraftSite::RunLength, &steps, None);
    labeled(
        ui,
        &layout,
        "Warm-up",
        "Ticks stepped before measuring",
        DragValue::new(&mut draft.warmup),
    );
    stop_rows(ui, rows, draft, input);
    timeout_rows(ui, &layout, draft);
}

/// Returns the note under a stop condition, as in "Runs will end at the first sample where Infected is at most 0,
/// from tick 0."
pub fn stop_note(stop: &StopDraft) -> String {
    format!(
        "Runs will end at the first sample where {} is {} {}, from tick {}.",
        stop.column,
        comparator_words(stop.comparator),
        crate::ui::results::plot::format_significant(stop.threshold),
        stop.min_tick
    )
}

/// Returns the text of `comparator` in a comparator combo box's list, its words and its symbol, as in "at most  <=".
pub fn comparator_item(comparator: Comparator) -> String {
    format!("{}  {}", comparator_words(comparator), comparator.as_str())
}

fn stop_rows(ui: &mut egui::Ui, rows: &mut FormRows, draft: &mut SweepDraft, input: &FormInput<'_>) {
    let layout = rows.layout;
    let entry = input.entry;
    let mut enabled = draft.stop.is_some();
    let (stop_when, ()) = layout.checkbox_row(ui, &mut enabled, "Stop when", |_| {});
    let stop_when = stop_when.on_hover_text("End a run at the first sample where this holds");
    if enabled {
        draft.add_stop(input.schema);
    } else {
        draft.clear_stop();
    }
    let Some(stop) = &mut draft.stop else {
        rows.reveal_row(ui, DraftSite::Stop, &stop_when, None);
        return;
    };
    let (label, stat) = layout.sub_row(ui, "Stat", |ui, sub| {
        stat_combo(
            ui,
            Id::new("sweep_stop_stat"),
            entry,
            &mut stop.column,
            sub.control_width(),
        )
        .on_hover_text("Stat to compare")
    });
    let stat = stat.labelled_by(label.id).labelled_by(stop_when.id);
    rows.reveal_row(ui, DraftSite::Stop, &stat, None);
    let (label, (comparator, threshold)) = layout.sub_row(ui, "Condition", |ui, _| {
        ui.horizontal(|ui| {
            let comparator = comparator_combo(ui, Id::new("sweep_stop_comparator"), &mut stop.comparator)
                .on_hover_text("Comparison with threshold");
            let threshold = ui.add(DragValue::new(&mut stop.threshold).speed(0.1));
            (comparator, threshold)
        })
        .inner
    });
    comparator.labelled_by(label.id);
    threshold.labelled_by(label.id);
    let note = site_note(input.summary, DraftSite::Stop, Note::Weak(stop_note(stop)));
    let (label, from_tick) = layout.sub_row(ui, "From tick", |ui, sub| {
        let from_tick = ui
            .add(DragValue::new(&mut stop.min_tick))
            .on_hover_text("First tick at which the condition can end a run");
        note_line(ui, sub.cell_width, &note);
        from_tick
    });
    from_tick.labelled_by(label.id);
}

fn timeout_rows(ui: &mut egui::Ui, layout: &FormLayout, draft: &mut SweepDraft) {
    let mut enabled = draft.timeout_s.is_some();
    let (timeout, ()) = layout.checkbox_row(ui, &mut enabled, "Timeout", |_| {});
    timeout.on_hover_text("End a run after a wall-clock time limit");
    match (enabled, draft.timeout_s) {
        (true, None) => draft.timeout_s = Some(DEFAULT_TIMEOUT_SECONDS),
        (false, Some(_)) => draft.timeout_s = None,
        _ => {}
    }
    let Some(seconds) = &mut draft.timeout_s else {
        return;
    };
    let (label, field) = layout.sub_row(ui, "Seconds per run", |ui, sub| {
        let field = ui.add(DragValue::new(seconds).range(0.0..=f64::MAX).suffix(" s"));
        note_line(
            ui,
            sub.cell_width,
            &Note::Weak("Whether a run times out depends on the machine's speed.".to_owned()),
        );
        field
    });
    field.labelled_by(label.id);
}

/// Returns a drag value for a number of ticks, labeled "1 tick" or "5 ticks".
fn ticks_drag_value(ticks: &mut u64) -> DragValue<'_> {
    DragValue::new(ticks)
        .custom_formatter(|value, _| {
            let count = value as u64;
            format!("{count} {}", plural(count, "tick"))
        })
        .custom_parser(|text| text.split_whitespace().next()?.parse().ok())
}

fn first_stat(entry: &ModelEntry) -> String {
    entry
        .stat_descriptors
        .first()
        .map(|stat| stat.label.to_owned())
        .unwrap_or_default()
}

fn stat_combo(ui: &mut egui::Ui, id: Id, entry: &ModelEntry, column: &mut String, width: f32) -> egui::Response {
    ComboBox::from_id_salt(id)
        .width(width)
        .truncate()
        .selected_text(column.as_str())
        .show_ui(ui, |ui| {
            for stat in &entry.stat_descriptors {
                if ui.selectable_label(column == stat.label, stat.label).clicked() {
                    stat.label.clone_into(column);
                }
            }
        })
        .response
}

fn comparator_combo(ui: &mut egui::Ui, id: Id, comparator: &mut Comparator) -> egui::Response {
    ComboBox::from_id_salt(id)
        .selected_text(comparator_words(*comparator))
        .width(COMPARATOR_WIDTH)
        .show_ui(ui, |ui| {
            for choice in COMPARATORS {
                ui.selectable_value(comparator, choice, comparator_item(choice));
            }
        })
        .response
}

/// Returns the feedback of the Sample every field: the samples a run of `steps` measured ticks takes, as in "1001
/// samples per run".
pub fn samples_feedback(steps: u64, stats_every: u64) -> String {
    match sample_count(steps, stats_every) {
        Some(samples) => format!("{samples} {} per run", plural(samples, "sample")),
        None => NOT_COUNTED.to_owned(),
    }
}

/// Returns the feedback of the Series every field for a series every `series_every` ticks that takes `bytes` once
/// counted, and a warning from half the limit while the results stay `in_memory`.
pub fn series_feedback(series_every: u64, bytes: Option<u64>, in_memory: bool) -> Note {
    if series_every == 0 {
        return Note::Weak("No series".to_owned());
    }
    let Some(bytes) = bytes else {
        return Note::Weak(NOT_COUNTED.to_owned());
    };
    if in_memory && bytes >= MAX_MEMORY_SERIES_BYTES / 2 {
        return Note::Warn(format!(
            "About {} in memory, over half the limit of {}",
            fmt_bytes(bytes),
            fmt_bytes(MAX_MEMORY_SERIES_BYTES)
        ));
    }
    Note::Weak(format!("About {} of series", fmt_bytes(bytes)))
}

/// Returns the note of an output row: the column `runs.csv` gives it, as in "Written as Susceptible:argmax".
pub fn output_note(reducer: &ReducerSpec) -> String {
    format!("Written as {}:{}", reducer.column, reducer.kind)
}

/// Returns the label of the search field of `site` that reads an output.
fn reader_label(site: DraftSite) -> &'static str {
    match site {
        DraftSite::Axis(GridAxis::X) => GridAxis::X.label(),
        DraftSite::Axis(GridAxis::Y) => GridAxis::Y.label(),
        _ => "Objective",
    }
}

/// Draws the Sample every and Series every rows, each with its feedback.
fn sampling_rows(ui: &mut egui::Ui, rows: &mut FormRows, draft: &mut SweepDraft, summary: &CheckSummary) {
    let layout = rows.layout;
    // Each Sampling issue names the field that causes it.
    let sampling_issue = summary.issues_at(DraftSite::Sampling).next().cloned();
    let (sample_note, series_note) = match &sampling_issue {
        Some(issue) if draft.stats_every == 0 => (
            Note::Issue(issue.kind, issue.message.clone()),
            series_feedback(
                draft.series_every,
                summary.series_bytes,
                draft.holds_results_in_memory(),
            ),
        ),
        Some(issue) => (
            Note::Weak(samples_feedback(draft.steps, draft.stats_every)),
            Note::Issue(issue.kind, issue.message.clone()),
        ),
        None => (
            Note::Weak(samples_feedback(draft.steps, draft.stats_every)),
            series_feedback(
                draft.series_every,
                summary.series_bytes,
                draft.holds_results_in_memory(),
            ),
        ),
    };
    let tooltip = "Ticks between stat samples. Frequent sampling slows runs, especially on the GPU.";
    let (label, sample_every) = layout.row(ui, "Sample every", |ui| {
        layout.with_feedback(ui, &sample_note, |ui| {
            ui.add(ticks_drag_value(&mut draft.stats_every).range(1..=u64::MAX))
        })
    });
    let label = label.on_hover_text(tooltip);
    let sample_every = sample_every.on_hover_text(tooltip).labelled_by(label.id);
    let tooltip = "Ticks between samples kept in the series, a multiple of Sample every. Set to 0 to keep no series.";
    let (label, series_every) = layout.row(ui, "Series every", |ui| {
        layout.with_feedback(ui, &series_note, |ui| ui.add(ticks_drag_value(&mut draft.series_every)))
    });
    let label = label.on_hover_text(tooltip);
    let series_every = series_every.on_hover_text(tooltip).labelled_by(label.id);
    let sampling_row = if draft.stats_every == 0 {
        &sample_every
    } else {
        &series_every
    };
    rows.reveal_row(ui, DraftSite::Sampling, sampling_row, None);
}

fn outputs_section(ui: &mut egui::Ui, rows: &mut FormRows, draft: &mut SweepDraft, input: &FormInput<'_>) {
    let layout = rows.layout;
    let summary = input.summary;
    let entry = input.entry;
    sampling_rows(ui, rows, draft, summary);

    let mut defaults = draft.default_reducers;
    layout.blank_row(ui, |ui| {
        ui.checkbox(&mut defaults, "Final value, minimum, maximum and mean of every stat")
            .on_hover_text("Record these four values of every stat for each run");
    });
    if defaults != draft.default_reducers {
        if defaults {
            draft.default_reducers = true;
        } else {
            draft.drop_default_outputs();
        }
    }

    let last_tick = draft.warmup.saturating_add(draft.steps);
    let readers: Vec<Option<DraftSite>> = (0..draft.reducers.len())
        .map(|position| draft.output_reader(position))
        .collect();
    let mut removed = None;
    for (position, reducer) in draft.reducers.iter_mut().enumerate() {
        let site = DraftSite::Output(position);
        let reader = readers.get(position).copied().flatten();
        let note = site_note(summary, site, Note::Weak(output_note(reducer)));
        let (label, stat) = layout.row(ui, "Output", |ui| {
            let stat = ui
                .horizontal(|ui| {
                    let spacing = ui.spacing().item_spacing.x;
                    let icon_width = icon_button_width(ui, MDI_DELETE_OUTLINE);
                    let tag = reader.map(|site| format!("Used by {}", reader_label(site)));
                    let tag_width = tag.as_deref().map_or(0.0, |tag| text_width(ui, tag) + spacing);
                    let combo_width =
                        ((layout.control_width() - icon_width - tag_width - 2.0 * spacing) / 2.0).max(40.0);
                    // A combo box grows to fit its text while the line has room. Each slot holds it to its width.
                    let stat = slot(ui, combo_width, |ui| {
                        stat_combo(
                            ui,
                            Id::new(("sweep_output_stat", position)),
                            entry,
                            &mut reducer.column,
                            combo_width,
                        )
                    });
                    slot(ui, combo_width, |ui| {
                        kind_combo(ui, position, &mut reducer.kind, last_tick, combo_width)
                    });
                    let name = format!("Remove output {}", column_output_label(&reducer.column, reducer.kind));
                    match (reader, tag) {
                        (Some(site), Some(tag)) => {
                            add_button(ui, false, Button::new(MDI_DELETE_OUTLINE).small(), &name)
                                .on_disabled_hover_text(format!(
                                    "Select another output for {} to remove this one",
                                    reader_label(site)
                                ));
                            ui.weak(tag);
                        }
                        _ => {
                            if icon_button(ui, MDI_DELETE_OUTLINE, &name, "Remove output").clicked() {
                                removed = Some(position);
                            }
                        }
                    }
                    stat
                })
                .inner;
            note_line(ui, layout.cell_width, &note);
            stat
        });
        let stat = stat.labelled_by(label.id);
        rows.reveal_row(ui, site, &stat, None);
        output_sub_rows(ui, &layout, position, &mut reducer.kind);
    }
    if let Some(position) = removed {
        draft.reducers.remove(position);
    }
    let add = add_button(ui, true, Button::new(format!("{MDI_PLUS} Add output")), "Add output")
        .on_hover_text("Record one more value of a stat for each run");
    if add.clicked() {
        draft.reducers.push(ReducerSpec {
            column: first_stat(entry),
            kind: ReducerKind::ArgMax,
        });
    }
}

/// Draws the combo box of output row `position`'s kind `width` wide. A window picked anew spans every tick up to
/// `last_tick`.
fn kind_combo(
    ui: &mut egui::Ui,
    position: usize,
    kind: &mut ReducerKind,
    last_tick: u64,
    width: f32,
) -> egui::Response {
    let mut picked = *kind;
    let response = ComboBox::from_id_salt(("sweep_output_kind", position))
        .width(width)
        .truncate()
        .selected_text(reducer_label(*kind))
        .show_ui(ui, |ui| {
            for choice in REDUCER_KINDS {
                let same = std::mem::discriminant(&choice) == std::mem::discriminant(&picked);
                if ui.selectable_label(same, reducer_label(choice)).clicked() && !same {
                    picked = match choice {
                        ReducerKind::WindowMean { .. } => ReducerKind::WindowMean {
                            start: 0,
                            end: last_tick,
                        },
                        choice => choice,
                    };
                }
            }
        })
        .response;
    *kind = picked;
    response
}

/// Draws the rows of an output kind that takes a threshold or a window, under output row `position`.
fn output_sub_rows(ui: &mut egui::Ui, layout: &FormLayout, position: usize, kind: &mut ReducerKind) {
    match kind {
        ReducerKind::FirstCrossing(comparison) => {
            let (label, (comparator, threshold)) = layout.sub_row(ui, "Crossing", |ui, _| {
                ui.horizontal(|ui| {
                    let comparator = comparator_combo(
                        ui,
                        Id::new(("sweep_output_comparator", position)),
                        &mut comparison.comparator,
                    );
                    let threshold = ui.add(DragValue::new(&mut comparison.threshold).speed(0.1));
                    (comparator, threshold)
                })
                .inner
            });
            comparator.labelled_by(label.id);
            threshold.labelled_by(label.id);
        }
        ReducerKind::WindowMean { start, end } => {
            let (label, field) = layout.sub_row(ui, "From tick", |ui, _| ui.add(DragValue::new(start)));
            field.labelled_by(label.id);
            let (label, field) = layout.sub_row(ui, "To tick", |ui, _| ui.add(DragValue::new(end)));
            field.labelled_by(label.id);
        }
        ReducerKind::Final
        | ReducerKind::Min
        | ReducerKind::Max
        | ReducerKind::Mean
        | ReducerKind::ArgMax
        | ReducerKind::ArgMin => {}
    }
}

/// Returns the feedback of the Concurrent runs field for `concurrency` on a model of `backend`, with `threads` worker
/// threads, in a browser when `browser` is set.
pub fn concurrency_feedback(concurrency: Concurrency, backend: Backend, threads: usize, browser: bool) -> String {
    if browser {
        return "One run at a time in a browser".to_owned();
    }
    match (concurrency, backend) {
        (Concurrency::Auto, _) => "Will be set at start from model size".to_owned(),
        (Concurrency::Fixed(runs), Backend::Gpu) if runs.get() == 1 => "One run on the GPU at a time".to_owned(),
        (Concurrency::Fixed(runs), Backend::Gpu) => format!("{runs} runs share the GPU"),
        (Concurrency::Fixed(runs), Backend::Cpu) => {
            let per_run = (threads / runs.get()).max(1) as u64;
            format!("About {per_run} {} per run", plural(per_run, "thread"))
        }
    }
}

fn execution_section(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    draft: &mut SweepDraft,
    input: &FormInput<'_>,
    request: &mut Option<SweepRequest>,
) {
    concurrency_row(ui, &rows.layout, draft, input.entry.metadata.backend);
    results_rows(ui, rows, draft, input, request);
}

fn concurrency_row(ui: &mut egui::Ui, layout: &FormLayout, draft: &mut SweepDraft, backend: Backend) {
    let browser = cfg!(target_arch = "wasm32");
    let threads = rayon::current_num_threads().max(1);
    let tooltip = match backend {
        Backend::Gpu => {
            "Runs on the GPU at the same time. Each needs its own device memory. A large model runs fastest alone."
        }
        Backend::Cpu => "Runs stepped at the same time. Each needs its own memory. A large model runs fastest alone.",
    };
    let feedback = Note::Weak(concurrency_feedback(draft.concurrency, backend, threads, browser));
    let (label, count) = layout.row(ui, "Concurrent runs", |ui| {
        layout.with_feedback(ui, &feedback, |ui| {
            ui.add_enabled_ui(!browser, |ui| {
                let mut fixed = matches!(draft.concurrency, Concurrency::Fixed(_));
                let choices = [
                    (false, "Auto", "Set number of runs at start from model size"),
                    (true, "Fixed", "Step this many runs at once"),
                ];
                if segmented(ui, &mut fixed, &choices, |_| Ok(())) {
                    draft.concurrency = if fixed {
                        Concurrency::Fixed(NonZeroUsize::MIN)
                    } else {
                        Concurrency::Auto
                    };
                }
                let mut runs = match draft.concurrency {
                    Concurrency::Fixed(runs) if !browser => runs.get(),
                    _ => 1,
                };
                // A loaded count above the thread count stays until the field is dragged.
                let field = ui.add_enabled(
                    fixed && !browser,
                    DragValue::new(&mut runs)
                        .range(1..=threads)
                        .clamp_existing_to_range(false),
                );
                if field.changed()
                    && let Some(runs) = NonZeroUsize::new(runs)
                {
                    draft.concurrency = Concurrency::Fixed(runs);
                }
                field
            })
            .inner
        })
    });
    let label = label.on_hover_text(tooltip);
    count.labelled_by(label.id);
}

fn results_rows(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    draft: &mut SweepDraft,
    input: &FormInput<'_>,
    request: &mut Option<SweepRequest>,
) {
    let layout = rows.layout;
    let browser = cfg!(target_arch = "wasm32");
    let summary = input.summary;
    let in_folder = draft.results_in_folder && !browser;
    let results_fallback = if browser {
        Note::Weak("A browser keeps results in memory. Save them once the sweep ends.".to_owned())
    } else {
        Note::Empty
    };
    let results_note = site_note(summary, DraftSite::Execution, results_fallback);
    let mut picked = in_folder;
    let (label, memory) = layout.row(ui, "Results", |ui| {
        let memory = ui
            .horizontal(|ui| {
                let memory = ui
                    .radio_value(&mut picked, false, "In memory")
                    .on_hover_text("Keep results in memory until saved. Starting another sweep will replace them.");
                let folder = ui
                    .add_enabled(!browser, RadioButton::new(picked, "In a folder"))
                    .on_hover_text("Write results to a folder as runs finish")
                    .on_disabled_hover_text("Unavailable in a browser. Use the desktop app.");
                if folder.clicked() {
                    picked = true;
                }
                memory
            })
            .inner;
        // Results in a folder report their issues on the Folder row below.
        if !in_folder {
            note_line(ui, layout.cell_width, &results_note);
        }
        memory
    });
    let memory = memory.labelled_by(label.id);
    if picked != in_folder {
        draft.results_in_folder = picked;
        if picked && draft.output_dir_text.trim().is_empty() {
            *request = Some(SweepRequest::ChooseFolder);
        }
    }
    if !draft.results_in_folder || browser {
        rows.reveal_row(ui, DraftSite::Execution, &memory, None);
        return;
    }
    folder_row(ui, rows, draft, input, request);
}

fn folder_row(
    ui: &mut egui::Ui,
    rows: &mut FormRows,
    draft: &mut SweepDraft,
    input: &FormInput<'_>,
    request: &mut Option<SweepRequest>,
) {
    let layout = rows.layout;
    let resumable = input.summary.folder_results.is_some_and(|results| results.resumable);
    let (label, field) = layout.row(ui, "Folder", |ui| {
        let field = ui
            .horizontal(|ui| {
                let choose_text = format!("{MDI_FOLDER_OUTLINE} Select");
                let choose_width =
                    text_width(ui, &choose_text) + 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x;
                let field = ui
                    .add(
                        TextEdit::singleline(&mut draft.output_dir_text)
                            .hint_text("Folder without results")
                            .desired_width((layout.control_width() - choose_width).max(0.0)),
                    )
                    .on_hover_text("Results will be written to this folder as runs finish");
                let choose = add_button(ui, true, Button::new(choose_text), "Select")
                    .on_hover_text("Select folder without results");
                if choose.clicked() {
                    *request = Some(SweepRequest::ChooseFolder);
                }
                field
            })
            .inner;
        let held_back = rows.holds_back_issue(ui, &field);
        let full_path = draft.output_dir_text.trim();
        let fallback = if full_path.is_empty() {
            Note::Empty
        } else {
            Note::Weak(format!("Results will go to {full_path}"))
        };
        let note = match site_note(input.summary, DraftSite::Execution, fallback.clone()) {
            Note::Issue(IssueKind::Invalid, _) if held_back => fallback,
            note => note,
        };
        let height = ui.text_style_height(&TextStyle::Body);
        ui.allocate_ui_with_layout(
            vec2(layout.cell_width, height),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_size(vec2(layout.cell_width, height));
                let (_, open) = Sides::new().shrink_left().truncate().show(
                    ui,
                    |ui| note_label(ui, &note),
                    |ui| {
                        resumable
                            && ui
                                .small_button("Open in Results")
                                .on_hover_text("Open these results. Press Resume sweep there to finish them.")
                                .clicked()
                    },
                );
                if open {
                    *request = Some(SweepRequest::OpenFolderResults);
                }
            },
        );
        field
    });
    let field = field.labelled_by(label.id);
    rows.reveal_row(
        ui,
        DraftSite::Execution,
        &field,
        Some(draft.output_dir_text.chars().count()),
    );
}

/// Returns `issue` as a line of the tab's problem lists, led by the row or the section it belongs to.
///
/// A message that starts with that name already, such as "Root seed: ...", is left as it is.
pub(super) fn banner_line(issue: &DraftIssue, draft: &SweepDraft, schema: &ModelSchema<'_>) -> String {
    let site_name = match issue.site {
        // These issues name the field they belong to.
        DraftSite::Sweep
        | DraftSite::Search
        | DraftSite::Parameters
        | DraftSite::Sampling
        | DraftSite::MethodSetting(_) => None,
        DraftSite::Factor(index) => schema.params.get(index).map(|descriptor| descriptor.label.to_owned()),
        DraftSite::Action(position) => draft
            .actions
            .get(position)
            .filter(|action| action.action_index < schema.actions.len())
            .map(|action| action.label(schema)),
        DraftSite::Output(position) => draft
            .reducers
            .get(position)
            .map(|reducer| column_output_label(&reducer.column, reducer.kind)),
        DraftSite::Design => Some("Design".to_owned()),
        DraftSite::DesignSeed => Some("Design seed".to_owned()),
        DraftSite::Objective => Some("Objective".to_owned()),
        DraftSite::Axis(axis) => Some(axis.label().to_owned()),
        DraftSite::Replicates => Some("Replicates".to_owned()),
        DraftSite::Seed => Some("Root seed".to_owned()),
        DraftSite::RunLength => Some("Run length".to_owned()),
        DraftSite::Stop => Some("Stop when".to_owned()),
        DraftSite::Execution if draft.results_in_folder => Some("Folder".to_owned()),
        DraftSite::Execution => Some("Results".to_owned()),
    };
    match site_name {
        Some(name) if !issue.message.starts_with(&name) => format!("{name}: {}", issue.message),
        _ => issue.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use henad_core::explore::reducer::{ReducerKind, ReducerSpec};
    use henad_core::explore::stop::Comparator;
    use henad_core::metadata::Backend;
    use henad_core::params::ParamValue;
    use henad_explore::exec::Concurrency;
    use henad_explore::schema::model_schema;
    use henad_models::registry::{ModelEntry, model_registry};

    use super::{
        SectionSummaries, SeedField, banner_line, comparator_item, concurrency_feedback, design_formula, output_note,
        replicates_feedback, samples_feedback, seed_note, series_feedback, steps_feedback, stop_note, table_note,
    };
    use crate::ui::sweep::draft::{
        DesignTableDraft, DraftDesign, DraftIssue, DraftMode, DraftSite, GridAxis, IssueKind, MAX_MEMORY_SERIES_BYTES,
        StopDraft, SweepDraft,
    };
    use crate::ui::sweep::layout::Note;
    use crate::ui::sweep::{CheckSummary, SweepPanel};

    // Indices of SIR's parameters.
    const INFECTION_RATE: usize = 2;
    const RECOVERY_RATE: usize = 3;

    fn sir() -> ModelEntry {
        model_registry(None)
            .into_iter()
            .find(|entry| entry.id == "sir")
            .expect("SIR is registered")
    }

    fn default_values(entry: &ModelEntry) -> Vec<ParamValue> {
        entry
            .param_descriptors
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect()
    }

    /// Returns the section summaries of the draft that `edit` makes of a new SIR draft.
    fn summaries_of(edit: impl FnOnce(&mut SweepDraft)) -> SectionSummaries {
        let entry = sir();
        let schema = model_schema(&entry);
        let panel_values = default_values(&entry);
        let mut panel = SweepPanel::default();
        edit(panel.draft_mut(&schema));
        let check = panel.cached_check(&schema, &panel_values);
        let summary = CheckSummary::new(check, &entry, &schema);
        SectionSummaries::new(check, &schema, &summary)
    }

    /// Returns the formula under the Design field of the draft that `edit` makes of a new SIR draft.
    fn formula_of(edit: impl FnOnce(&mut SweepDraft)) -> String {
        let entry = sir();
        let schema = model_schema(&entry);
        let mut draft = SweepDraft::new(&schema);
        edit(&mut draft);
        let planned = draft
            .check(&schema, &default_values(&entry))
            .unwrap_or_else(|issues| panic!("the draft does not plan: {issues:?}"));
        let configs = planned.counts().0;
        let (param_previews, tick_previews) = draft.level_previews(&schema);
        design_formula(&draft, &schema, &param_previews, &tick_previews, configs)
    }

    fn vary(draft: &mut SweepDraft, index: usize, text: &str) {
        draft.factors[index].vary = true;
        text.clone_into(&mut draft.factors[index].levels_text);
    }

    #[test]
    fn a_new_draft_summarizes_its_defaults() {
        let summaries = summaries_of(|_| {});
        assert_eq!(summaries.design, "Every combination · 1 configuration");
        assert_eq!(summaries.parameters, "None varied");
        assert_eq!(summaries.seeds, "1 replicate · root 0 · common random numbers");
        assert_eq!(summaries.run_length, "1000 steps");
        assert_eq!(summaries.outputs, "4 per stat · a sample every tick");
        assert_eq!(summaries.execution, "Auto · results in memory");
        assert_eq!(summaries.plan, "1 run");
        assert_eq!(summaries.search, "Random search · 200 evaluations");
    }

    #[test]
    fn a_summary_names_what_its_section_changes() {
        let summaries = summaries_of(|draft| {
            draft.design = DraftDesign::LatinHypercube;
            draft.design_seed_text = "7".to_owned();
            vary(draft, INFECTION_RATE, "0.1:0.5");
            draft.replicates = 3;
            draft.common_random_numbers = false;
            draft.warmup = 100;
            draft.stop = Some(StopDraft {
                column: "Infected".to_owned(),
                comparator: Comparator::LessOrEqual,
                threshold: 0.0,
                min_tick: 0,
            });
            draft.timeout_s = Some(600.0);
            draft.stats_every = 10;
            draft.series_every = 50;
            draft.concurrency = Concurrency::Fixed(NonZeroUsize::MIN.saturating_add(3));
            draft.results_in_folder = true;
            draft.output_dir_text = "/tmp/sweep-01".to_owned();
        });
        assert_eq!(summaries.design, "Latin hypercube · 20 samples");
        assert_eq!(summaries.parameters, "1 of 5 varied");
        assert_eq!(
            summaries.seeds,
            "3 replicates · root 0 · independent seeds · design seed 7"
        );
        assert_eq!(
            summaries.run_length,
            "1000 steps, 100 warm-up · stops when Infected is at most 0 · 600 s timeout"
        );
        assert_eq!(
            summaries.outputs,
            "4 per stat · a sample every 10 ticks, series every 50"
        );
        assert_eq!(summaries.execution, "4 at once · results to sweep-01");
        assert_eq!(summaries.plan, "60 runs");

        let unchosen = summaries_of(|draft| draft.results_in_folder = true);
        assert_eq!(unchosen.execution, "Auto · no folder selected");
        assert_eq!(unchosen.plan, "Unknown", "a folder still to choose blocks the plan");
    }

    #[test]
    fn a_design_table_and_a_search_summarize_their_own_settings() {
        let table = summaries_of(|draft| {
            draft.design = DraftDesign::Table;
            draft.table = Some(DesignTableDraft {
                file_name: "sir-design.csv".to_owned(),
                text: "infection_rate\n0.1\n0.2\n".to_owned(),
            });
            draft.default_reducers = false;
            draft.series_every = 0;
        });
        assert_eq!(table.design, "Design table · 2 rows");
        assert_eq!(table.parameters, "Set by design table");
        assert_eq!(table.outputs, "None · a sample every tick · no series");

        let search = summaries_of(|draft| {
            draft.mode = DraftMode::Search;
            draft.factors[2].vary = true;
        });
        assert_eq!(search.parameters, "1 of 5 searched");
    }

    #[test]
    fn the_design_formula_shows_how_each_design_counts_its_configurations() {
        assert_eq!(formula_of(|_| {}), "Nothing varies, so 1 configuration will run.");
        let both = |draft: &mut SweepDraft| {
            vary(draft, INFECTION_RATE, "0.1:0.5:0.1");
            vary(draft, RECOVERY_RATE, "0.02, 0.05, 0.1");
        };
        assert_eq!(
            formula_of(both),
            "Infection Rate (5) × Recovery Rate (3) = 15 configurations"
        );
        assert_eq!(
            formula_of(|draft| {
                both(draft);
                draft.design = DraftDesign::VaryEachAlone;
            }),
            "5 + 3 = 8 configurations"
        );
        assert_eq!(
            formula_of(|draft| {
                vary(draft, INFECTION_RATE, "0.1, 0.2, 0.3");
                vary(draft, RECOVERY_RATE, "0.02, 0.05, 0.1");
                draft.design = DraftDesign::Zip;
            }),
            "3 configurations. Use lists of equal length."
        );
        assert_eq!(
            formula_of(|draft| {
                vary(draft, INFECTION_RATE, "0.1:0.5");
                vary(draft, RECOVERY_RATE, "0.02:0.1");
                draft.design = DraftDesign::LatinHypercube;
            }),
            "20 configurations from 2 ranges"
        );
        assert_eq!(
            formula_of(|draft| {
                draft.design = DraftDesign::Table;
                draft.table = Some(DesignTableDraft {
                    file_name: "sir-design.csv".to_owned(),
                    text: "infection_rate\n0.1\n0.2\n0.3\n".to_owned(),
                });
            }),
            "3 configurations, one per row"
        );
        let entry = sir();
        let schema = model_schema(&entry);
        assert_eq!(
            formula_of(|draft| {
                vary(draft, INFECTION_RATE, "0.1, 0.2");
                draft.add_action(&schema, 0, 10);
                draft.actions[0].vary_tick = true;
                draft.actions[0].ticks_text = "10, 20, 30".to_owned();
            }),
            "Infection Rate (2) × Seed outbreak tick (3) = 6 configurations"
        );
    }

    #[test]
    fn a_table_names_the_parameters_and_ticks_its_columns_set() {
        let entry = sir();
        let schema = model_schema(&entry);
        let mut draft = SweepDraft::new(&schema);
        draft.add_action(&schema, 0, 10);
        let table = |text: &str| DesignTableDraft {
            file_name: "sir-design.csv".to_owned(),
            text: text.to_owned(),
        };
        assert_eq!(
            table_note(&table("infection_rate,recovery_rate\n0.1,0.2\n"), &draft, &schema),
            "Sets Infection Rate and Recovery Rate"
        );
        assert_eq!(
            table_note(
                &table("infection_rate,recovery_rate,action.seed_outbreak\n0.1,0.2,5\n"),
                &draft,
                &schema
            ),
            "Sets Infection Rate, Recovery Rate and Seed outbreak tick"
        );
        assert_eq!(
            table_note(&table("nothing\n1\n"), &draft, &schema),
            "Sets no parameter of this model"
        );
    }

    #[test]
    fn each_field_says_what_its_value_means_for_the_runs() {
        assert_eq!(
            replicates_feedback(DraftMode::Sweep, Some((15, 3, 45))),
            "15 × 3 = 45 runs"
        );
        assert_eq!(
            replicates_feedback(DraftMode::Search, Some((200, 3, 600))),
            "200 × 3 = 600 runs"
        );
        assert_eq!(replicates_feedback(DraftMode::Sweep, None), "per configuration");
        assert_eq!(replicates_feedback(DraftMode::Search, None), "per evaluation");
        assert_eq!(steps_feedback(1000, 100), "1100 ticks per run");
        assert_eq!(steps_feedback(1, 0), "1 tick per run");
        assert_eq!(samples_feedback(1000, 1), "1001 samples per run");
        assert_eq!(samples_feedback(1000, 10), "101 samples per run");
        assert_eq!(series_feedback(0, None, true), Note::Weak("No series".to_owned()));
        assert_eq!(
            series_feedback(1, Some(1_677_722), true),
            Note::Weak("About 1.6 MB of series".to_owned())
        );
        assert!(
            matches!(
                series_feedback(1, Some(MAX_MEMORY_SERIES_BYTES / 2), true),
                Note::Warn(_)
            ),
            "half the memory limit warns"
        );
        assert_eq!(
            series_feedback(1, Some(MAX_MEMORY_SERIES_BYTES / 2), false),
            Note::Weak(format!(
                "About {} of series",
                henad_core::helpers::fmt_bytes(MAX_MEMORY_SERIES_BYTES / 2)
            )),
            "a folder holds any series"
        );
    }

    #[test]
    fn concurrency_feedback_names_what_each_run_gets() {
        let fixed = |runs: usize| Concurrency::Fixed(NonZeroUsize::new(runs).expect("runs are not zero"));
        assert_eq!(
            concurrency_feedback(Concurrency::Auto, Backend::Cpu, 12, false),
            "Will be set at start from model size"
        );
        assert_eq!(
            concurrency_feedback(fixed(4), Backend::Cpu, 12, false),
            "About 3 threads per run"
        );
        assert_eq!(
            concurrency_feedback(fixed(24), Backend::Cpu, 12, false),
            "About 1 thread per run"
        );
        assert_eq!(
            concurrency_feedback(fixed(4), Backend::Gpu, 12, false),
            "4 runs share the GPU"
        );
        assert_eq!(
            concurrency_feedback(fixed(4), Backend::Cpu, 12, true),
            "One run at a time in a browser"
        );
    }

    #[test]
    fn a_stop_condition_and_an_output_read_as_sentences() {
        let stop = StopDraft {
            column: "Infected".to_owned(),
            comparator: Comparator::LessOrEqual,
            threshold: 0.0,
            min_tick: 0,
        };
        assert_eq!(
            stop_note(&stop),
            "Runs will end at the first sample where Infected is at most 0, from tick 0."
        );
        assert_eq!(comparator_item(Comparator::LessOrEqual), "at most  <=");
        assert_eq!(comparator_item(Comparator::NotEqual), "not equal to  !=");
        let output = ReducerSpec {
            column: "Susceptible".to_owned(),
            kind: ReducerKind::ArgMax,
        };
        assert_eq!(output_note(&output), "Written as Susceptible:argmax");
    }

    #[test]
    fn a_banner_line_names_the_row_its_issue_belongs_to() {
        let sir = sir();
        let schema = model_schema(&sir);
        let mut draft = SweepDraft::new(&schema);
        draft.add_action(&schema, 0, 10);
        let infection = schema
            .params
            .iter()
            .position(|descriptor| descriptor.id == "infection_rate")
            .expect("SIR has an infection rate");
        let line = |draft: &SweepDraft, site, message: &str| {
            let issue = DraftIssue::new(site, message);
            banner_line(&issue, draft, &schema)
        };
        assert_eq!(
            line(&draft, DraftSite::Factor(infection), "No values"),
            "Infection Rate: No values"
        );
        assert_eq!(
            line(&draft, DraftSite::Action(0), "No values"),
            "Seed outbreak: No values"
        );
        draft.add_action(&schema, 0, 20);
        assert_eq!(
            line(&draft, DraftSite::Action(1), "No values"),
            "Seed outbreak 2: No values",
            "a repeat of an action goes by its number"
        );
        draft.reducers.push(ReducerSpec {
            column: "Susceptible".to_owned(),
            kind: ReducerKind::ArgMax,
        });
        assert_eq!(
            line(&draft, DraftSite::Output(0), "No values"),
            "Susceptible, tick of maximum: No values",
            "an output goes by its stat first"
        );
        assert_eq!(
            line(&draft, DraftSite::Seed, "Root seed: 'x' is not a number"),
            "Root seed: 'x' is not a number"
        );
        assert_eq!(line(&draft, DraftSite::Sweep, "Too many runs"), "Too many runs");
        assert_eq!(
            line(&draft, DraftSite::Axis(GridAxis::Y), "Set Minimum and Maximum"),
            "Y axis: Set Minimum and Maximum"
        );
        assert_eq!(
            line(&draft, DraftSite::Execution, "Series take about 5 GB"),
            "Results: Series take about 5 GB"
        );
        draft.results_in_folder = true;
        assert_eq!(
            line(&draft, DraftSite::Execution, "Select a folder"),
            "Folder: Select a folder"
        );
    }

    #[test]
    fn a_seed_issue_beside_its_field_leaves_out_the_field_name() {
        let field = SeedField {
            label: "Root seed",
            dice_name: "Generate random root seed",
            tooltip: "",
            hint: "",
            site: DraftSite::Seed,
        };
        let mut summary = CheckSummary::default();
        assert_eq!(seed_note(&summary, field), Note::Empty);
        summary.issues.push(DraftIssue::new(
            DraftSite::Seed,
            "Root seed: 'x' is not an integer from 0 to 18446744073709551615",
        ));
        assert_eq!(
            seed_note(&summary, field),
            Note::Issue(
                IssueKind::Invalid,
                "'x' is not an integer from 0 to 18446744073709551615".to_owned()
            )
        );
        let design_seed = SeedField {
            label: "Design seed",
            site: DraftSite::DesignSeed,
            ..field
        };
        assert_eq!(
            seed_note(&summary, design_seed),
            Note::Empty,
            "another seed's issue stays on its own row"
        );
    }
}
