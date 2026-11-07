//! Writer of `series.csv`, the sampled stat rows of every run.
//!
//! The rows of one run are contiguous, and runs come in plan order. The stat columns are the columns that
//! `--export-stats` writes, after a `run_id` column. A model with a time scale writes a `time` column after `tick`, as
//! `--export-stats` does.

use std::io::{self, Write};

use henad_core::explore::measure::SeriesBuffer;
use henad_core::export::StatColumns;
use henad_core::export::csv::fmt_f64;
use henad_core::metadata::TimeScale;
use henad_core::params::{ParamDescriptor, ParamValue};

/// Headers of the columns before the stats.
pub const SERIES_ID_COLUMNS: [&str; 2] = ["run_id", "tick"];

/// Header of the column after [`SERIES_ID_COLUMNS`] that a model with a time scale writes.
pub const SERIES_TIME_COLUMN: &str = "time";

/// Writer of `series.csv`.
#[derive(Debug)]
pub struct SeriesWriter<W: Write> {
    dest: W,
    /// Position of the tick-length parameter in a config's values, `None` for a writer with no `time` column.
    tick_length: Option<usize>,
}

/// Returns the position of the tick-length parameter in `params` for a model with the time scale `time`, or `None`
/// for a `series.csv` with no `time` column.
///
/// Every writer and the header check of a resume take the position from this function.
pub(crate) fn tick_length_position(time: Option<&TimeScale>, params: &[ParamDescriptor]) -> Option<usize> {
    time?.tick_length_index(params)
}

/// Returns the header line of `series.csv` for the stat layout `columns`, with its line ending.
///
/// `timed` adds the `time` column after the ids.
pub(crate) fn header_line(columns: &StatColumns, timed: bool) -> String {
    let mut line = SERIES_ID_COLUMNS.join(",");
    if timed {
        line.push(',');
        line.push_str(SERIES_TIME_COLUMN);
    }
    for column in 0..columns.len() {
        line.push(',');
        line.push_str(columns.header(column));
    }
    line.push('\n');
    line
}

impl<W: Write> SeriesWriter<W> {
    /// Writes the header for the stat layout `columns`.
    ///
    /// `tick_length` is the position of the tick-length parameter in a config's values, for a model with a time
    /// scale, and adds the `time` column. [`TimeScale::tick_length_index`] returns it.
    ///
    /// # Errors
    ///
    /// Returns the error of the write.
    pub fn new(mut dest: W, columns: &StatColumns, tick_length: Option<usize>) -> io::Result<Self> {
        dest.write_all(header_line(columns, tick_length.is_some()).as_bytes())?;
        Ok(Self { dest, tick_length })
    }

    /// Returns a writer that adds rows to `dest`, a `series.csv` whose header is already written.
    ///
    /// `tick_length` is as for [`Self::new`], and has to agree with the header.
    pub fn appending(dest: W, tick_length: Option<usize>) -> Self {
        Self { dest, tick_length }
    }

    /// Writes every row of `series`, the series of run `run_id`, whose parameter values are `params`.
    ///
    /// A writer with a `time` column reads the run's tick length from `params`, and writes an empty cell when
    /// `params` holds no `f32` at its position.
    ///
    /// # Errors
    ///
    /// Returns the error of the write.
    pub fn write_run(&mut self, run_id: u64, series: &SeriesBuffer, params: &[ParamValue]) -> io::Result<()> {
        let tick_length = self.tick_length.map(|position| match params.get(position) {
            Some(ParamValue::F32(length)) => *length,
            _ => f32::NAN,
        });
        for (tick, values) in series.rows() {
            write!(self.dest, "{run_id},{tick}")?;
            if let Some(tick_length) = tick_length {
                write!(self.dest, ",{}", fmt_f64(TimeScale::time_at(tick, tick_length)))?;
            }
            for &value in values {
                write!(self.dest, ",{}", fmt_f64(value))?;
            }
            writeln!(self.dest)?;
        }
        Ok(())
    }

    /// Flushes the rows written so far.
    ///
    /// # Errors
    ///
    /// Returns the error of the flush.
    pub fn flush(&mut self) -> io::Result<()> {
        self.dest.flush()
    }

    /// Flushes the writer and returns it.
    ///
    /// # Errors
    ///
    /// Returns the error of the flush.
    pub fn into_inner(mut self) -> io::Result<W> {
        self.dest.flush()?;
        Ok(self.dest)
    }
}

#[cfg(test)]
mod tests {
    use henad_core::explore::measure::SeriesBuffer;
    use henad_core::export::{StatColumns, StatsWriter};
    use henad_core::helpers::{f32_param, stat, stat_vec2, u32_param};
    use henad_core::metadata::{TimeScale, TimeUnit};
    use henad_core::params::ParamValue;

    use super::{SeriesWriter, header_line, tick_length_position};

    const COLOR: [u8; 4] = [0, 0, 0, 255];

    #[test]
    fn the_stat_columns_match_the_stats_export() {
        let stats = [stat("Infected", 3.0, COLOR), stat_vec2("Velocity", 1.0, 0.5, COLOR)];
        let mut exported = Vec::new();
        let mut stats_writer = StatsWriter::new(&mut exported);
        stats_writer.push(0, &stats).expect("a vector takes every write");
        stats_writer.finish().expect("a vector flushes");
        let exported = String::from_utf8(exported).expect("the rows are UTF-8");

        let columns = StatColumns::plan(&stats);
        let mut series = SeriesBuffer::new(columns.len());
        let mut row = Vec::new();
        columns.extract(0, &stats, &mut row).expect("the layout holds");
        series.push(0, &row);
        row[0] = f64::NAN;
        series.push(5, &row);
        let mut writer = SeriesWriter::new(Vec::new(), &columns, None).expect("a vector takes every write");
        writer.write_run(4, &series, &[]).expect("a vector takes every write");
        let written = String::from_utf8(writer.into_inner().expect("a vector flushes")).expect("the rows are UTF-8");

        let exported: Vec<&str> = exported.lines().collect();
        let written: Vec<&str> = written.lines().collect();
        assert_eq!(written[0], format!("run_id,{}", exported[0]));
        assert_eq!(written[1], format!("4,{}", exported[1]));
        assert!(
            written[2].starts_with("4,5,,"),
            "a value that is not finite is an empty cell"
        );
        assert_eq!(written.len(), 3);
    }

    /// A model with a time scale writes the time column as `--export-stats` does, from each run's own tick length.
    #[test]
    fn the_time_column_matches_the_stats_export_run_by_run() {
        let stats = [stat("Infected", 3.0, COLOR)];
        let columns = StatColumns::plan(&stats);
        let mut series = SeriesBuffer::new(columns.len());
        let mut row = Vec::new();
        for tick in [0, 3, 6] {
            columns.extract(tick, &stats, &mut row).expect("the layout holds");
            series.push(tick, &row);
        }
        let mut writer = SeriesWriter::new(Vec::new(), &columns, Some(1)).expect("a vector takes every write");
        for (run_id, length) in [(0, 0.1), (1, 0.25)] {
            let params = [ParamValue::U32(64), ParamValue::F32(length)];
            writer
                .write_run(run_id, &series, &params)
                .expect("a vector takes every write");

            let mut exported = Vec::new();
            let mut stats_writer = StatsWriter::new(&mut exported).tick_length(length);
            for tick in [0, 3, 6] {
                stats_writer.push(tick, &stats).expect("a vector takes every write");
            }
            stats_writer.finish().expect("a vector flushes");
            let exported = String::from_utf8(exported).expect("the rows are UTF-8");
            let rows: Vec<String> = exported
                .lines()
                .skip(1)
                .map(|line| format!("{run_id},{line}"))
                .collect();
            let written = String::from_utf8(writer.dest.clone()).expect("the rows are UTF-8");
            for row in rows {
                assert!(
                    written.lines().any(|line| line == row),
                    "{row} is missing from\n{written}"
                );
            }
        }
        let written = String::from_utf8(writer.into_inner().expect("a vector flushes")).expect("the rows are UTF-8");
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines[0], "run_id,tick,time,Infected");
        assert_eq!(&lines[1..4], ["0,0,0,3", "0,3,0.3,3", "0,6,0.6,3"]);
        assert_eq!(&lines[4..], ["1,0,0,3", "1,3,0.75,3", "1,6,1.5,3"]);
    }

    /// A time scale naming no `f32` parameter adds no `time` column, to the header a resume checks and to the stats
    /// export alike.
    #[test]
    fn a_time_scale_without_an_f32_tick_length_adds_no_time_column() {
        let params = [
            u32_param("grid_width", "Grid width", 64, 1, 4096),
            f32_param("tick_length", "One tick is (days)", 1.0, 0.01, 30.0, None).hidden(),
        ];
        let values = [ParamValue::U32(64), ParamValue::F32(0.5)];
        let columns = StatColumns::plan(&[stat("Infected", 3.0, COLOR)]);
        let timed = TimeScale::new(TimeUnit::Day, "tick_length");
        assert_eq!(tick_length_position(Some(&timed), &params), Some(1));
        assert_eq!(tick_length_position(None, &params), None);
        for id in ["grid_width", "absent"] {
            let scale = TimeScale::new(TimeUnit::Day, id);
            let position = tick_length_position(Some(&scale), &params);
            assert_eq!(position, None, "{id}");
            assert_eq!(scale.tick_length_in(&params, &values), None, "{id}");
            let writer = SeriesWriter::new(Vec::new(), &columns, position).expect("a vector takes every write");
            let written = writer.into_inner().expect("a vector flushes");
            assert_eq!(String::from_utf8(written).expect("UTF-8"), header_line(&columns, false));
        }
    }
}
