//! Writers and readers of the tables a search writes beside `runs.csv`.
//!
//! `evaluations.csv` has one row per evaluation and `batches.csv` one row per batch, each written as its batch is
//! told. A genetic algorithm adds `generations.csv`, one row per finished generation. Once the search ends,
//! `best.csv` ranks every candidate of a random search, hill climb or genetic algorithm, and `archive.csv` lists the
//! filled cells of a Pattern Space Exploration (PSE). A config is written in the columns `runs.csv` gives it, one per
//! parameter and then one per action's tick.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::str::FromStr;

use henad_core::explore::plan::Config;
use henad_core::explore::search::pse::{ArchiveEntry, PatternCell, PatternPlacement, PatternSpaceSettings};
use henad_core::explore::search::{GenerationSummary, RankingEntry, SearchAlgorithm, SearchSpec};
use henad_core::explore::spec::ActionSpec;
use henad_core::explore::value::format_value;
use henad_core::export::csv::{escape_field, fmt_f64};
use henad_core::params::{ParamDescriptor, ParamKind};

use crate::output::read::{ReadError, parse_one, record_ends};
use crate::output::{BATCHES_FILE, EVALUATIONS_FILE, GENERATIONS_FILE};
use crate::search_run::{EvaluatedCandidate, EvaluationReading, SearchUpdate};

/// Headers of the columns of `evaluations.csv` before the config.
pub const EVALUATION_ID_COLUMNS: [&str; 8] = [
    "candidate_id",
    "batch",
    "origin",
    "first_parent_id",
    "second_parent_id",
    "reevaluated_id",
    "replicate_offset",
    "replicates",
];

/// Headers of the columns of `evaluations.csv` after the config, for a search that scores an objective.
pub const OBJECTIVE_COLUMNS: [&str; 4] = ["failed", "objective", "pooled_objective", "pooled_replicates"];

/// Headers of the columns of `evaluations.csv` after the config, for a PSE.
pub const PLACEMENT_COLUMNS: [&str; 7] = ["failed", "x", "y", "x_index", "y_index", "outside", "new_cell"];

/// Headers of `batches.csv` for a search that scores an objective.
pub const OBJECTIVE_BATCH_COLUMNS: [&str; 5] = ["batch", "evaluations", "runs", "best_candidate_id", "best_objective"];

/// Headers of `batches.csv` for a PSE.
pub const PATTERN_BATCH_COLUMNS: [&str; 4] = ["batch", "evaluations", "runs", "filled_cells"];

/// Headers of `generations.csv`.
pub const GENERATION_COLUMNS: [&str; 4] = ["generation", "best", "median", "worst"];

/// Headers of the columns of `best.csv` before the config.
pub const RANKING_ID_COLUMNS: [&str; 2] = ["rank", "candidate_id"];

/// Headers of the columns of `best.csv` after the config.
pub const RANKING_COLUMNS: [&str; 5] = [
    "pooled_objective",
    "pooled_replicates",
    "pooled_failed",
    "evaluations",
    "first_batch",
];

/// Headers of the columns of `archive.csv` before the config.
pub const ARCHIVE_ID_COLUMNS: [&str; 8] = [
    "x_index",
    "y_index",
    "x_min",
    "x_max",
    "y_min",
    "y_max",
    "hits",
    "candidate_id",
];

/// Headers of the columns of `archive.csv` after the config.
pub const ARCHIVE_COLUMNS: [&str; 2] = ["x", "y"];

/// Parameter and action columns of a config, as `runs.csv` writes them.
#[derive(Debug, Clone)]
pub struct ConfigColumns {
    /// Kind of every parameter, in descriptor order.
    kinds: Vec<ParamKind>,
    /// Header of every column, escaped.
    headers: Vec<String>,
}

impl ConfigColumns {
    pub fn new(params: &[ParamDescriptor], actions: &[ActionSpec]) -> Self {
        let headers = params
            .iter()
            .map(|param| escape_field(param.id))
            .chain(actions.iter().map(|action| escape_field(&action.column_name())))
            .collect();
        Self {
            kinds: params.iter().map(|param| param.kind.clone()).collect(),
            headers,
        }
    }

    /// Appends a comma and the value of each column of `config` to `row`.
    fn push_values(&self, row: &mut String, config: &Config) {
        for (kind, value) in self.kinds.iter().zip(&config.params) {
            row.push(',');
            row.push_str(&escape_field(&format_value(kind, value)));
        }
        for tick in &config.action_ticks {
            row.push_str(&format!(",{tick}"));
        }
    }
}

/// Returns the header line of columns `first`, the config's columns and columns `last`, with its line ending.
fn header_line(first: &[&str], columns: &ConfigColumns, last: &[&str]) -> String {
    let mut names: Vec<&str> = first.to_vec();
    names.extend(columns.headers.iter().map(String::as_str));
    names.extend_from_slice(last);
    let mut line = names.join(",");
    line.push('\n');
    line
}

/// Returns `value` as a cell, empty for `None`.
fn optional_cell(value: Option<impl ToString>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

/// Writer of `evaluations.csv`, `batches.csv` and, for a genetic algorithm, `generations.csv`.
#[derive(Debug)]
pub struct SearchTablesWriter<W: Write> {
    evaluations: W,
    batches: W,
    /// Writer of `generations.csv`, `None` for a search other than a genetic algorithm.
    generations: Option<W>,
    columns: ConfigColumns,
    /// Whether the search is a PSE. A PSE places candidates in cells and scores none.
    is_pattern_search: bool,
}

impl<W: Write> SearchTablesWriter<W> {
    /// Writes the headers of the tables, for the parameters `params` and the actions `actions`, and flushes each.
    ///
    /// `is_pattern_search` is set for a PSE. A `generations` writer is given for a genetic algorithm alone. A search
    /// stopped before its first batch is told then leaves each table with its header.
    ///
    /// # Errors
    ///
    /// Returns the error of a write or a flush.
    pub fn new(
        mut evaluations: W,
        mut batches: W,
        mut generations: Option<W>,
        params: &[ParamDescriptor],
        actions: &[ActionSpec],
        is_pattern_search: bool,
    ) -> io::Result<Self> {
        let columns = ConfigColumns::new(params, actions);
        let (reading_columns, batch_columns) = if is_pattern_search {
            (&PLACEMENT_COLUMNS[..], &PATTERN_BATCH_COLUMNS[..])
        } else {
            (&OBJECTIVE_COLUMNS[..], &OBJECTIVE_BATCH_COLUMNS[..])
        };
        evaluations.write_all(header_line(&EVALUATION_ID_COLUMNS, &columns, reading_columns).as_bytes())?;
        evaluations.flush()?;
        writeln!(batches, "{}", batch_columns.join(","))?;
        batches.flush()?;
        if let Some(generations) = &mut generations {
            writeln!(generations, "{}", GENERATION_COLUMNS.join(","))?;
            generations.flush()?;
        }
        Ok(Self {
            evaluations,
            batches,
            generations,
            columns,
            is_pattern_search,
        })
    }

    /// Writes the evaluations, batch and finished generations of `update`, and flushes every table.
    ///
    /// # Errors
    ///
    /// Returns the error of a write or a flush.
    pub fn write_batch(&mut self, update: &SearchUpdate) -> io::Result<()> {
        for evaluated in &update.evaluated {
            let row = self.evaluation_row(evaluated);
            self.evaluations.write_all(row.as_bytes())?;
        }
        let mut row = format!("{},{},{}", update.batch, update.evaluations, update.runs);
        if self.is_pattern_search {
            row.push_str(&format!(",{}", update.filled_cells));
        } else {
            let best = update.best.as_ref();
            row.push_str(&format!(
                ",{},{}",
                optional_cell(best.map(|entry| entry.candidate_id)),
                best.map_or_else(String::new, |entry| fmt_f64(entry.objective))
            ));
        }
        writeln!(self.batches, "{row}")?;
        if let Some(generations) = &mut self.generations {
            for generation in &update.generations {
                write_generation(generations, generation)?;
            }
            generations.flush()?;
        }
        self.evaluations.flush()?;
        self.batches.flush()
    }

    fn evaluation_row(&self, evaluated: &EvaluatedCandidate) -> String {
        let [first_parent_id, second_parent_id] = evaluated.origin.parent_ids();
        let mut row = format!(
            "{},{},{},{},{},{},{},{}",
            evaluated.candidate_id,
            evaluated.batch,
            evaluated.origin.as_str(),
            optional_cell(first_parent_id),
            optional_cell(second_parent_id),
            optional_cell(evaluated.origin.reevaluated_id()),
            evaluated.replicate_offset,
            evaluated.replicates
        );
        self.columns.push_values(&mut row, &evaluated.config);
        row.push_str(&format!(",{}", evaluated.failed_count));
        match evaluated.reading {
            EvaluationReading::Objective {
                objective,
                pooled_objective,
                pooled_replicates,
            } => row.push_str(&format!(
                ",{},{},{pooled_replicates}",
                fmt_f64(objective),
                fmt_f64(pooled_objective)
            )),
            EvaluationReading::Placement { placement, new_cell } => match placement {
                Some(placement) => {
                    // Before an automatic range is fixed, the outputs are written without a cell.
                    let cell = placement.cell.map_or_else(
                        || ",,,".to_owned(),
                        |cell| format!("{},{},{},{new_cell}", cell.x_index, cell.y_index, placement.outside),
                    );
                    row.push_str(&format!(",{},{},{cell}", fmt_f64(placement.x), fmt_f64(placement.y)));
                }
                None => row.push_str(",,,,,,"),
            },
        }
        row.push('\n');
        row
    }

    /// Flushes the writers and hands them back, `evaluations.csv`, then `batches.csv`, then `generations.csv`.
    ///
    /// # Errors
    ///
    /// Returns the error of a flush.
    pub fn into_inner(mut self) -> io::Result<(W, W, Option<W>)> {
        self.evaluations.flush()?;
        self.batches.flush()?;
        if let Some(generations) = &mut self.generations {
            generations.flush()?;
        }
        Ok((self.evaluations, self.batches, self.generations))
    }
}

fn write_generation(dest: &mut impl Write, generation: &GenerationSummary) -> io::Result<()> {
    writeln!(
        dest,
        "{},{},{},{}",
        generation.generation,
        fmt_f64(generation.best),
        fmt_f64(generation.median),
        fmt_f64(generation.worst)
    )
}

/// Writes `best.csv` to `dest`, one row per entry of `ranking` in order, ranked from 1, and hands `dest` back.
///
/// `configs` holds the config of every candidate `ranking` names. A candidate without one is left out.
///
/// # Errors
///
/// Returns the error of a write or the flush.
pub(crate) fn write_ranking<W: Write>(
    mut dest: W,
    ranking: &[RankingEntry],
    configs: &BTreeMap<u64, Config>,
    columns: &ConfigColumns,
) -> io::Result<W> {
    dest.write_all(header_line(&RANKING_ID_COLUMNS, columns, &RANKING_COLUMNS).as_bytes())?;
    for (position, entry) in ranking.iter().enumerate() {
        let Some(config) = configs.get(&entry.candidate_id) else {
            continue;
        };
        let mut row = format!("{},{}", position + 1, entry.candidate_id);
        columns.push_values(&mut row, config);
        row.push_str(&format!(
            ",{},{},{},{},{}",
            fmt_f64(entry.objective),
            entry.replicate_count,
            entry.failed_count,
            entry.evaluations,
            entry.first_batch
        ));
        writeln!(dest, "{row}")?;
    }
    dest.flush()?;
    Ok(dest)
}

/// Writes `archive.csv` to `dest`, one row per entry of `archive` in cell order, and hands `dest` back.
///
/// `settings` gives the bounds of each cell, and `configs` the config of every exemplar `archive` names. An entry
/// without a config, or on an axis with no range, is left out.
///
/// # Errors
///
/// Returns the error of a write or the flush.
pub(crate) fn write_archive<W: Write>(
    mut dest: W,
    settings: &PatternSpaceSettings,
    archive: &[ArchiveEntry],
    configs: &BTreeMap<u64, Config>,
    columns: &ConfigColumns,
) -> io::Result<W> {
    dest.write_all(header_line(&ARCHIVE_ID_COLUMNS, columns, &ARCHIVE_COLUMNS).as_bytes())?;
    for entry in archive {
        let (Some(config), Some((x_min, x_max)), Some((y_min, y_max))) = (
            configs.get(&entry.candidate_id),
            settings.x_axis.cell_bounds(entry.cell.x_index),
            settings.y_axis.cell_bounds(entry.cell.y_index),
        ) else {
            continue;
        };
        let mut row = format!(
            "{},{},{},{},{},{},{},{}",
            entry.cell.x_index,
            entry.cell.y_index,
            fmt_f64(x_min),
            fmt_f64(x_max),
            fmt_f64(y_min),
            fmt_f64(y_max),
            entry.hits,
            entry.candidate_id
        );
        columns.push_values(&mut row, config);
        row.push_str(&format!(",{},{}", fmt_f64(entry.x), fmt_f64(entry.y)));
        writeln!(dest, "{row}")?;
    }
    dest.flush()?;
    Ok(dest)
}

/// Course of a search batch by batch, as its events report it or its tables record it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchHistory {
    /// Standing after each batch told, in order.
    pub batches: Vec<BatchStanding>,
    /// Finished generations of a genetic algorithm, in order.
    pub generations: Vec<GenerationSummary>,
    /// Filled cells of a PSE's archive, by cell.
    pub archive: BTreeMap<PatternCell, ArchiveEntry>,
    /// Evaluations of a PSE with an output outside its axis, placed in an edge cell.
    pub outside_count: u64,
    /// Settings a PSE places outputs with, each axis with its range. `None` for any other search, or while an
    /// automatic range waits for the initial samples.
    pub pattern_settings: Option<PatternSpaceSettings>,
}

/// Standing of a search after one batch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatchStanding {
    pub batch: u64,
    /// Evaluations told so far, the batch's included.
    pub evaluations: u64,
    /// Runs of those evaluations.
    pub runs: u64,
    /// Best candidate after the batch, `None` for a PSE.
    pub best_candidate_id: Option<u64>,
    /// Objective of the best candidate, `None` for a PSE or an objective that is not finite.
    pub best_objective: Option<f64>,
    /// Cells a PSE's archive fills after the batch, 0 for any other search.
    pub filled_cells: u64,
}

impl SearchHistory {
    /// Adds the batch `update` reports.
    pub fn push(&mut self, update: &SearchUpdate) {
        let best = update.best.as_ref();
        self.batches.push(BatchStanding {
            batch: update.batch,
            evaluations: update.evaluations,
            runs: update.runs,
            best_candidate_id: best.map(|entry| entry.candidate_id),
            best_objective: best
                .map(|entry| entry.objective)
                .filter(|objective| objective.is_finite()),
            filled_cells: update.filled_cells,
        });
        self.generations.extend_from_slice(&update.generations);
        if update.pattern_settings.is_some() {
            self.pattern_settings.clone_from(&update.pattern_settings);
        }
        for entry in &update.landed_entries {
            self.archive.insert(entry.cell, entry.clone());
        }
        self.outside_count += update
            .evaluated
            .iter()
            .filter(|evaluated| {
                matches!(
                    evaluated.reading,
                    EvaluationReading::Placement {
                        placement: Some(PatternPlacement { outside: true, .. }),
                        ..
                    }
                )
            })
            .count() as u64;
    }

    /// Reads a history from the text of `batches.csv`, `generations.csv` and `evaluations.csv`, each `None` when
    /// the search wrote no such table, and from `search`, the search the tables record.
    ///
    /// A PSE's cells come from `evaluations.csv`, so a search stopped before its end shows the cells it filled. An
    /// automatic range is taken from the rows of the initial samples, as the search took it, and places the rows
    /// written before it was fixed. A value written as an empty cell reads as a value that is not finite. A partial
    /// last line is left out.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] for a table that is not valid CSV, lacks a column the search writes, or holds a field
    /// the column cannot take.
    pub fn read(
        batches: Option<&str>,
        generations: Option<&str>,
        evaluations: Option<&str>,
        search: Option<&SearchSpec>,
    ) -> Result<Self, ReadError> {
        let mut history = Self::default();
        if let Some(text) = batches {
            let table = SearchCsvTable::read(text, BATCHES_FILE)?;
            let (batch, evaluations, runs) = (
                table.column("batch")?,
                table.column("evaluations")?,
                table.column("runs")?,
            );
            let best_candidate_id = table.optional_column("best_candidate_id");
            let best_objective = table.optional_column("best_objective");
            let filled_cells = table.optional_column("filled_cells");
            for record in 0..table.rows.len() {
                history.batches.push(BatchStanding {
                    batch: table.field(record, batch)?,
                    evaluations: table.field(record, evaluations)?,
                    runs: table.field(record, runs)?,
                    best_candidate_id: best_candidate_id
                        .map(|column| table.optional_field(record, column))
                        .transpose()?
                        .flatten(),
                    best_objective: best_objective
                        .map(|column| table.optional_field(record, column))
                        .transpose()?
                        .flatten(),
                    filled_cells: filled_cells
                        .map(|column| table.field(record, column))
                        .transpose()?
                        .unwrap_or(0),
                });
            }
        }
        if let Some(text) = generations {
            let table = SearchCsvTable::read(text, GENERATIONS_FILE)?;
            let generation = table.column("generation")?;
            let (best, median, worst) = (table.column("best")?, table.column("median")?, table.column("worst")?);
            for record in 0..table.rows.len() {
                history.generations.push(GenerationSummary {
                    generation: table.field(record, generation)?,
                    best: table.float(record, best)?,
                    median: table.float(record, median)?,
                    worst: table.float(record, worst)?,
                });
            }
        }
        if let Some(text) = evaluations {
            let table = SearchCsvTable::read(text, EVALUATIONS_FILE)?;
            let candidate_id = table.leading_column("candidate_id")?;
            // The config columns are the model's parameter ids, so every column after them is found by its place.
            // The last column tells a PSE's table from a scored one.
            if table.header.last().map(String::as_str) == PLACEMENT_COLUMNS.last().copied() {
                let [failed, x, y, x_index, y_index, outside, new_cell] = table.trailing_columns(PLACEMENT_COLUMNS)?;
                history.pattern_settings = match search.map(|search| (&search.algorithm, search.max_evaluations)) {
                    Some((SearchAlgorithm::PatternSpaceExploration(settings), max_evaluations)) => {
                        table.ranged_settings(settings, max_evaluations, [candidate_id, x, y])?
                    }
                    _ => None,
                };
                for record in 0..table.rows.len() {
                    table.field::<u64>(record, failed)?;
                    table.optional_field::<bool>(record, new_cell)?;
                    if table.optional_field::<bool>(record, outside)? == Some(true) {
                        history.outside_count += 1;
                    }
                    let written = table
                        .optional_field(record, x_index)?
                        .zip(table.optional_field(record, y_index)?)
                        .map(|(x_index, y_index)| PatternCell { x_index, y_index });
                    let cell = match (written, &history.pattern_settings) {
                        // A row written before an automatic range was taken lands with the range taken since.
                        (None, Some(settings)) => {
                            let (x, y) = (table.float(record, x)?, table.float(record, y)?);
                            (x.is_finite() && y.is_finite())
                                .then(|| settings.locate(x, y).cell)
                                .flatten()
                        }
                        (written, _) => written,
                    };
                    let Some(cell) = cell else {
                        continue;
                    };
                    // The first evaluation to land in a cell is its exemplar, as in the archive.
                    let entry = match history.archive.entry(cell) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => entry.insert(ArchiveEntry {
                            cell,
                            hits: 0,
                            candidate_id: table.field(record, candidate_id)?,
                            x: table.float(record, x)?,
                            y: table.float(record, y)?,
                        }),
                    };
                    entry.hits += 1;
                }
            } else {
                table.check_objectives(candidate_id)?;
            }
        }
        Ok(history)
    }
}

/// Complete records of one search table, read for a [`SearchHistory`].
struct SearchCsvTable {
    file: &'static str,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl SearchCsvTable {
    fn read(text: &str, file: &'static str) -> Result<Self, ReadError> {
        let path = Path::new(file);
        let mut start = 0;
        let mut records = record_ends(text.as_bytes())
            .into_iter()
            .enumerate()
            .map(|(index, end)| {
                let record = parse_one(text, start..end, path, index + 1);
                start = end;
                record
            });
        let header = records.next().transpose()?.unwrap_or_default();
        let rows = records.collect::<Result<Vec<_>, _>>()?;
        if let Some(position) = rows.iter().position(|row| row.len() != header.len()) {
            return Err(ReadError::FieldCount {
                path: path.to_owned(),
                record_number: position + 2,
                found: rows[position].len(),
                expected: header.len(),
            });
        }
        Ok(Self { file, header, rows })
    }

    fn optional_column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|header| header == name)
    }

    /// Returns the position of the column `name` among the columns of `evaluations.csv` before the config.
    fn leading_column(&self, name: &'static str) -> Result<usize, ReadError> {
        self.header
            .iter()
            .take(EVALUATION_ID_COLUMNS.len())
            .position(|header| header == name)
            .ok_or_else(|| self.missing_column(name))
    }

    /// Returns the positions of the columns `names`, the last columns of `evaluations.csv`, after the config.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError::MissingColumn`] for the last of `names` not in its place.
    fn trailing_columns<const N: usize>(&self, names: [&'static str; N]) -> Result<[usize; N], ReadError> {
        let start = self.header.len().saturating_sub(N).max(EVALUATION_ID_COLUMNS.len());
        for (offset, name) in names.into_iter().enumerate().rev() {
            if self.header.get(start + offset).is_none_or(|header| header != name) {
                return Err(self.missing_column(name));
            }
        }
        Ok(std::array::from_fn(|offset| start + offset))
    }

    /// Checks every row of the `evaluations.csv` of a search that scores an objective, whose `candidate_id` is
    /// column `candidate_id`.
    fn check_objectives(&self, candidate_id: usize) -> Result<(), ReadError> {
        let [failed, objective, pooled_objective, pooled_replicates] = self.trailing_columns(OBJECTIVE_COLUMNS)?;
        for record in 0..self.rows.len() {
            self.field::<u64>(record, candidate_id)?;
            self.field::<u64>(record, failed)?;
            self.float(record, objective)?;
            self.float(record, pooled_objective)?;
            self.field::<u64>(record, pooled_replicates)?;
        }
        Ok(())
    }

    fn missing_column(&self, name: &'static str) -> ReadError {
        ReadError::MissingColumn {
            path: Path::new(self.file).to_owned(),
            column: name,
        }
    }

    /// Returns `settings` with each automatic range taken from the rows of the initial samples, as
    /// [`PatternSpaceSettings::with_automatic_ranges`] takes it, or `None` while the table lacks one of them.
    ///
    /// `columns` are the positions of `candidate_id`, `x` and `y`. A PSE under a budget of `max_evaluations`
    /// takes its range from the candidates below [`PatternSpaceSettings::range_sample_count`].
    fn ranged_settings(
        &self,
        settings: &PatternSpaceSettings,
        max_evaluations: u64,
        columns: [usize; 3],
    ) -> Result<Option<PatternSpaceSettings>, ReadError> {
        if settings.has_ranges() {
            return Ok(Some(settings.clone()));
        }
        let [candidate_id, x, y] = columns;
        let sample_count = settings.range_sample_count(max_evaluations);
        let mut told = 0;
        let mut outputs = Vec::new();
        for record in 0..self.rows.len() {
            if self.field::<u64>(record, candidate_id)? >= sample_count {
                continue;
            }
            told += 1;
            let (x, y) = (self.float(record, x)?, self.float(record, y)?);
            if x.is_finite() && y.is_finite() {
                outputs.push((x, y));
            }
        }
        Ok((told >= sample_count).then(|| settings.with_automatic_ranges(&outputs)))
    }

    fn column(&self, name: &'static str) -> Result<usize, ReadError> {
        self.optional_column(name).ok_or_else(|| self.missing_column(name))
    }

    fn bad_field(&self, record: usize, column: usize) -> ReadError {
        ReadError::BadField {
            path: Path::new(self.file).to_owned(),
            record_number: record + 2,
            column: self.header[column].clone(),
            text: self.rows[record][column].clone(),
        }
    }

    /// Returns the value in a field.
    fn field<T: FromStr>(&self, record: usize, column: usize) -> Result<T, ReadError> {
        self.rows[record][column]
            .parse()
            .ok()
            .ok_or_else(|| self.bad_field(record, column))
    }

    /// Returns the value in a field, `None` for an empty one.
    fn optional_field<T: FromStr>(&self, record: usize, column: usize) -> Result<Option<T>, ReadError> {
        if self.rows[record][column].is_empty() {
            Ok(None)
        } else {
            self.field(record, column).map(Some)
        }
    }

    /// Returns the float in a field, `NaN` for an empty one.
    fn float(&self, record: usize, column: usize) -> Result<f64, ReadError> {
        Ok(self.optional_field(record, column)?.unwrap_or(f64::NAN))
    }
}

#[cfg(test)]
mod tests {
    use henad_compute::entry::register_grid_model;
    use henad_core::authoring::model::grid_model::GridModel;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
    use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec};
    use henad_core::explore::spec::SweepSpec;
    use henad_core::export::csv::CsvError;
    use henad_core::grid::Grid2D;
    use henad_core::helpers::{bool_param, f32_param, u32_param};
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::topology::NeighborhoodKind;
    use henad_core::view::{StatDescriptor, StatValue};

    use std::fs::File;
    use std::io::BufWriter;

    use super::{SearchHistory, SearchTablesWriter};
    use crate::output::read::ReadError;
    use crate::output::{BATCHES_FILE, EVALUATIONS_FILE, MANIFEST_FILE, RUNS_FILE};
    use crate::progress::{NoProgress, Progress, ProgressEvent};
    use crate::result_set::ResultSet;
    use crate::tests::support::{ScratchDir, sweep_options, sweep_with};

    /// Model whose parameters share their ids with the columns `evaluations.csv` writes after a config.
    ///
    /// Its stats are `x` and `y` scaled to 0 to 200. Each column then holds another value than the parameter it
    /// shares a name with.
    struct TrailingNames;

    impl GridModel for TrailingNames {
        const NAME: &'static str = "Trailing Names";
        const ID: &'static str = "trailing_names";
        const DESCRIPTION: &'static str = "A model with parameters named like search columns, registered only by tests";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF], [0xFF, 0xFF, 0xFF, 0xFF]];
        const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
        const STATS: &'static [StatDescriptor] = &[
            StatDescriptor::new("Left", [0xFF, 0xFF, 0xFF, 0xFF]),
            StatDescriptor::new("Right", [0xFF, 0xFF, 0xFF, 0xFF]),
        ];
        type Params = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            vec![
                f32_param("x", "X", 0.5, 0.0, 1.0, None),
                f32_param("y", "Y", 0.5, 0.0, 1.0, None),
                u32_param("x_index", "X index", 7, 0, 7),
                u32_param("y_index", "Y index", 7, 0, 7),
                bool_param("outside", "Outside", true),
            ]
        }

        fn from_params(_params: &[ParamValue]) {}

        fn init(grid: &mut Grid2D<u8>, params: &[ParamValue], _rng: &mut u64) {
            for (cell, param) in grid.current_mut().iter_mut().zip(params) {
                if let ParamValue::F32(value) = *param {
                    *cell = (value * 200.0) as u8;
                }
            }
        }

        fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
            cell
        }

        fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
            grid.current()[..2]
                .iter()
                .map(|&cell| StatValue::Scalar(f64::from(cell)))
                .collect()
        }
    }

    /// Progress that keeps the course of a search from its batches.
    #[derive(Default)]
    struct HistoryRecorder(SearchHistory);

    impl Progress for HistoryRecorder {
        fn report(&mut self, event: &ProgressEvent<'_>) {
            if let ProgressEvent::SearchBatchTold(update) = event {
                self.0.push(update);
            }
        }
    }

    /// Returns a search of [`TrailingNames`] with `algorithm`, 12 evaluations in batches of 4 over `x` and `y`.
    fn trailing_names_search(algorithm: SearchAlgorithm) -> SweepSpec {
        let mut spec = SweepSpec::new(TrailingNames::ID);
        spec.fixed = vec![
            ("grid_width".to_owned(), "4".to_owned()),
            ("grid_height".to_owned(), "4".to_owned()),
        ];
        spec.run.steps = 2;
        spec.measure.default_reducers = false;
        spec.measure.reducers = ["Left:max", "Right:max"]
            .map(|raw| raw.parse().expect("a valid reducer"))
            .to_vec();
        let objective = match algorithm {
            SearchAlgorithm::PatternSpaceExploration(_) => None,
            SearchAlgorithm::Random | SearchAlgorithm::HillClimb(_) | SearchAlgorithm::Genetic(_) => Some(Objective {
                column: "Left:max".to_owned(),
                goal: Goal::Minimize,
                aggregate: Aggregate::Median,
            }),
        };
        let unit = || LevelSpec::Range {
            min: 0.0,
            max: 1.0,
            step: None,
        };
        spec.search = Some(SearchSpec {
            algorithm,
            max_evaluations: 12,
            batch_size: 4,
            objective,
            space: vec![FactorSpec::param("x", unit()), FactorSpec::param("y", unit())],
        });
        spec
    }

    #[test]
    fn parameters_named_like_the_placement_columns_leave_the_history_alone() {
        let entry = register_grid_model::<TrailingNames>();
        let pattern = |x_axis, y_axis| {
            SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings {
                initial_samples: 4,
                ..PatternSpaceSettings::new(x_axis, y_axis)
            })
        };
        let scratch = ScratchDir::new("trailing-names");
        for (name, algorithm) in [
            ("random", SearchAlgorithm::Random),
            (
                "pse",
                pattern(
                    PatternAxis::bounded("Left:max", 0.0, 200.0, 4),
                    PatternAxis::bounded("Right:max", 0.0, 200.0, 4),
                ),
            ),
            (
                "pse-automatic",
                pattern(
                    PatternAxis::automatic("Left:max", 4),
                    PatternAxis::automatic("Right:max", 4),
                ),
            ),
        ] {
            let output_dir = scratch.path().join(name);
            let mut recorder = HistoryRecorder::default();
            let spec = trailing_names_search(algorithm);
            sweep_with(&entry, None, &spec, &output_dir, &sweep_options(false), &mut recorder)
                .expect("the search runs");
            let history = ResultSet::open_dir(&output_dir, usize::MAX)
                .expect("the folder reads back")
                .search_history()
                .expect("the tables read")
                .expect("a search has a history");
            assert_eq!(history, recorder.0, "{name}: the tables hold what the events reported");
            assert_eq!(
                history.archive.is_empty(),
                name == "random",
                "{name}: only a PSE fills cells"
            );
        }
    }

    #[test]
    fn a_search_stopped_during_its_first_batch_reads_back() {
        let entry = register_grid_model::<TrailingNames>();
        let scratch = ScratchDir::new("search-first-batch");
        let output_dir = scratch.path().join("search");
        let spec = trailing_names_search(SearchAlgorithm::Random);
        sweep_with(&entry, None, &spec, &output_dir, &sweep_options(false), &mut NoProgress).expect("the search runs");
        // The process ended during its first batch, with its runs written and the batch not yet told.
        let create = |file| BufWriter::new(File::create(output_dir.join(file)).expect("a table is created"));
        let tables = SearchTablesWriter::new(
            create(EVALUATIONS_FILE),
            create(BATCHES_FILE),
            None,
            entry.param_descriptors(),
            &[],
            false,
        )
        .expect("the headers write");
        #[expect(clippy::mem_forget, reason = "a killed process drops nothing")]
        std::mem::forget(tables);
        for file in [EVALUATIONS_FILE, BATCHES_FILE] {
            let text = std::fs::read_to_string(output_dir.join(file)).expect("the table reads");
            assert!(
                text.ends_with('\n') && text.lines().count() == 1,
                "{file} holds its header alone"
            );
        }
        let history = |set: &ResultSet| {
            set.search_history()
                .expect("the tables read")
                .expect("a search has a history")
        };
        let set = ResultSet::open_dir(&output_dir, usize::MAX).expect("the folder reads back");
        assert!(history(&set).batches.is_empty());

        // A process that ended before the headers were flushed left the tables empty.
        for file in [EVALUATIONS_FILE, BATCHES_FILE] {
            std::fs::write(output_dir.join(file), "").expect("a table is emptied");
        }
        let set = ResultSet::open_dir(&output_dir, usize::MAX).expect("the folder reads back");
        assert!(history(&set).batches.is_empty());
        let picked = [MANIFEST_FILE, RUNS_FILE, EVALUATIONS_FILE, BATCHES_FILE]
            .into_iter()
            .map(|file| {
                let bytes = std::fs::read(output_dir.join(file)).expect("the file reads");
                (file.to_owned(), bytes)
            })
            .collect();
        let set = ResultSet::from_files(picked, usize::MAX).expect("the picked files read");
        assert!(history(&set).batches.is_empty());
    }

    #[test]
    fn a_malformed_record_is_reported_where_it_is() {
        let batches =
            "batch,evaluations,runs,best_candidate_id,best_objective\n0,4,4,1,2.5\n1,8,8,1\"x,2.5\n2,12,12,1,2.5\n";
        let error = SearchHistory::read(Some(batches), None, None, None).expect_err("a quote inside a field");
        assert!(
            matches!(
                error,
                ReadError::Csv {
                    record_number: 3,
                    source: CsvError::MisplacedQuote { line: 3 },
                    ..
                }
            ),
            "{error:?}"
        );
        let two_lines = batches.replacen("2.5", "\"2.5\n\"", 1);
        let error = SearchHistory::read(Some(&two_lines), None, None, None).expect_err("a quote inside a field");
        assert!(
            matches!(
                error,
                ReadError::Csv {
                    record_number: 3,
                    source: CsvError::MisplacedQuote { line: 4 },
                    ..
                }
            ),
            "a quoted line feed in record 2 puts record 3 on line 4: {error:?}"
        );

        let header = "candidate_id,batch,origin,first_parent_id,second_parent_id,reevaluated_id,replicate_offset,\
                      replicates,rate";
        let scored =
            format!("{header},failed,objective,pooled_objective,pooled_replicates\n0,0,random,,,,0,2,0.1,0,1.5,,2\n");
        let history = SearchHistory::read(None, None, Some(&scored), None).expect("a valid table");
        assert!(history.archive.is_empty());
        let error = SearchHistory::read(None, None, Some(&scored.replace("1.5,,2", "one,,2")), None)
            .expect_err("an objective that is not a number");
        assert!(
            matches!(&error, ReadError::BadField { record_number: 2, column, .. } if column == "objective"),
            "{error:?}"
        );
        let bare = format!("{header}\n0,0,random,,,,0,2,0.1\n");
        let error = SearchHistory::read(None, None, Some(&bare), None).expect_err("no columns after the config");
        assert!(
            matches!(
                error,
                ReadError::MissingColumn {
                    column: "pooled_replicates",
                    ..
                }
            ),
            "{error:?}"
        );
    }
}
