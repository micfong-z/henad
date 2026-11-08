//! Compare runs: a converted copy and its original sheet run side by side, and their measures are compared tick by
//! tick.
//!
//! Each model runs once as a sweep of one run through henad-explore's [`SweepRun`], off the UI thread as the Sweep
//! tab's runs do, at one seed and at the preview's size, for [`COMPARE_TICKS`] ticks. The two series of measures are
//! plotted together, and the *first difference* is the first tick at which a measure both models hold reads another
//! value. An unedited copy of a model that replays exactly matches its original at every tick, since the conversion
//! copies every node with its random key. Two runs of a model that does not replay exactly, such as one that adds up
//! floating-point numbers in the order agents are found, can differ at one seed, and the comparison names no tick for
//! it.
//!
//! A comparison holds the key of the copy and the original it ran, and the Editor drops it once either changes.
//! Dropping a [`Comparison`] stops both runs within `STOP_WAIT` (250 ms), and a run held in its step is left
//! detached.

use egui::{Panel, RichText, Ui};
use egui_plot::{Legend, Line, LineStyle, Plot, VLine};
use henad_compute::entry::ModelEntry;
use henad_core::explore::measure::SeriesBuffer;
use henad_core::explore::outcome::RunStatus;
use henad_core::explore::spec::SweepSpec;
use henad_core::params::ParamValue;
use henad_explore::handle::{SweepEvent, SweepOutput, SweepRun, SweepRunOptions};
use henad_explore::model_file::ModelFile;
use henad_graph::sheet::sentence::{
    CLOSE_COMPARISON, compare_failed, compare_running, compare_series, runs_differ, runs_do_not_replay, runs_match,
    runs_share_no_measure,
};

use super::preview::preview_values;
use super::{BannerKey, EditorRequest};
use crate::state::AppState;
#[cfg(not(target_arch = "wasm32"))]
use crate::state::STOP_WAIT;
use crate::ui::results::plot::{NO_DATA_COLOR, config_color, format_precise};
use crate::ui::show_plot;
use crate::ui::sweep::session::provenance;

/// Number of ticks each model of a comparison runs.
pub const COMPARE_TICKS: u64 = 200;

/// Seed of a comparison when the Parameters tab sets none.
pub const DEFAULT_COMPARE_SEED: u64 = 1;

/// Height in points of the comparison's plot.
const PLOT_HEIGHT: f32 = 200.0;

/// Ids of the parameters that set a model's size, which a comparison runs at the preview's size.
const SIZE_IDS: [&str; 3] = ["grid_width", "grid_height", "num_agents"];

/// Result of a comparison whose two runs both finished.
#[derive(Debug, Clone, PartialEq)]
pub enum CompareOutcome {
    /// Every measure both models hold reads the same value at every tick up to `ticks`.
    Match {
        /// Last tick compared.
        ticks: u64,
    },
    /// A measure reads another value at `tick`, the first tick at which any measure differs.
    Differ {
        /// First tick at which a measure differs.
        tick: u64,
        /// Name of the first measure that differs at that tick.
        measure: String,
        /// Value of the measure in the original.
        value: f64,
        /// Value of the measure in the copy.
        copy_value: f64,
    },
    /// The two models hold no measure of the same name.
    NoSharedMeasure,
    /// A measure differs, and either model does not replay exactly, so two runs of it at one seed can differ.
    Inexact,
}

/// Measures of one finished run: the name of each column and the rows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunMeasures {
    /// Name of each measure, in column order.
    pub columns: Vec<String>,
    /// One row per sampled tick.
    pub series: SeriesBuffer,
}

/// Returns whether two sampled values are the same, two values that are not numbers included.
fn same_value(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || a == b || (a.is_nan() && b.is_nan())
}

/// Compares the measures of the original, `original`, with those of its copy, `copy`, tick by tick over the ticks
/// both sampled, and returns the first difference or a match.
pub fn first_difference(original: &RunMeasures, copy: &RunMeasures) -> CompareOutcome {
    let shared: Vec<(usize, usize)> = original
        .columns
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let copy_index = copy.columns.iter().position(|other| other == name)?;
            Some((index, copy_index))
        })
        .collect();
    if shared.is_empty() {
        return CompareOutcome::NoSharedMeasure;
    }
    let mut last = 0;
    let mut copy_rows = copy.series.rows().peekable();
    for (tick, row) in original.series.rows() {
        while copy_rows.next_if(|(copy_tick, _)| *copy_tick < tick).is_some() {}
        let Some((_, copy_row)) = copy_rows.next_if(|(copy_tick, _)| *copy_tick == tick) else {
            continue;
        };
        for (index, copy_index) in &shared {
            let (value, copy_value) = (row[*index], copy_row[*copy_index]);
            if !same_value(value, copy_value) {
                return CompareOutcome::Differ {
                    tick,
                    measure: original.columns[*index].clone(),
                    value,
                    copy_value,
                };
            }
        }
        last = tick;
    }
    CompareOutcome::Match { ticks: last }
}

/// One side of a comparison: the run of one model, and its measures once it finished.
#[derive(Debug)]
struct Side {
    /// Name of the model, as the model's file names it.
    name: String,
    /// Run of the model, `None` once it ended.
    run: Option<SweepRun>,
    /// Names of the measures, known once the run is planned.
    columns: Vec<String>,
    /// Measures of the finished run.
    measures: Option<RunMeasures>,
}

/// Stops the run within [`STOP_WAIT`]. A dropped [`SweepRun`] would wait for its thread without a bound.
impl Drop for Side {
    fn drop(&mut self) {
        stop(self.run.take());
    }
}

impl Side {
    /// Reads the events of the run, and returns the error that ended it, if any.
    fn poll(&mut self) -> Option<String> {
        let run = self.run.as_mut()?;
        run.update(0.0);
        let mut failure = None;
        let mut ended = false;
        while let Some(event) = run.try_recv() {
            match event {
                SweepEvent::Planned(outline) => self.columns.clone_from(&outline.stat_columns),
                SweepEvent::RunFinished { outcome, .. } => {
                    if matches!(outcome.status, RunStatus::Ok | RunStatus::NonFinite) {
                        self.measures = Some(RunMeasures {
                            columns: self.columns.clone(),
                            series: outcome.series,
                        });
                    } else {
                        failure = Some(outcome.note.unwrap_or_else(|| format!("{:?}", outcome.status)));
                    }
                }
                SweepEvent::Failed(message) => {
                    failure = Some(message);
                    ended = true;
                }
                SweepEvent::Finished(_) => ended = true,
                SweepEvent::Warned(_) | SweepEvent::SearchBatchTold(_) => {}
            }
        }
        if ended {
            stop(self.run.take());
            if self.measures.is_none() && failure.is_none() {
                failure = Some(format!("{} ended without a result", self.name));
            }
        }
        failure
    }
}

/// Stops `run` within [`STOP_WAIT`] and leaves a thread held in a step detached.
fn stop(run: Option<SweepRun>) {
    let Some(run) = run else { return };
    #[cfg(not(target_arch = "wasm32"))]
    if !run.stop(STOP_WAIT) {
        log::warn!("A run of Compare runs did not stop within {} ms", STOP_WAIT.as_millis());
    }
    #[cfg(target_arch = "wasm32")]
    drop(run);
}

/// A copy and its original running side by side, or their measures once both runs finished.
#[derive(Debug)]
pub struct Comparison {
    /// Seed both runs take.
    pub seed: u64,
    /// Load and code revision of the copy and hash of the original's text that the runs started from.
    pub key: BannerKey,
    /// The original, then the copy.
    sides: [Side; 2],
    /// Error that ended a run, `None` while both run or once both finished.
    pub failure: Option<String>,
    /// Whether both models replay exactly, so that a difference comes from the copy.
    replays_exactly: bool,
}

impl Comparison {
    /// Starts the runs of `original`, the text of the sheet, and `copy`, the text of its copy, at `seed` and at the
    /// preview's size. `key` is the key of the copy and the original in the Editor. Each event of either run repaints
    /// the app.
    ///
    /// # Errors
    ///
    /// Returns a message when either text does not build a model or a run cannot start. A run that started is stopped
    /// then.
    pub fn start(app: &AppState, original: &str, copy: &str, seed: u64, key: BannerKey) -> Result<Self, String> {
        let original = entry_of(original)?;
        let copy = entry_of(copy)?;
        let fixed = size_values(&original);
        let replays_exactly = original.metadata().replays_exactly && copy.metadata().replays_exactly;
        let side = |entry: &ModelEntry, run| Side {
            name: entry.name().to_owned(),
            run: Some(run),
            columns: Vec::new(),
            measures: None,
        };
        let original_run = start_run(app, original.clone(), &fixed, seed)?;
        let original_side = side(&original, original_run);
        // The original's run stops with its side when the copy's run does not start.
        let copy_run = start_run(app, copy.clone(), &fixed, seed)?;
        Ok(Self {
            seed,
            key,
            sides: [original_side, side(&copy, copy_run)],
            failure: None,
            replays_exactly,
        })
    }

    /// Reads the events of both runs.
    pub fn poll(&mut self) {
        for side in &mut self.sides {
            if let Some(failure) = side.poll()
                && self.failure.is_none()
            {
                self.failure = Some(failure);
            }
        }
        if self.failure.is_some() {
            for side in &mut self.sides {
                stop(side.run.take());
            }
        }
    }

    /// Returns the result once both runs finished, `None` while either runs or after a failure.
    ///
    /// A difference reads [`CompareOutcome::Inexact`] when either model does not replay exactly.
    pub fn outcome(&self) -> Option<CompareOutcome> {
        let [original, copy] = &self.sides;
        let outcome = first_difference(original.measures.as_ref()?, copy.measures.as_ref()?);
        Some(match outcome {
            CompareOutcome::Differ { .. } if !self.replays_exactly => CompareOutcome::Inexact,
            outcome => outcome,
        })
    }

    /// Returns the line the comparison shows: running, the result, or the failure.
    pub fn text(&self) -> String {
        let [original, copy] = &self.sides;
        if let Some(failure) = &self.failure {
            return compare_failed(failure);
        }
        match self.outcome() {
            None => compare_running(&original.name, &copy.name, COMPARE_TICKS, self.seed),
            Some(CompareOutcome::Match { ticks }) => runs_match(&original.name, &copy.name, ticks),
            Some(CompareOutcome::Differ {
                tick,
                measure,
                value,
                copy_value,
            }) => {
                let [value, copy_value] = difference_texts(value, copy_value);
                runs_differ(&original.name, &copy.name, tick, &measure, &copy_value, &value)
            }
            Some(CompareOutcome::NoSharedMeasure) => runs_share_no_measure(&original.name, &copy.name),
            Some(CompareOutcome::Inexact) => runs_do_not_replay(&original.name, &copy.name),
        }
    }
}

/// Returns the texts of two values that differ, with six significant digits, or with every digit when six print both
/// the same.
fn difference_texts(value: f64, copy_value: f64) -> [String; 2] {
    let short = [format_precise(value), format_precise(copy_value)];
    if short[0] == short[1] {
        [value.to_string(), copy_value.to_string()]
    } else {
        short
    }
}

/// Returns the entry of the model file `text`.
fn entry_of(text: &str) -> Result<ModelEntry, String> {
    ModelFile::parse(text)
        .and_then(|file| file.entry())
        .map_err(|error| error.to_string())
}

/// Returns the values that run `entry` at the preview's size, in the form `--set` takes.
fn size_values(entry: &ModelEntry) -> Vec<(String, String)> {
    let (values, _) = preview_values(entry);
    entry
        .param_descriptors()
        .iter()
        .zip(values)
        .filter(|(descriptor, _)| SIZE_IDS.contains(&descriptor.id()))
        .filter_map(|(descriptor, value)| match value {
            ParamValue::U32(size) => Some((descriptor.id().to_owned(), size.to_string())),
            _ => None,
        })
        .collect()
}

/// Starts one run of `entry` for [`COMPARE_TICKS`] ticks at `seed` with the values `fixed`.
fn start_run(app: &AppState, entry: ModelEntry, fixed: &[(String, String)], seed: u64) -> Result<SweepRun, String> {
    let mut spec = SweepSpec::new(entry.id());
    spec.fixed = fixed.to_vec();
    spec.run.steps = COMPARE_TICKS;
    spec.run.replicates = 1;
    spec.measure.default_reducers = false;
    spec.seeds.root = seed;
    let mut options = SweepRunOptions::new(provenance(app.product.host));
    options.wake = Some(app.repaint_waker());
    #[cfg(not(target_arch = "wasm32"))]
    {
        options.build_pool = app.model_pool().current().1;
    }
    SweepRun::start(entry, None, spec, SweepOutput::Memory, options).map_err(|error| error.to_string())
}

/// Starts Compare runs for the copy in the Editor and its original, at the Parameters tab's seed. A failure to start
/// shows as the comparison's failure.
pub fn start_comparison(app: &mut AppState) {
    let Some(copy) = app.editor.model.graph().map(super::graph::GraphEditor::text) else {
        return;
    };
    let Some(original) = super::expert::original_text(&app.editor) else {
        return;
    };
    let Some(key) = super::expert::current_copy_key(&app.editor) else {
        return;
    };
    let seed = app.seed.unwrap_or(DEFAULT_COMPARE_SEED);
    app.editor.comparison = None;
    match Comparison::start(app, &original, &copy, seed, key) {
        Ok(comparison) => app.editor.comparison = Some(Box::new(comparison)),
        Err(error) => app.editor.file_status = Some(compare_failed(&error)),
    }
}

/// Draws the comparison under the banner: its line, Close comparison, and the plot of both runs' measures.
pub fn comparison_ui(ui: &mut Ui, comparison: &Comparison, request: &mut Option<EditorRequest>) {
    Panel::top("henad_expert_comparison").resizable(false).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(comparison.text()));
            if ui.button(CLOSE_COMPARISON).clicked() {
                *request = Some(EditorRequest::CloseComparison);
            }
        });
        let [original, copy] = &comparison.sides;
        let (Some(original_measures), Some(copy_measures)) = (&original.measures, &copy.measures) else {
            return;
        };
        let outcome = comparison.outcome();
        let plot = Plot::new("henad_expert_comparison_plot")
            .height(PLOT_HEIGHT)
            .x_axis_label("Tick")
            .legend(Legend::default());
        show_plot(ui, plot, |plot_ui| {
            for (index, column) in original_measures.columns.iter().enumerate() {
                let color = config_color(index);
                plot_ui.line(
                    Line::new(compare_series(column, &original.name), points(original_measures, index))
                        .color(color)
                        .width(1.5),
                );
                if let Some(copy_index) = copy_measures.columns.iter().position(|other| other == column) {
                    plot_ui.line(
                        Line::new(compare_series(column, &copy.name), points(copy_measures, copy_index))
                            .color(color)
                            .width(1.5)
                            .style(LineStyle::dashed_loose()),
                    );
                }
            }
            if let Some(CompareOutcome::Differ { tick, .. }) = outcome {
                plot_ui.vline(VLine::new("", tick as f64).color(NO_DATA_COLOR));
            }
        });
    });
}

/// Returns the points of column `column` of `measures`, each at its tick.
fn points(measures: &RunMeasures, column: usize) -> Vec<[f64; 2]> {
    measures
        .series
        .rows()
        .map(|(tick, row)| [tick as f64, row[column]])
        .collect()
}

#[cfg(test)]
mod tests {
    use henad_core::explore::measure::SeriesBuffer;

    use super::{CompareOutcome, RunMeasures, difference_texts, first_difference};

    fn measures(columns: &[&str], rows: &[(u64, &[f64])]) -> RunMeasures {
        let mut series = SeriesBuffer::new(columns.len());
        for (tick, values) in rows {
            series.push(*tick, values);
        }
        RunMeasures {
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            series,
        }
    }

    #[test]
    fn the_first_tick_at_which_a_shared_measure_differs_is_named() {
        let original = measures(&["Alive"], &[(0, &[5.0]), (1, &[6.0]), (2, &[7.0])]);
        let copy = measures(
            &["Alive", "Extra"],
            &[(0, &[5.0, 1.0]), (1, &[8.0, 1.0]), (2, &[9.0, 1.0])],
        );
        assert_eq!(
            first_difference(&original, &copy),
            CompareOutcome::Differ {
                tick: 1,
                measure: "Alive".to_owned(),
                value: 6.0,
                copy_value: 8.0,
            }
        );
    }

    #[test]
    fn equal_series_match_up_to_their_last_tick() {
        let rows: &[(u64, &[f64])] = &[(0, &[1.0, f64::NAN]), (1, &[2.0, f64::NAN])];
        let original = measures(&["S", "I"], rows);
        assert_eq!(
            first_difference(&original, &original.clone()),
            CompareOutcome::Match { ticks: 1 }
        );
        let other = measures(&["R"], &[(0, &[1.0]), (1, &[2.0])]);
        assert_eq!(first_difference(&original, &other), CompareOutcome::NoSharedMeasure);
    }

    #[test]
    fn differing_values_print_short_unless_short_forms_are_equal() {
        assert_eq!(difference_texts(22915.0, 22455.0), ["22,915", "22,455"]);
        assert_eq!(
            difference_texts(f64::from(0.3_f32), f64::from(0.25_f32)),
            ["0.3", "0.25"]
        );
        let [value, copy_value] = difference_texts(0.300_000_1, 0.300_000_2);
        assert_ne!(value, copy_value);
    }
}
