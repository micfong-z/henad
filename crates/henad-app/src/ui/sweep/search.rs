//! Search section of the Sweep tab: the method, its objective or its grid of two outputs, its budget, and the settings
//! of the method in a nested section.
//!
//! The Objective and axis lists offer every output the runs record, then the common outputs they do not record yet.
//! Picking one of those adds it to the Outputs section, where the search's use of it locks its row.

use egui::{Button, Checkbox, ComboBox, DragValue, Id, Label, Response, RichText, Ui, vec2};
use henad_core::explore::plan::ModelSchema;
use henad_core::explore::search::genetic::GeneticSettings;
use henad_core::explore::search::hill_climb::HillClimbSettings;
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::search::{Aggregate, Goal};

use crate::ui::plural;
use crate::ui::results::plot::format_significant;
use crate::ui::results::store::{ResultsStore, output_label};
use crate::ui::sweep::CheckSummary;
use crate::ui::sweep::builder::FormInput;
use crate::ui::sweep::draft::{
    AXIS_RANGE_MISSING, DraftAlgorithm, DraftIssue, DraftSite, GridAxis, INITIAL_SAMPLES_KEY, IssueKind,
    MAX_DRAFT_RUNS, SearchDraft, SweepDraft, batch_estimate, generation_estimate,
};
use crate::ui::sweep::layout::{
    FormLayout, FormRows, IssueCount, Note, NoteButton, SectionHeading, checkbox_indent, nested_section, note_block,
    note_line_with_button, slot, text_width,
};

/// Most candidates the Batch size field takes.
const MAX_BATCH_SIZE: usize = 1 << 16;

/// Most members the Population field takes.
const MAX_POPULATION: usize = 1 << 16;

/// Most cells the Cells field of a grid axis takes.
const MAX_AXIS_CELLS: u32 = 256;

/// Width of an axis's Minimum and Maximum fields, in points.
const AXIS_BOUND_WIDTH: f32 = 72.0;

/// Width of the control cell below which an axis's Minimum, Maximum and Cells stack as rows, in points.
pub const AXIS_FIELDS_STACK_BELOW: f32 = 340.0;

/// Salt of the Method settings section's id.
const METHOD_SETTINGS_ID: &str = "henad_sweep_method_settings";

/// Key of the population setting. Its row sits beside the budget, outside Method settings.
const POPULATION_KEY: &str = "genetic.population";

/// Note under the axes while either still spans 0 to 0.
pub const AXES_UNSET: &str =
    "Set Minimum and Maximum for each axis. Values outside the range will be counted in the edge cells.";

/// Caption in the Objective and axis lists over the outputs the runs do not record yet.
const UNRECORDED_CAPTION: &str = "Not recorded yet. Selecting one will add it to Outputs.";

/// Button that records the outputs a loaded search reads and its runs do not record.
const ADD_TO_OUTPUTS: NoteButton<'static> = NoteButton {
    text: "Add to Outputs",
    tooltip: "Record output for each run",
};

const OBJECTIVE_TOOLTIP: &str = "Output to optimize";

const AXIS_TOOLTIP: &str = "Output along this axis of the grid";

const AUTOMATIC_RANGE_TOOLTIP: &str = "Set range from outputs of initial samples";

/// Tooltip of the Across replicates combo box of an objective.
const OBJECTIVE_AGGREGATE_TOOLTIP: &str =
    "Combine replicates of each configuration into one value. Failed runs count as the worst value.";

/// Tooltip of the Across replicates combo box of a Pattern Space Exploration.
const PATTERN_AGGREGATE_TOOLTIP: &str =
    "Combine replicates of each configuration into one point. Failed runs are excluded.";

const STEP_SIZE_TOOLTIP: &str = "Largest change to a parameter, as a fraction of its range";

/// Returns the name the Sweep and Results tabs give `algorithm`.
pub fn algorithm_label(algorithm: DraftAlgorithm) -> &'static str {
    match algorithm {
        DraftAlgorithm::Random => "Random search",
        DraftAlgorithm::HillClimb => "Hill climbing",
        DraftAlgorithm::Genetic => "Genetic algorithm",
        DraftAlgorithm::PatternSpace => "Pattern Space Exploration",
    }
}

fn algorithm_tooltip(algorithm: DraftAlgorithm) -> &'static str {
    match algorithm {
        DraftAlgorithm::Random => "Try random configurations and keep the best",
        DraftAlgorithm::HillClimb => "Move to the best nearby configuration and restart when progress stops",
        DraftAlgorithm::Genetic => "Breed a population of configurations over generations",
        DraftAlgorithm::PatternSpace => "Find configurations that fill empty cells in a grid of two outputs",
    }
}

/// Returns the line under the Method field that says what `algorithm` does.
pub fn algorithm_description(algorithm: DraftAlgorithm) -> &'static str {
    match algorithm {
        DraftAlgorithm::Random => "Tries random configurations and keeps the best.",
        DraftAlgorithm::HillClimb => {
            "Moves to the best nearby configuration and restarts from a random configuration when progress stops."
        }
        DraftAlgorithm::Genetic => "Breeds a population of configurations over generations.",
        DraftAlgorithm::PatternSpace => "Finds configurations that fill empty cells in a grid of two outputs.",
    }
}

/// Returns the labels of the rows Method settings holds for `algorithm`.
fn method_setting_labels(algorithm: DraftAlgorithm) -> &'static [&'static str] {
    match algorithm {
        DraftAlgorithm::Random => &[],
        DraftAlgorithm::HillClimb => &["Step size", "Patience"],
        DraftAlgorithm::Genetic => &[
            "Elites",
            "Tournament size",
            "Crossover rate",
            "Mutation rate",
            "Step size",
            "Re-evaluation rate",
        ],
        DraftAlgorithm::PatternSpace => &["Initial samples", "Step size"],
    }
}

/// Returns each label of the rows the Search section shows for `algorithm`, paired with the indent before it.
///
/// The rows of Method settings count while it is collapsed too, so opening it moves no column.
pub fn search_labels(ui: &Ui, algorithm: DraftAlgorithm) -> Vec<(&'static str, f32)> {
    let mut labels = vec![("Method", 0.0)];
    if algorithm.has_objective() {
        labels.extend([("Objective", 0.0), ("Goal", 0.0)]);
    } else {
        labels.extend([(GridAxis::X.label(), 0.0), (GridAxis::Y.label(), 0.0)]);
    }
    labels.extend([("Across replicates", 0.0), ("Evaluations", 0.0), ("Batch size", 0.0)]);
    if algorithm == DraftAlgorithm::Genetic {
        labels.push(("Population", 0.0));
    }
    let indent = ui.spacing().indent;
    labels.extend(method_setting_labels(algorithm).iter().map(|&label| (label, indent)));
    labels
}

/// Returns the feedback of the Evaluations field: the runs its budget takes, as in "× 3 replicates = 600 runs".
pub fn evaluations_feedback(evaluations: u64, replicates: u64) -> String {
    let runs = evaluations.saturating_mul(replicates);
    format!(
        "× {replicates} {} = {runs} {}",
        plural(replicates, "replicate"),
        plural(runs, "run")
    )
}

/// Returns the feedback of the Batch size field: the batches the budget of `search` takes, as in "About 13 batches".
pub fn batches_feedback(search: &SearchDraft) -> String {
    let genetic = (search.algorithm == DraftAlgorithm::Genetic).then_some(&search.genetic);
    let batches = batch_estimate(search.max_evaluations, search.batch_size, genetic);
    let noun = if batches == 1 { "batch" } else { "batches" };
    format!("About {batches} {noun}")
}

/// Returns the feedback of the Population field: the generations `max_evaluations` take, as in "About 6
/// generations", or a warning for a budget smaller than one generation.
pub fn population_feedback(max_evaluations: u64, settings: &GeneticSettings) -> Note {
    match generation_estimate(max_evaluations, settings) {
        Some(generations) => Note::Weak(format!("About {generations} {}", plural(generations, "generation"))),
        None => Note::Warn("Fewer evaluations than one generation".to_owned()),
    }
}

/// Returns the feedback of the Initial samples field: the share of `max_evaluations` drawn at random, as in "64 of
/// 200 evaluations drawn at random", or a warning once it covers every evaluation.
pub fn initial_samples_feedback(initial_samples: u64, max_evaluations: u64) -> Note {
    if initial_samples >= max_evaluations {
        return Note::Warn("Initial samples cover every evaluation. The search will be entirely random.".to_owned());
    }
    Note::Weak(format!(
        "{initial_samples} of {max_evaluations} {} drawn at random",
        plural(max_evaluations, "evaluation")
    ))
}

/// Returns the summary of the Method settings section of `search`, as in "Step size 0.1 · patience 5".
pub fn method_settings_summary(search: &SearchDraft) -> String {
    let number = format_significant;
    match search.algorithm {
        DraftAlgorithm::Random => String::new(),
        DraftAlgorithm::HillClimb => {
            let settings = &search.hill_climb;
            let mut text = format!(
                "Step size {} · patience {}",
                number(settings.mutation_scale),
                settings.patience
            );
            if settings.reevaluate {
                text.push_str(" · re-evaluate best");
            }
            text
        }
        DraftAlgorithm::Genetic => {
            let settings = &search.genetic;
            format!(
                "Elites {} · tournament size {} · crossover {} · mutation {} · step size {} · re-evaluation {}",
                settings.elite_count,
                settings.tournament_size,
                number(settings.crossover_rate),
                number(settings.mutation_rate),
                number(settings.mutation_scale),
                number(settings.reevaluate_fraction)
            )
        }
        DraftAlgorithm::PatternSpace => {
            let settings = &search.pattern_space;
            format!(
                "{} initial {} · step size {}",
                settings.initial_samples,
                plural(settings.initial_samples, "sample"),
                number(settings.mutation_scale)
            )
        }
    }
}

/// Returns the note under the axes of `settings`: the cells of the grid, or the issues of both axes at once.
///
/// `issues` are the issues of the two axis rows. Invalid input comes first, each issue led by its axis. An axis still
/// spanning 0 to 0 is missing input, and [`AXES_UNSET`] asks for the range of both.
pub fn axes_note(settings: &PatternSpaceSettings, issues: &[&DraftIssue]) -> Note {
    // Each issue is led by its axis, unless its message names the axis already.
    let lines = |keep: &dyn Fn(&DraftIssue) -> bool| -> String {
        let lines: Vec<String> = issues
            .iter()
            .filter(|issue| keep(issue))
            .map(|issue| match issue.site {
                DraftSite::Axis(axis) if !issue.message.starts_with(axis.label()) => {
                    format!("{}: {}", axis.label(), issue.message)
                }
                _ => issue.message.clone(),
            })
            .collect();
        lines.join(". ")
    };
    let invalid = lines(&|issue| issue.kind == IssueKind::Invalid);
    if !invalid.is_empty() {
        return Note::Issue(IssueKind::Invalid, invalid);
    }
    let missing = lines(&|issue| issue.kind == IssueKind::Missing && issue.message != AXIS_RANGE_MISSING);
    if !missing.is_empty() {
        return Note::Issue(IssueKind::Missing, missing);
    }
    if issues.iter().any(|issue| issue.message == AXIS_RANGE_MISSING) {
        return Note::Issue(IssueKind::Missing, AXES_UNSET.to_owned());
    }
    let (columns, rows) = (settings.x_axis.cells, settings.y_axis.cells);
    let cells = u64::from(columns) * u64::from(rows);
    Note::Weak(format!("{columns} × {rows} = {cells} {}", plural(cells, "cell")))
}

/// Reads a bound of an axis as its field writes it, digits grouped by commas.
///
/// Spaces are ignored, and the minus sign U+2212 reads as a hyphen.
pub fn parse_bound(text: &str) -> Option<f64> {
    let text: String = text
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ',')
        .map(|character| if character == '\u{2212}' { '-' } else { character })
        .collect();
    text.parse().ok()
}

/// Output columns the Objective and axis lists offer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputChoices {
    /// Columns the runs record, as in `Infected:max`.
    pub recorded: Vec<String>,
    /// Columns of each stat the runs do not record yet. Picking one adds it to the Outputs section.
    pub unrecorded: Vec<String>,
}

impl OutputChoices {
    /// Returns the outputs `draft` offers, a draft of `schema`'s model.
    pub fn of(draft: &SweepDraft, schema: &ModelSchema<'_>) -> Self {
        Self {
            recorded: draft.output_names(schema),
            unrecorded: draft.unrecorded_outputs(schema),
        }
    }
}

/// Draws the Search section: the method, the objective or the axes, the budget and the method's settings.
pub(super) fn search_section(ui: &mut Ui, rows: &mut FormRows, draft: &mut SweepDraft, input: &FormInput<'_>) {
    let schema = input.schema;
    let summary = input.summary;
    let choices = OutputChoices::of(draft, schema);
    // Outputs a loaded search reads that its runs do not record, for the button that adds them.
    let unrecorded: Vec<String> = draft
        .search
        .watched_columns()
        .into_iter()
        .filter(|(_, column)| !column.is_empty() && !draft.records_output(column, schema))
        .map(|(_, column)| column.to_owned())
        .collect();
    let replicates = draft.replicates;
    let mut added: Vec<String> = Vec::new();
    let search = &mut draft.search;

    method_row(ui, rows, search, summary);
    let lists = OutputLists {
        choices: &choices,
        unrecorded: &unrecorded,
    };
    if search.algorithm.has_objective() {
        objective_rows(ui, rows, search, &lists, summary, &mut added);
    } else {
        let issues: Vec<&DraftIssue> = summary
            .issues
            .iter()
            .filter(|issue| matches!(issue.site, DraftSite::Axis(_)))
            .collect();
        let context = AxisContext {
            lists,
            issues,
            results: input.results,
        };
        axis_rows(ui, rows, search, &context, &mut added);
    }
    budget_rows(ui, rows, search, replicates, summary);
    method_settings(ui, rows, search, summary);
    for column in added {
        draft.add_watched_output(&column);
    }
}

/// Outputs the Objective and axis rows offer, and those a loaded search reads that its runs do not record.
struct OutputLists<'a> {
    choices: &'a OutputChoices,
    unrecorded: &'a [String],
}

fn method_row(ui: &mut Ui, rows: &mut FormRows, search: &mut SearchDraft, summary: &CheckSummary) {
    let layout = rows.layout;
    // An issue of the search as a whole takes the place of the description.
    let note = Note::issue_or(
        summary.issues_at(DraftSite::Search),
        Note::Weak(algorithm_description(search.algorithm).to_owned()),
    );
    let (label, method) = layout.row(ui, "Method", |ui| {
        let method = ComboBox::from_id_salt("sweep_search_method")
            .width(layout.control_width())
            .truncate()
            .selected_text(algorithm_label(search.algorithm))
            .show_ui(ui, |ui| {
                for algorithm in DraftAlgorithm::ALL {
                    ui.selectable_value(&mut search.algorithm, algorithm, algorithm_label(algorithm))
                        .on_hover_text(algorithm_tooltip(algorithm));
                }
            })
            .response;
        let description = algorithm_description(search.algorithm);
        note_block(ui, layout.cell_width, description, &note);
        method
    });
    let method = method
        .on_hover_text(algorithm_tooltip(search.algorithm))
        .labelled_by(label.id);
    rows.reveal_row(ui, DraftSite::Search, &method, None);
}

/// Draws the Objective, Goal and Across replicates rows. An output picked among those not recorded goes to `added`.
fn objective_rows(
    ui: &mut Ui,
    rows: &mut FormRows,
    search: &mut SearchDraft,
    lists: &OutputLists<'_>,
    summary: &CheckSummary,
    added: &mut Vec<String>,
) {
    let layout = rows.layout;
    let note = Note::issue_or(summary.issues_at(DraftSite::Objective), Note::Empty);
    let button = (!lists.unrecorded.is_empty()).then_some(ADD_TO_OUTPUTS);
    let (label, (objective, add)) = layout.row(ui, "Objective", |ui| {
        let (objective, picked) = output_combo(
            ui,
            Id::new("sweep_search_objective"),
            layout.control_width(),
            lists.choices,
            &mut search.objective_column,
        );
        added.extend(picked);
        let add = note_line_with_button(ui, layout.cell_width, &note, button);
        (objective, add)
    });
    if add {
        added.extend(lists.unrecorded.iter().cloned());
    }
    let label = label.on_hover_text(OBJECTIVE_TOOLTIP);
    let objective = objective.on_hover_text(OBJECTIVE_TOOLTIP).labelled_by(label.id);
    rows.reveal_row(ui, DraftSite::Objective, &objective, None);

    let tooltip = "Maximize or minimize objective";
    let (label, goal) = layout.row(ui, "Goal", |ui| {
        goal_combo(ui, layout.control_width(), &mut search.goal)
    });
    let label = label.on_hover_text(tooltip);
    goal.on_hover_text(tooltip).labelled_by(label.id);

    aggregate_row(
        ui,
        &layout,
        Id::new("sweep_search_aggregate"),
        &mut search.aggregate,
        OBJECTIVE_AGGREGATE_TOOLTIP,
    );
}

/// Inputs of the axis rows besides the settings they edit.
struct AxisContext<'a> {
    lists: OutputLists<'a>,
    /// Issues of both axis rows, in the order the check found them.
    issues: Vec<&'a DraftIssue>,
    /// Results of the draft's model in the Results tab, `None` while it holds no runs of the model.
    results: Option<&'a ResultsStore>,
}

/// Returns the part of an axis's id that names it.
fn axis_key(axis: GridAxis) -> &'static str {
    match axis {
        GridAxis::X => "x",
        GridAxis::Y => "y",
    }
}

/// Draws a row per axis with its output and range, the note of both axes, and the Across replicates row. An output
/// picked among those not recorded goes to `added`.
fn axis_rows(
    ui: &mut Ui,
    rows: &mut FormRows,
    search: &mut SearchDraft,
    context: &AxisContext<'_>,
    added: &mut Vec<String>,
) {
    let layout = rows.layout;
    for axis in [GridAxis::X, GridAxis::Y] {
        let (pattern_axis, _) = search.pattern_axis_mut(axis);
        let (label, (combo, picked)) = layout.row(ui, axis.label(), |ui| {
            output_combo(
                ui,
                Id::new(("sweep_search_axis", axis_key(axis))),
                layout.control_width(),
                context.lists.choices,
                &mut pattern_axis.column,
            )
        });
        added.extend(picked);
        let label = label.on_hover_text(AXIS_TOOLTIP);
        let combo = combo.on_hover_text(AXIS_TOOLTIP).labelled_by(label.id);
        rows.reveal_row(ui, DraftSite::Axis(axis), &combo, None);
        let range = match context.results {
            None => ResultsRange::NoResults,
            Some(store) => match store.output_range(&pattern_axis.column) {
                Some((low, high)) if high > low => ResultsRange::Range(low, high),
                _ => ResultsRange::NoRange,
            },
        };
        range_rows(ui, &layout, label.id, (search, axis), range);
    }
    let settings = &mut search.pattern_space;
    let note = axes_note(settings, &context.issues);
    let button = (!context.lists.unrecorded.is_empty()).then_some(ADD_TO_OUTPUTS);
    // The note of unset axes is guidance, and wraps in a block that keeps its height whatever the note says.
    let add = layout.blank_row(ui, |ui| {
        if button.is_some() {
            return note_line_with_button(ui, layout.cell_width, &note, button);
        }
        note_block(ui, layout.cell_width, AXES_UNSET, &note);
        false
    });
    if add {
        added.extend(context.lists.unrecorded.iter().cloned());
    }
    aggregate_row(
        ui,
        &layout,
        Id::new("sweep_search_pattern_aggregate"),
        &mut settings.aggregate,
        PATTERN_AGGREGATE_TOOLTIP,
    );
}

/// Range of an axis's output in the Results tab.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ResultsRange {
    /// The Results tab holds no runs of the model.
    NoResults,
    /// The runs give the output no range, recording it nowhere or at one value.
    NoRange,
    /// Lowest and highest value of the output.
    Range(f64, f64),
}

/// Label of a field under an axis, with the closure that adds the field and returns its response.
type AxisField<'a> = (&'a str, &'a mut dyn FnMut(&mut Ui) -> Response);

/// Draws the Automatic range checkbox of an axis under the axis's row, whose label is `axis_label`, then its
/// Minimum, Maximum and Cells: on one line, or one line each in a cell narrower than [`AXIS_FIELDS_STACK_BELOW`].
///
/// `search` and `axis` name the axis in the draft. While its range is automatic, Cells shows alone, and the draft
/// keeps the range the bounds held. While the Results tab holds runs of the model, a button beside the checkbox sets
/// both bounds to `range`.
fn range_rows(
    ui: &mut Ui,
    layout: &FormLayout,
    axis_label: Id,
    (search, axis): (&mut SearchDraft, GridAxis),
    range: ResultsRange,
) {
    let indent = if layout.stacked { checkbox_indent(ui) } else { 0.0 };
    let stack = layout.cell_width < AXIS_FIELDS_STACK_BELOW;
    layout.blank_row(ui, |ui| {
        let was_automatic = search.pattern_axis_mut(axis).0.is_automatic();
        let mut automatic = was_automatic;
        let mut use_results = false;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.add(Checkbox::new(&mut automatic, "Automatic range"))
                .on_hover_text(AUTOMATIC_RANGE_TOOLTIP);
            if range == ResultsRange::NoResults {
                return;
            }
            use_results = ui
                .add_enabled(
                    range != ResultsRange::NoRange,
                    Button::new("Use range from results").small(),
                )
                .on_hover_text("Set Minimum and Maximum to the range of this output in the Results tab")
                .on_disabled_hover_text("No values for this output in the Results tab")
                .clicked();
        });

        let (pattern_axis, remembered) = search.pattern_axis_mut(axis);
        let PatternAxis { min, max, cells, .. } = pattern_axis;
        let label_width = ["Minimum", "Maximum", "Cells"]
            .into_iter()
            .map(|text| text_width(ui, text))
            .fold(0.0, f32::max);
        let field_size = vec2(AXIS_BOUND_WIDTH, ui.spacing().interact_size.y);
        let field = |ui: &mut Ui, text: &str, add_field: &mut dyn FnMut(&mut Ui) -> Response| {
            let label = if stack {
                slot(ui, label_width, |ui| ui.label(text))
            } else {
                ui.label(text)
            };
            add_field(ui).labelled_by(axis_label).labelled_by(label.id);
        };
        let draw_fields = |ui: &mut Ui, fields: &mut [AxisField<'_>]| {
            if stack {
                for (text, add_field) in fields {
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        field(ui, text, &mut **add_field);
                    });
                }
            } else {
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    for (text, add_field) in fields {
                        field(ui, text, &mut **add_field);
                    }
                });
            }
        };
        let mut cells = |ui: &mut Ui| {
            ui.add(
                DragValue::new(&mut *cells)
                    .range(1..=MAX_AXIS_CELLS)
                    .clamp_existing_to_range(false),
            )
        };
        if was_automatic {
            draw_fields(ui, &mut [("Cells", &mut cells)]);
        } else {
            let (min, max) = (min.get_or_insert(remembered.0), max.get_or_insert(remembered.1));
            let speed = ((*max - *min).abs() * 0.005).max(0.001);
            let mut minimum = |ui: &mut Ui| ui.add_sized(field_size, bound_drag_value(&mut *min, speed));
            let mut maximum = |ui: &mut Ui| ui.add_sized(field_size, bound_drag_value(&mut *max, speed));
            draw_fields(
                ui,
                &mut [
                    ("Minimum", &mut minimum),
                    ("Maximum", &mut maximum),
                    ("Cells", &mut cells),
                ],
            );
        }

        if use_results && let ResultsRange::Range(low, high) = range {
            let (pattern_axis, _) = search.pattern_axis_mut(axis);
            pattern_axis.min = Some(low);
            pattern_axis.max = Some(high);
        } else if automatic != was_automatic {
            search.set_automatic_range(axis, automatic);
        }
    });
}

/// Returns a drag value for a bound of an axis, its text rounded to four significant digits.
fn bound_drag_value(value: &mut f64, speed: f64) -> DragValue<'_> {
    DragValue::new(value)
        .speed(speed)
        .custom_formatter(|value, _| format_significant(value))
        .custom_parser(parse_bound)
}

/// Draws the Evaluations and Batch size rows, and Population for a genetic algorithm, each with what its value
/// means for the runs.
fn budget_rows(ui: &mut Ui, rows: &mut FormRows, search: &mut SearchDraft, replicates: u64, summary: &CheckSummary) {
    let layout = rows.layout;
    let feedback = Note::Weak(evaluations_feedback(search.max_evaluations, replicates));
    feedback_row(
        ui,
        &layout,
        "Evaluations",
        "Configurations to run, re-evaluations included",
        &feedback,
        DragValue::new(&mut search.max_evaluations)
            .range(1..=MAX_DRAFT_RUNS)
            .clamp_existing_to_range(false),
    );
    let feedback = Note::Weak(batches_feedback(search));
    feedback_row(
        ui,
        &layout,
        "Batch size",
        "Configurations run at once before the search selects the next batch",
        &feedback,
        DragValue::new(&mut search.batch_size)
            .range(1..=MAX_BATCH_SIZE)
            .clamp_existing_to_range(false),
    );
    if search.algorithm != DraftAlgorithm::Genetic {
        return;
    }
    let site = DraftSite::MethodSetting(POPULATION_KEY);
    let feedback = Note::issue_or(
        summary.issues_at(site),
        population_feedback(search.max_evaluations, &search.genetic),
    );
    let population = feedback_row(
        ui,
        &layout,
        "Population",
        "Configurations per generation",
        &feedback,
        DragValue::new(&mut search.genetic.population)
            .range(1..=MAX_POPULATION)
            .clamp_existing_to_range(false),
    );
    rows.reveal_row(ui, site, &population, None);
}

/// Draws the Method settings section of every method but a random search, collapsed until opened.
///
/// A reveal of a setting in it opens it first, and holds [`FormRows::pending`] until it is fully open.
fn method_settings(ui: &mut Ui, rows: &mut FormRows, search: &mut SearchDraft, summary: &CheckSummary) {
    if search.algorithm == DraftAlgorithm::Random {
        return;
    }
    let mut issues = IssueCount::default();
    for issue in &summary.issues {
        if matches!(issue.site, DraftSite::MethodSetting(key) if key != POPULATION_KEY) {
            issues.add(issue.kind);
        }
    }
    let settings_summary = method_settings_summary(search);
    let heading = SectionHeading {
        title: "Method settings",
        issues,
        summary: &settings_summary,
    };
    let target = rows
        .reveal
        .filter(|site| matches!(site, DraftSite::MethodSetting(key) if *key != POPULATION_KEY));
    let (outer_layout, outer_reveal) = (rows.layout, rows.reveal);
    let max_evaluations = search.max_evaluations;
    let outcome = nested_section(ui, METHOD_SETTINGS_ID, heading, false, target.is_some(), |ui, body| {
        rows.layout = outer_layout.indented(ui.spacing().indent);
        rows.reveal = target.filter(|_| body.settled);
        match search.algorithm {
            DraftAlgorithm::Random => {}
            DraftAlgorithm::HillClimb => hill_climb_rows(ui, rows, &mut search.hill_climb, summary),
            DraftAlgorithm::Genetic => genetic_rows(ui, rows, &mut search.genetic, summary),
            DraftAlgorithm::PatternSpace => {
                pattern_space_rows(ui, rows, &mut search.pattern_space, max_evaluations, summary);
            }
        }
    });
    rows.layout = outer_layout;
    rows.reveal = outer_reveal;
    if target.is_some() && !outcome.settled {
        rows.pending = true;
    }
}

/// Draws `label` and `widget` on one form row with `feedback` beside the widget, both with `tooltip`.
///
/// Returns the widget's response.
fn feedback_row(
    ui: &mut Ui,
    layout: &FormLayout,
    label: &str,
    tooltip: &str,
    feedback: &Note,
    widget: impl egui::Widget,
) -> Response {
    let (label, response) = layout.row(ui, label, |ui| layout.with_feedback(ui, feedback, |ui| ui.add(widget)));
    let label = label.on_hover_text(tooltip);
    response.on_hover_text(tooltip).labelled_by(label.id)
}

/// Draws the row of the method setting `key`, with its issue in the feedback slot, and reveals it when asked.
fn setting_row(
    ui: &mut Ui,
    rows: &mut FormRows,
    summary: &CheckSummary,
    key: &'static str,
    (label, tooltip): (&str, &str),
    widget: impl egui::Widget,
) {
    let site = DraftSite::MethodSetting(key);
    let feedback = Note::issue_or(summary.issues_at(site), Note::Empty);
    let layout = rows.layout;
    let response = feedback_row(ui, &layout, label, tooltip, &feedback, widget);
    rows.reveal_row(ui, site, &response, None);
}

/// Returns a drag value for a share from 0 to 1.
fn share_drag_value(share: &mut f64) -> DragValue<'_> {
    DragValue::new(share)
        .range(0.0..=1.0)
        .speed(0.005)
        .max_decimals(3)
        .clamp_existing_to_range(false)
}

fn hill_climb_rows(ui: &mut Ui, rows: &mut FormRows, settings: &mut HillClimbSettings, summary: &CheckSummary) {
    setting_row(
        ui,
        rows,
        summary,
        "hill_climb.mutation_scale",
        ("Step size", STEP_SIZE_TOOLTIP),
        share_drag_value(&mut settings.mutation_scale),
    );
    setting_row(
        ui,
        rows,
        summary,
        "hill_climb.patience",
        (
            "Patience",
            "Batches without improvement before restarting from a random configuration",
        ),
        DragValue::new(&mut settings.patience)
            .range(1..=u64::from(u32::MAX))
            .clamp_existing_to_range(false),
    );
    rows.layout.blank_row(ui, |ui| {
        ui.add(Checkbox::new(&mut settings.reevaluate, "Re-evaluate best"))
            .on_hover_text("Run best configuration again in each batch with fresh replicates");
    });
}

fn genetic_rows(ui: &mut Ui, rows: &mut FormRows, settings: &mut GeneticSettings, summary: &CheckSummary) {
    setting_row(
        ui,
        rows,
        summary,
        "genetic.elite_count",
        (
            "Elites",
            "Best configurations carried unchanged into the next generation",
        ),
        DragValue::new(&mut settings.elite_count)
            .range(0..=MAX_POPULATION)
            .clamp_existing_to_range(false),
    );
    setting_row(
        ui,
        rows,
        summary,
        "genetic.tournament_size",
        ("Tournament size", "Configurations compared to select each parent"),
        DragValue::new(&mut settings.tournament_size)
            .range(1..=MAX_POPULATION)
            .clamp_existing_to_range(false),
    );
    setting_row(
        ui,
        rows,
        summary,
        "genetic.crossover_rate",
        (
            "Crossover rate",
            "Probability that a child combines parameters from two parents",
        ),
        share_drag_value(&mut settings.crossover_rate),
    );
    setting_row(
        ui,
        rows,
        summary,
        "genetic.mutation_rate",
        ("Mutation rate", "Probability of mutating each parameter of a child"),
        share_drag_value(&mut settings.mutation_rate),
    );
    setting_row(
        ui,
        rows,
        summary,
        "genetic.mutation_scale",
        ("Step size", STEP_SIZE_TOOLTIP),
        share_drag_value(&mut settings.mutation_scale),
    );
    setting_row(
        ui,
        rows,
        summary,
        "genetic.reevaluate_fraction",
        (
            "Re-evaluation rate",
            "Fraction of each generation run again with fresh replicates, best first",
        ),
        share_drag_value(&mut settings.reevaluate_fraction),
    );
}

fn pattern_space_rows(
    ui: &mut Ui,
    rows: &mut FormRows,
    settings: &mut PatternSpaceSettings,
    max_evaluations: u64,
    summary: &CheckSummary,
) {
    let layout = rows.layout;
    let site = DraftSite::MethodSetting(INITIAL_SAMPLES_KEY);
    let feedback = Note::issue_or(
        summary.issues_at(site),
        initial_samples_feedback(settings.initial_samples, max_evaluations),
    );
    let initial_samples = feedback_row(
        ui,
        &layout,
        "Initial samples",
        "Random configurations to run before breeding from the grid",
        &feedback,
        DragValue::new(&mut settings.initial_samples)
            .range(0..=MAX_DRAFT_RUNS)
            .clamp_existing_to_range(false),
    );
    rows.reveal_row(ui, site, &initial_samples, None);
    setting_row(
        ui,
        rows,
        summary,
        "pse.mutation_scale",
        ("Step size", STEP_SIZE_TOOLTIP),
        share_drag_value(&mut settings.mutation_scale),
    );
}

/// Draws a combo box `width` wide of the outputs `choices` offers, writing the column picked into `column`, and
/// showing `column` whether or not it is among them.
///
/// Returns the combo box's response, and the column picked when it was among those not recorded yet.
fn output_combo(
    ui: &mut Ui,
    id: Id,
    width: f32,
    choices: &OutputChoices,
    column: &mut String,
) -> (Response, Option<String>) {
    let selected = if column.is_empty() {
        "Select output".to_owned()
    } else {
        output_label(column)
    };
    let mut picked = None;
    let response = ComboBox::from_id_salt(id)
        .width(width)
        .truncate()
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for output in &choices.recorded {
                if ui.selectable_label(column == output, output_label(output)).clicked() {
                    output.clone_into(column);
                }
            }
            if choices.unrecorded.is_empty() {
                return;
            }
            ui.separator();
            ui.add(Label::new(RichText::new(UNRECORDED_CAPTION).weak()).wrap());
            for output in &choices.unrecorded {
                if ui.selectable_label(column == output, output_label(output)).clicked() {
                    output.clone_into(column);
                    picked = Some(output.clone());
                }
            }
        })
        .response;
    (response, picked)
}

fn goal_combo(ui: &mut Ui, width: f32, goal: &mut Goal) -> Response {
    let text = |goal: Goal| match goal {
        Goal::Maximize => "Maximize",
        Goal::Minimize => "Minimize",
    };
    ComboBox::from_id_salt("sweep_search_goal")
        .width(width)
        .truncate()
        .selected_text(text(*goal))
        .show_ui(ui, |ui| {
            for choice in [Goal::Maximize, Goal::Minimize] {
                ui.selectable_value(goal, choice, text(choice));
            }
        })
        .response
}

/// Draws the Across replicates row, whose combo box of id `id` writes `aggregate`.
fn aggregate_row(ui: &mut Ui, layout: &FormLayout, id: Id, aggregate: &mut Aggregate, tooltip: &str) {
    let text = |aggregate: Aggregate| match aggregate {
        Aggregate::Median => "Median",
        Aggregate::Mean => "Mean",
    };
    let (label, combo) = layout.row(ui, "Across replicates", |ui| {
        ComboBox::from_id_salt(id)
            .width(layout.control_width())
            .truncate()
            .selected_text(text(*aggregate))
            .show_ui(ui, |ui| {
                for choice in [Aggregate::Median, Aggregate::Mean] {
                    ui.selectable_value(aggregate, choice, text(choice));
                }
            })
            .response
    });
    let label = label.on_hover_text(tooltip);
    combo.on_hover_text(tooltip).labelled_by(label.id);
}

#[cfg(test)]
mod tests {
    use henad_core::explore::search::genetic::GeneticSettings;
    use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
    use henad_explore::schema::model_schema;
    use henad_models::registry::{ModelEntry, model_registry};

    use super::{
        AXES_UNSET, OutputChoices, axes_note, batches_feedback, evaluations_feedback, initial_samples_feedback,
        method_settings_summary, parse_bound, population_feedback,
    };
    use crate::ui::sweep::draft::{
        AXIS_RANGE_MISSING, DraftAlgorithm, DraftIssue, DraftMode, DraftSite, GridAxis, IssueKind, SweepDraft,
    };
    use crate::ui::sweep::layout::Note;

    fn sir() -> ModelEntry {
        model_registry(None)
            .into_iter()
            .find(|entry| entry.id == "sir")
            .expect("SIR is registered")
    }

    fn search_draft(algorithm: DraftAlgorithm) -> SweepDraft {
        let entry = sir();
        let mut draft = SweepDraft::new(&model_schema(&entry));
        draft.mode = DraftMode::Search;
        draft.search.algorithm = algorithm;
        draft
    }

    #[test]
    fn each_budget_field_says_what_it_means_for_the_runs() {
        assert_eq!(evaluations_feedback(200, 3), "× 3 replicates = 600 runs");
        assert_eq!(evaluations_feedback(1, 1), "× 1 replicate = 1 run");

        let mut draft = search_draft(DraftAlgorithm::Random);
        assert_eq!(
            batches_feedback(&draft.search),
            "About 13 batches",
            "200 in batches of 16"
        );
        draft.search.batch_size = 256;
        assert_eq!(batches_feedback(&draft.search), "About 1 batch");
        let draft = search_draft(DraftAlgorithm::Genetic);
        assert_eq!(
            batches_feedback(&draft.search),
            "About 15 batches",
            "no batch spans two generations"
        );

        let settings = GeneticSettings::default();
        assert_eq!(
            population_feedback(200, &settings),
            Note::Weak("About 6 generations".to_owned())
        );
        assert_eq!(
            population_feedback(32, &settings),
            Note::Weak("About 1 generation".to_owned())
        );
        assert_eq!(
            population_feedback(20, &settings),
            Note::Warn("Fewer evaluations than one generation".to_owned())
        );

        assert_eq!(
            initial_samples_feedback(64, 200),
            Note::Weak("64 of 200 evaluations drawn at random".to_owned())
        );
        assert!(matches!(initial_samples_feedback(200, 200), Note::Warn(_)));
    }

    #[test]
    fn method_settings_summarize_each_method() {
        let draft = search_draft(DraftAlgorithm::Random);
        assert_eq!(method_settings_summary(&draft.search), "");
        let mut draft = search_draft(DraftAlgorithm::HillClimb);
        assert_eq!(method_settings_summary(&draft.search), "Step size 0.1 · patience 5");
        draft.search.hill_climb.reevaluate = true;
        assert_eq!(
            method_settings_summary(&draft.search),
            "Step size 0.1 · patience 5 · re-evaluate best"
        );
        let draft = search_draft(DraftAlgorithm::Genetic);
        assert_eq!(
            method_settings_summary(&draft.search),
            "Elites 2 · tournament size 3 · crossover 0.9 · mutation 0.2 · step size 0.1 · re-evaluation 0.25"
        );
        let draft = search_draft(DraftAlgorithm::PatternSpace);
        assert_eq!(
            method_settings_summary(&draft.search),
            "64 initial samples · step size 0.1"
        );
    }

    #[test]
    fn the_output_lists_offer_what_the_runs_do_not_record_yet() {
        let entry = sir();
        let schema = model_schema(&entry);
        let mut draft = search_draft(DraftAlgorithm::Random);
        let choices = OutputChoices::of(&draft, &schema);
        assert_eq!(
            choices.recorded.len(),
            4 * schema.stats.len(),
            "the four defaults of every stat"
        );
        assert_eq!(
            choices.unrecorded,
            [
                "Susceptible:argmax",
                "Susceptible:argmin",
                "Infected:argmax",
                "Infected:argmin",
                "Recovered:argmax",
                "Recovered:argmin",
            ]
        );

        draft.default_reducers = false;
        assert!(draft.add_watched_output("Infected:argmax"));
        let choices = OutputChoices::of(&draft, &schema);
        assert_eq!(choices.recorded, ["Infected:argmax"]);
        assert_eq!(choices.unrecorded.len(), 6 * schema.stats.len() - 1);
        assert!(!choices.unrecorded.iter().any(|column| column == "Infected:argmax"));
    }

    fn axes(x_max: f64, y_max: f64) -> PatternSpaceSettings {
        let axis = |column: &str, max: f64| PatternAxis::bounded(column, 0.0, max, 20);
        PatternSpaceSettings::new(axis("Infected:max", x_max), axis("Infected:argmax", y_max))
    }

    #[test]
    fn the_axes_share_one_note_of_their_cells_or_their_issues() {
        let settings = axes(1000.0, 300.0);
        assert_eq!(axes_note(&settings, &[]), Note::Weak("20 × 20 = 400 cells".to_owned()));

        let unset = [
            DraftIssue::missing(DraftSite::Axis(GridAxis::X), AXIS_RANGE_MISSING),
            DraftIssue::missing(DraftSite::Axis(GridAxis::Y), AXIS_RANGE_MISSING),
        ];
        let unset: Vec<&DraftIssue> = unset.iter().collect();
        assert_eq!(
            axes_note(&axes(0.0, 0.0), &unset),
            Note::Issue(IssueKind::Missing, AXES_UNSET.to_owned())
        );

        let invalid = [
            DraftIssue::new(DraftSite::Axis(GridAxis::X), "Maximum must be greater than Minimum"),
            DraftIssue::missing(DraftSite::Axis(GridAxis::Y), AXIS_RANGE_MISSING),
            DraftIssue::new(
                DraftSite::Axis(GridAxis::Y),
                "Recovered, maximum is missing from Outputs",
            ),
        ];
        let invalid: Vec<&DraftIssue> = invalid.iter().collect();
        assert_eq!(
            axes_note(&settings, &invalid),
            Note::Issue(
                IssueKind::Invalid,
                "X axis: Maximum must be greater than Minimum. Y axis: Recovered, maximum is missing from Outputs"
                    .to_owned()
            ),
            "invalid input first, every axis at once"
        );

        let setting = [DraftIssue::new(
            DraftSite::Axis(GridAxis::X),
            "X axis Cells must be at least 1, got 0",
        )];
        let setting: Vec<&DraftIssue> = setting.iter().collect();
        assert_eq!(
            axes_note(&settings, &setting),
            Note::Issue(IssueKind::Invalid, "X axis Cells must be at least 1, got 0".to_owned()),
            "a message that names its axis is not led by it again"
        );

        let unpicked = [DraftIssue::missing(DraftSite::Axis(GridAxis::X), "Select output")];
        let unpicked: Vec<&DraftIssue> = unpicked.iter().collect();
        assert_eq!(
            axes_note(&settings, &unpicked),
            Note::Issue(IssueKind::Missing, "X axis: Select output".to_owned())
        );
    }

    #[test]
    fn a_bound_reads_back_as_its_field_writes_it() {
        assert_eq!(parse_bound("1,027,606"), Some(1_027_606.0));
        assert_eq!(parse_bound(" 0.05 "), Some(0.05));
        assert_eq!(parse_bound("\u{2212}3"), Some(-3.0));
        assert_eq!(parse_bound("–"), None, "the dash of a value that is not finite");
    }
}
