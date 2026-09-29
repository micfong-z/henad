//! Writer of `summary.csv`, statistics over the replicates of each config, rebuilt from `runs.csv`.
//!
//! A row holds a config's id, block and values, its run counts, the mean tick its runs ended on, and five columns
//! for each reducer `R` of `runs.csv`: `R:mean`, `R:sd`, `R:n`, `R:ci95_low` and `R:ci95_high`. Statistics cover the
//! runs that did not fail. `n` counts the finite values, and a statistic with too few of them is an empty cell.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::ops::Range;

use henad_core::explore::outcome::RunStatus;
use henad_core::explore::summary::{ReplicateSummary, SummaryAccumulator};
use henad_core::export::csv::{CsvError, escape_field, fmt_f64, parse_records};

use crate::output::runs_csv::{NOTE_COLUMN, OUTCOME_COLUMNS};

/// Statistics written for each reducer, as suffixes of its column name.
pub const STATISTICS: [&str; 5] = ["mean", "sd", "n", "ci95_low", "ci95_high"];

/// Text of a `runs.csv` that cannot be summarized.
#[derive(Debug)]
pub enum SummaryError {
    /// Text that is not valid CSV.
    Csv(CsvError),
    /// A header without the column `column`.
    MissingColumn { column: &'static str },
    /// Record `record_number`, counted from 1 for the header, with a different number of fields from the header.
    FieldCount {
        record_number: usize,
        /// Fields in the record.
        found: usize,
        /// Fields in the header.
        expected: usize,
    },
    /// Field `column` of record `record_number`, holding `text` the column cannot take.
    BadField {
        record_number: usize,
        column: String,
        text: String,
    },
    /// Writing the summary failed.
    Io(io::Error),
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Csv(_) => f.write_str("runs.csv is not valid CSV"),
            Self::MissingColumn { column } => write!(f, "runs.csv has no '{column}' column"),
            Self::FieldCount {
                record_number,
                found,
                expected,
            } => write!(
                f,
                "record {record_number} of runs.csv has {found} fields, expected {expected}"
            ),
            Self::BadField {
                record_number,
                column,
                text,
            } => {
                write!(
                    f,
                    "record {record_number} of runs.csv has invalid value '{text}' in column '{column}'"
                )
            }
            Self::Io(_) => f.write_str("cannot write the summary"),
        }
    }
}

impl std::error::Error for SummaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Csv(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::MissingColumn { .. } | Self::FieldCount { .. } | Self::BadField { .. } => None,
        }
    }
}

impl From<io::Error> for SummaryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Positions of the columns of a `runs.csv` header that the summary reads.
///
/// Note that a parameter can share its name with a later column. Those columns are found from the end of the header.
struct RunsLayout {
    config_id: usize,
    block: usize,
    /// Parameter and action columns, the ones between `run_key` and `status`.
    values: Range<usize>,
    status: usize,
    ticks: usize,
    /// Reducer columns, the ones between `steps_per_s` and `note`.
    reducers: Range<usize>,
}

impl RunsLayout {
    fn read(header: &[String]) -> Result<Self, SummaryError> {
        let find_first = |column: &'static str| {
            header
                .iter()
                .position(|name| name == column)
                .ok_or(SummaryError::MissingColumn { column })
        };
        let find_last = |column: &'static str| {
            header
                .iter()
                .rposition(|name| name == column)
                .ok_or(SummaryError::MissingColumn { column })
        };
        let last_outcome = OUTCOME_COLUMNS[OUTCOME_COLUMNS.len() - 1];
        let config_id = find_first("config_id")?;
        let block = find_first("block")?;
        let values_start = find_first("run_key")? + 1;
        let status = find_last("status")?;
        Ok(Self {
            config_id,
            block,
            values: values_start..status,
            status,
            ticks: find_last("ticks")?,
            reducers: find_last(last_outcome)? + 1..find_last(NOTE_COLUMN)?,
        })
    }
}

/// Block and values of one config, as `runs.csv` writes them.
struct ConfigFields<'r> {
    block: &'r str,
    values: &'r [String],
}

/// Reads `runs`, the text of a `runs.csv`, writes the summary of every config in it to `dest`, and hands `dest` back.
///
/// Configs are written in id order. Note that the statistics depend on the order of the runs in `runs`, down to the
/// last bit.
///
/// # Errors
///
/// Returns [`SummaryError`] when `runs` is not a `runs.csv` or a write fails.
pub fn write_summary<W: Write>(runs: &str, mut dest: W) -> Result<W, SummaryError> {
    let records = parse_records(runs).map_err(SummaryError::Csv)?;
    let Some((header, rows)) = records.split_first() else {
        return Err(SummaryError::MissingColumn { column: "run_id" });
    };
    let layout = RunsLayout::read(header)?;
    let reducer_names = &header[layout.reducers.clone()];

    let mut accumulator = SummaryAccumulator::new(reducer_names.len());
    let mut configs: BTreeMap<u64, ConfigFields<'_>> = BTreeMap::new();
    let mut reducers = Vec::with_capacity(reducer_names.len());
    for (index, row) in rows.iter().enumerate() {
        let record_number = index + 2;
        if row.len() != header.len() {
            return Err(SummaryError::FieldCount {
                record_number,
                found: row.len(),
                expected: header.len(),
            });
        }
        let bad_field = |column: usize| SummaryError::BadField {
            record_number,
            column: header[column].clone(),
            text: row[column].clone(),
        };
        let config_id: u64 = row[layout.config_id]
            .parse()
            .ok()
            .ok_or_else(|| bad_field(layout.config_id))?;
        let status: RunStatus = row[layout.status]
            .parse()
            .ok()
            .ok_or_else(|| bad_field(layout.status))?;
        let ticks: u64 = row[layout.ticks].parse().ok().ok_or_else(|| bad_field(layout.ticks))?;
        reducers.clear();
        for column in layout.reducers.clone() {
            let value = match row[column].as_str() {
                "" => None,
                text => Some(text.parse::<f64>().ok().ok_or_else(|| bad_field(column))?),
            };
            reducers.push(value);
        }
        accumulator.push(config_id, status, ticks, &reducers);
        configs.entry(config_id).or_insert_with(|| ConfigFields {
            block: &row[layout.block],
            values: &row[layout.values.clone()],
        });
    }

    let mut columns: Vec<String> = ["config_id", "block"].map(str::to_owned).to_vec();
    columns.extend(header[layout.values.clone()].iter().map(|name| escape_field(name)));
    columns.extend(["runs", "ok", "failed", "ticks:mean"].map(str::to_owned));
    for name in reducer_names {
        columns.extend(STATISTICS.map(|statistic| escape_field(&format!("{name}:{statistic}"))));
    }
    writeln!(dest, "{}", columns.join(","))?;
    for summary in accumulator.rows() {
        let Some(fields) = configs.get(&summary.config_id) else {
            continue;
        };
        let mut cells = vec![summary.config_id.to_string(), escape_field(fields.block)];
        cells.extend(fields.values.iter().map(|value| escape_field(value)));
        cells.extend([summary.runs, summary.ok, summary.failed].map(|count| count.to_string()));
        cells.push(optional_cell(summary.ticks.mean));
        for reducer in &summary.reducers {
            cells.extend(statistic_cells(reducer));
        }
        writeln!(dest, "{}", cells.join(","))?;
    }
    dest.flush()?;
    Ok(dest)
}

/// Returns the cells of `summary` in [`STATISTICS`] order.
fn statistic_cells(summary: &ReplicateSummary) -> [String; 5] {
    [
        optional_cell(summary.mean),
        optional_cell(summary.standard_deviation),
        summary.n.to_string(),
        optional_cell(summary.ci95.map(|(low, _)| low)),
        optional_cell(summary.ci95.map(|(_, high)| high)),
    ]
}

/// Returns `value` as a cell, empty for `None`.
fn optional_cell(value: Option<f64>) -> String {
    value.map_or_else(String::new, fmt_f64)
}

#[cfg(test)]
mod tests {
    use super::{SummaryError, write_summary};

    const RUNS: &str = "\
run_id,config_id,block,rep,seed,run_key,rate,\"a, b\",status,stop_reason,ticks,population,build_ms,wall_ms,steps_per_s,Infected:max,\"Speed.[0, 1):min\",note
0,0,0,0,11,00000000000000aa,0.1,x,ok,steps,10,64,1,2,5000,1,,
1,0,0,1,12,00000000000000ab,0.1,x,ok,steps,10,64,1,2,5000,2,4,
2,0,0,2,13,00000000000000ac,0.1,x,non_finite,steps,10,64,1,2,5000,3,,Infected is not finite at tick 5
3,0,0,3,14,00000000000000ad,0.1,x,ok,steps,10,64,1,2,5000,4,,
4,1,1,0,11,00000000000000ba,0.2,\"y, z\",panicked,fault,3,64,1,2,1500,99,,\"while stepping, it panicked\"
5,1,1,1,12,00000000000000bb,0.2,\"y, z\",ok,steps,10,64,1,2,5000,7,1,
";

    fn summary(runs: &str) -> Result<String, SummaryError> {
        write_summary(runs, Vec::new()).map(|bytes| String::from_utf8(bytes).expect("the rows are UTF-8"))
    }

    #[test]
    fn the_summary_matches_hand_computed_statistics() {
        let text = summary(RUNS).expect("a valid runs.csv");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "config_id,block,rate,\"a, b\",runs,ok,failed,ticks:mean,\
             Infected:max:mean,Infected:max:sd,Infected:max:n,Infected:max:ci95_low,Infected:max:ci95_high,\
             \"Speed.[0, 1):min:mean\",\"Speed.[0, 1):min:sd\",\"Speed.[0, 1):min:n\",\
             \"Speed.[0, 1):min:ci95_low\",\"Speed.[0, 1):min:ci95_high\""
        );
        let first: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(first[..8], ["0", "0", "0.1", "x", "4", "3", "0", "10"]);
        assert_eq!(first[8], "2.5", "the mean of 1, 2, 3 and 4");
        let sd: f64 = first[9].parse().expect("a spread");
        assert!((sd - 1.290_994).abs() < 1e-6, "sd {sd}");
        assert_eq!(first[10], "4");
        let low: f64 = first[11].parse().expect("an interval");
        assert!((2.5 - low - 2.054_260).abs() < 1e-6, "half-width {}", 2.5 - low);
        assert_eq!(first[13..], ["4", "", "1", "", ""], "one finite value has no spread");

        assert_eq!(
            lines[2], "1,1,0.2,\"y, z\",2,1,1,10,7,,1,,,1,,1,,",
            "the failed run is counted and left out"
        );
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn a_parameter_named_like_a_later_column_is_read_as_a_parameter() {
        let runs = "\
run_id,config_id,block,rep,seed,run_key,ticks,status,note,status,stop_reason,ticks,population,build_ms,wall_ms,steps_per_s,Infected:max,note
0,0,0,0,11,00000000000000aa,5,x,y,ok,steps,10,64,1,2,5000,1,
1,0,0,1,12,00000000000000ab,5,x,y,ok,steps,10,64,1,2,5000,3,
";
        let text = summary(runs).expect("a valid runs.csv");
        let lines: Vec<&str> = text.lines().collect();
        assert!(
            lines[0].starts_with("config_id,block,ticks,status,note,runs,ok,failed,ticks:mean,Infected:max:mean,"),
            "{}",
            lines[0]
        );
        let first: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(first[..10], ["0", "0", "5", "x", "y", "2", "2", "0", "10", "2"]);
    }

    #[test]
    fn a_table_that_is_not_runs_csv_is_refused() {
        assert!(matches!(
            summary("run_id,config_id\n0,0\n"),
            Err(SummaryError::MissingColumn { column: "block" })
        ));
        let short = RUNS.replace("5,1,1,1,12,", "5,1,1,12,");
        let error = summary(&short).expect_err("a short record");
        assert!(matches!(
            error,
            SummaryError::FieldCount {
                record_number: 7,
                found: 17,
                expected: 18
            }
        ));
        assert_eq!(error.to_string(), "record 7 of runs.csv has 17 fields, expected 18");
        let bad = RUNS.replace("ok,steps,10,64,1,2,5000,4,", "ok,steps,ten,64,1,2,5000,4,");
        assert!(
            matches!(summary(&bad), Err(SummaryError::BadField { record_number: 5, column, .. }) if column == "ticks")
        );
    }
}
