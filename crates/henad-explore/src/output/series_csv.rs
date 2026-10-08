//! Writer of `series.csv`, the sampled stat rows of every run.
//!
//! The rows of one run are contiguous, and runs come in plan order. The stat columns are those `--export-stats`
//! writes, after a `run_id` column.

use std::io::{self, Write};

use henad_core::explore::measure::SeriesBuffer;
use henad_core::export::StatColumns;
use henad_core::export::csv::fmt_f64;

/// Headers of the columns before the stats.
pub const SERIES_ID_COLUMNS: [&str; 2] = ["run_id", "tick"];

/// Writer of `series.csv`.
#[derive(Debug)]
pub struct SeriesWriter<W: Write> {
    dest: W,
}

/// Returns the header line of `series.csv` for the stat layout `columns`, with its line ending.
pub(crate) fn header_line(columns: &StatColumns) -> String {
    let mut line = SERIES_ID_COLUMNS.join(",");
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
    /// # Errors
    ///
    /// Returns the error of the write.
    pub fn new(mut dest: W, columns: &StatColumns) -> io::Result<Self> {
        dest.write_all(header_line(columns).as_bytes())?;
        Ok(Self { dest })
    }

    /// Returns a writer that adds rows to `dest`, a `series.csv` whose header is already written.
    pub fn appending(dest: W) -> Self {
        Self { dest }
    }

    /// Writes every row of `series`, the series of run `run_id`.
    ///
    /// # Errors
    ///
    /// Returns the error of the write.
    pub fn write_run(&mut self, run_id: u64, series: &SeriesBuffer) -> io::Result<()> {
        for (tick, values) in series.rows() {
            write!(self.dest, "{run_id},{tick}")?;
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

#[cfg(test)]
mod tests {
    use henad_core::explore::measure::SeriesBuffer;
    use henad_core::export::{StatColumns, StatsWriter};
    use henad_core::helpers::{stat, stat_vec2};

    use super::SeriesWriter;

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
        let mut writer = SeriesWriter::new(Vec::new(), &columns).expect("a vector takes every write");
        writer.write_run(4, &series).expect("a vector takes every write");
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
}
