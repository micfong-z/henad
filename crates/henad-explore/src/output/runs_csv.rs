//! Writer of `runs.csv`, one row per run in plan order.
//!
//! A row holds the ids, seed and key of a run, the value of every parameter and the tick of every action, the status
//! and timing of the run, and one value per reducer. The note is the last column. It holds the actions the model
//! refused, then a fault message, the timeout or the first value that was not finite.

use std::io::{self, Write};

use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::Config;
use henad_core::explore::spec::ActionSpec;
use henad_core::explore::value::format_value;
use henad_core::export::csv::{escape_field, fmt_f64};
use henad_core::params::{ParamDescriptor, ParamKind};

/// Headers of the columns before the parameters.
pub const ID_COLUMNS: [&str; 6] = ["run_id", "config_id", "block", "rep", "seed", "run_key"];

/// Headers of the columns between the parameters and the reducers.
pub const OUTCOME_COLUMNS: [&str; 7] = [
    "status",
    "stop_reason",
    "ticks",
    "population",
    "build_ms",
    "wall_ms",
    "steps_per_s",
];

/// Headers of the columns that depend on the machine and its load, and on nothing in the plan.
pub const TIMING_COLUMNS: [&str; 3] = ["build_ms", "wall_ms", "steps_per_s"];

/// Header of the last column.
pub const NOTE_COLUMN: &str = "note";

/// Returns the names of the columns of `runs.csv` for the parameters `params`, the actions `actions` and the reducer
/// columns named `reducers`, before CSV escaping.
pub(crate) fn column_names(params: &[ParamDescriptor], actions: &[ActionSpec], reducers: &[String]) -> Vec<String> {
    let mut names: Vec<String> = ID_COLUMNS.iter().map(|&column| column.to_owned()).collect();
    names.extend(params.iter().map(|param| param.id.to_owned()));
    names.extend(actions.iter().map(ActionSpec::column_name));
    names.extend(OUTCOME_COLUMNS.iter().map(|&column| column.to_owned()));
    names.extend(reducers.iter().cloned());
    names.push(NOTE_COLUMN.to_owned());
    names
}

/// Returns the header line of `runs.csv` for the column names `names`, escaped and with its line ending.
pub(crate) fn header_line(names: &[String]) -> String {
    let mut line = names
        .iter()
        .map(|name| escape_field(name))
        .collect::<Vec<_>>()
        .join(",");
    line.push('\n');
    line
}

/// Writer of `runs.csv`.
#[derive(Debug)]
pub struct RunsWriter<W: Write> {
    dest: W,
    /// Kind of every parameter, in descriptor order.
    kinds: Vec<ParamKind>,
}

impl<W: Write> RunsWriter<W> {
    /// Writes the header for the parameters `params`, the actions `actions` and the reducer columns named
    /// `reducers`.
    ///
    /// # Errors
    ///
    /// Returns the error of the write.
    pub fn new(
        mut dest: W,
        params: &[ParamDescriptor],
        actions: &[ActionSpec],
        reducers: &[String],
    ) -> io::Result<Self> {
        dest.write_all(header_line(&column_names(params, actions, reducers)).as_bytes())?;
        Ok(Self::appending(dest, params))
    }

    /// Returns a writer that adds rows to `dest`, a `runs.csv` whose header is already written, for the parameters
    /// `params`.
    pub fn appending(dest: W, params: &[ParamDescriptor]) -> Self {
        Self {
            dest,
            kinds: params.iter().map(|param| param.kind.clone()).collect(),
        }
    }

    /// Writes the row of `outcome`, a run of `config`.
    ///
    /// # Errors
    ///
    /// Returns the error of the write.
    pub fn write_run(&mut self, outcome: &RunOutcome, config: &Config) -> io::Result<()> {
        let run = &outcome.run;
        let mut row = format!(
            "{},{},{},{},{},{:016x}",
            run.run_id, run.config_id, config.block, run.rep, run.seed, outcome.run_key
        );
        for (kind, value) in self.kinds.iter().zip(&config.params) {
            row.push(',');
            row.push_str(&escape_field(&format_value(kind, value)));
        }
        for tick in &config.action_ticks {
            row.push_str(&format!(",{tick}"));
        }
        let timings = [outcome.build_ms, outcome.wall_ms, outcome.steps_per_s()].map(format_timing);
        row.push_str(&format!(
            ",{},{},{},{},{},{},{}",
            outcome.status.as_str(),
            outcome.stop_reason.as_str(),
            outcome.ticks,
            outcome.population,
            timings[0],
            timings[1],
            timings[2],
        ));
        for value in &outcome.reducers {
            row.push(',');
            row.push_str(&value.map_or_else(String::new, fmt_f64));
        }
        row.push(',');
        row.push_str(&escape_field(outcome.note.as_deref().unwrap_or_default()));
        writeln!(self.dest, "{row}")
    }

    /// Flushes the rows written so far.
    ///
    /// # Errors
    ///
    /// Returns the error of the flush.
    pub fn flush(&mut self) -> io::Result<()> {
        self.dest.flush()
    }

    /// Flushes the writer and hands it back.
    ///
    /// # Errors
    ///
    /// Returns the error of the flush.
    pub fn into_inner(mut self) -> io::Result<W> {
        self.dest.flush()?;
        Ok(self.dest)
    }
}

/// Returns a time or a rate rounded to three decimal places, or an empty cell for a value that is not finite.
fn format_timing(value: f64) -> String {
    fmt_f64((value * 1000.0).round() / 1000.0)
}

#[cfg(test)]
mod tests {
    use henad_core::explore::measure::SeriesBuffer;
    use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus, StopReason};
    use henad_core::explore::plan::Config;
    use henad_core::explore::spec::ActionSpec;
    use henad_core::helpers::{choice_param, f32_param};
    use henad_core::params::ParamValue;

    use super::{RunsWriter, format_timing};

    const NETWORKS: &[&str] = &["Random", "Geometric"];

    #[test]
    fn a_row_holds_every_column_in_header_order() {
        let params = [
            f32_param("spread, chance", "Spread chance", 0.2, 0.0, 1.0, None),
            choice_param("network", "Network", NETWORKS, 0),
        ];
        let reducers = ["Infected:max".to_owned(), "Speed.[0, 1):min".to_owned()];
        let actions = [ActionSpec {
            name: "second, wave".to_owned(),
            ..ActionSpec::new("seed_outbreak", 40)
        }];
        let mut writer = RunsWriter::new(Vec::new(), &params, &actions, &reducers).expect("a vector takes every write");
        let outcome = RunOutcome {
            run: PlannedRun {
                run_id: 7,
                config_id: 2,
                rep: 1,
                seed: 99,
            },
            run_key: 0xab,
            status: RunStatus::Panicked,
            stop_reason: StopReason::Fault,
            ticks: 40,
            population: 256,
            build_ms: 1.23456,
            wall_ms: 8.0,
            reducers: vec![Some(12.5), None],
            series: SeriesBuffer::new(2),
            note: Some("while stepping, \"x\" panicked".to_owned()),
        };
        let config = Config {
            block: 1,
            params: vec![ParamValue::F32(0.1), ParamValue::Choice(1)],
            action_ticks: vec![25],
        };
        writer.write_run(&outcome, &config).expect("a vector takes every write");
        let text = String::from_utf8(writer.into_inner().expect("a vector flushes")).expect("the rows are UTF-8");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            "run_id,config_id,block,rep,seed,run_key,\"spread, chance\",network,\"action.second, wave\",status,\
             stop_reason,ticks,population,build_ms,wall_ms,steps_per_s,Infected:max,\"Speed.[0, 1):min\",note"
        );
        assert_eq!(
            lines[1],
            "7,2,1,1,99,00000000000000ab,0.1,Geometric,25,panicked,fault,40,256,1.235,8,5000,12.5,,\
             \"while stepping, \"\"x\"\" panicked\""
        );
    }

    #[test]
    fn a_timing_keeps_three_decimal_places() {
        assert_eq!(format_timing(0.1 + 0.2), "0.3");
        assert_eq!(format_timing(2.0), "2");
        assert_eq!(format_timing(f64::INFINITY), "");
        assert_eq!(format_timing(f64::NAN), "");
    }
}
