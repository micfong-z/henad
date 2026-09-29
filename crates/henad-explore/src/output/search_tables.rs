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
use henad_core::export::csv::{escape_field, fmt_f64, parse_records};
use henad_core::params::{ParamDescriptor, ParamKind};

use crate::output::read::{ReadError, record_ends};
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
    /// Writes the headers of the tables, for the parameters `params` and the actions `actions`.
    ///
    /// `is_pattern_search` is set for a PSE. A `generations` writer is given for a genetic algorithm alone.
    ///
    /// # Errors
    ///
    /// Returns the error of a write.
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
        writeln!(batches, "{}", batch_columns.join(","))?;
        if let Some(generations) = &mut generations {
            writeln!(generations, "{}", GENERATION_COLUMNS.join(","))?;
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
pub fn write_ranking<W: Write>(
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
pub fn write_archive<W: Write>(
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
                    batch: table.number(record, batch)?,
                    evaluations: table.number(record, evaluations)?,
                    runs: table.number(record, runs)?,
                    best_candidate_id: best_candidate_id
                        .map(|column| table.optional_number(record, column))
                        .transpose()?
                        .flatten(),
                    best_objective: best_objective
                        .map(|column| table.optional_number(record, column))
                        .transpose()?
                        .flatten(),
                    filled_cells: filled_cells
                        .map(|column| table.number(record, column))
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
                    generation: table.number(record, generation)?,
                    best: table.float(record, best)?,
                    median: table.float(record, median)?,
                    worst: table.float(record, worst)?,
                });
            }
        }
        if let Some(text) = evaluations {
            let table = SearchCsvTable::read(text, EVALUATIONS_FILE)?;
            if let Some(x_index) = table.optional_column("x_index") {
                let (candidate_id, y_index) = (table.column("candidate_id")?, table.column("y_index")?);
                let (x, y, outside_column) = (table.column("x")?, table.column("y")?, table.column("outside")?);
                history.pattern_settings = match search.map(|search| (&search.algorithm, search.max_evaluations)) {
                    Some((SearchAlgorithm::PatternSpaceExploration(settings), max_evaluations)) => {
                        table.ranged_settings(settings, max_evaluations, [candidate_id, x, y])?
                    }
                    _ => None,
                };
                for record in 0..table.rows.len() {
                    if table.rows[record][outside_column] == "true" {
                        history.outside_count += 1;
                    }
                    let written = table
                        .optional_number(record, x_index)?
                        .zip(table.optional_number(record, y_index)?)
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
                            candidate_id: table.number(record, candidate_id)?,
                            x: table.float(record, x)?,
                            y: table.float(record, y)?,
                        }),
                    };
                    entry.hits += 1;
                }
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
        let complete = record_ends(text.as_bytes()).last().copied().unwrap_or(0);
        let mut records = parse_records(&text[..complete]).map_err(|source| ReadError::Csv {
            path: Path::new(file).to_owned(),
            record_number: 1,
            source,
        })?;
        let header = if records.is_empty() {
            Vec::new()
        } else {
            records.remove(0)
        };
        if let Some(position) = records.iter().position(|row| row.len() != header.len()) {
            return Err(ReadError::FieldCount {
                path: Path::new(file).to_owned(),
                record_number: position + 2,
                found: records[position].len(),
                expected: header.len(),
            });
        }
        Ok(Self {
            file,
            header,
            rows: records,
        })
    }

    fn optional_column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|header| header == name)
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
            if self.number::<u64>(record, candidate_id)? >= sample_count {
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
        self.optional_column(name).ok_or_else(|| ReadError::MissingColumn {
            path: Path::new(self.file).to_owned(),
            column: name,
        })
    }

    fn bad_field(&self, record: usize, column: usize) -> ReadError {
        ReadError::BadField {
            path: Path::new(self.file).to_owned(),
            record_number: record + 2,
            column: self.header[column].clone(),
            text: self.rows[record][column].clone(),
        }
    }

    fn number<T: FromStr>(&self, record: usize, column: usize) -> Result<T, ReadError> {
        self.rows[record][column]
            .parse()
            .ok()
            .ok_or_else(|| self.bad_field(record, column))
    }

    /// Returns the number in a field, `None` for an empty one.
    fn optional_number<T: FromStr>(&self, record: usize, column: usize) -> Result<Option<T>, ReadError> {
        if self.rows[record][column].is_empty() {
            Ok(None)
        } else {
            self.number(record, column).map(Some)
        }
    }

    /// Returns the float in a field, `NaN` for an empty one.
    fn float(&self, record: usize, column: usize) -> Result<f64, ReadError> {
        Ok(self.optional_number(record, column)?.unwrap_or(f64::NAN))
    }
}
