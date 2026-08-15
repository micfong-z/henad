//! Time-series stat output. One row per sampled tick, one column per stat series.
//!
//! `StatsHistory` forgets its oldest entries once full, which is exactly wrong for a run whose
//! output is a data file, so this writes straight through to disk instead. Memory stays flat
//! regardless of run length.
//!
//! The column layout is fixed from the *first* sample and reused for every later row, so the
//! header and every row always agree. A model whose `stats()` shape changes mid-run is a
//! programming error, and [`StatsWriter::push`] reports it as one.

use std::io::Write;

use anyhow::{Result, bail};

use henad_core::view::{StatEntry, StatValue};

/// Separator between a series label and a component suffix, e.g. `Average Velocity.x`.
const SUFFIX_SEP: char = '.';

/// One output column, and the part of the stat series feeding it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Column {
    /// Header text, already escaped for CSV.
    header: String,
    /// Index into the `Vec<StatEntry>` returned by `stats()`.
    series: usize,
    part: Part,
}

/// The scalar pulled out of a [`StatValue`] for one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Scalar,
    VecX,
    VecY,
    /// Magnitude of a `Vector2D`, so a vector series is still usable without recombining lanes.
    VecMagnitude,
    /// Count in one histogram bucket.
    Bucket(usize),
    /// Total across all histogram buckets.
    BucketTotal,
}

/// Streams a stat time series to a writer as CSV.
///
/// Construct, [`push`](Self::push) once per sampled tick, then [`finish`](Self::finish). The
/// header is written on the first `push`, since the column set comes from the sample shape rather
/// than being declared up front. `StatDescriptor` carries a label and colour but not whether the
/// value is a scalar, a vector, or a histogram.
pub struct StatsWriter<W: Write> {
    out: W,
    /// `None` until the first `push` fixes the layout.
    columns: Option<Vec<Column>>,
    rows: u64,
}

impl<W: Write> StatsWriter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            columns: None,
            rows: 0,
        }
    }

    /// Record one sample. The first call fixes the column layout and emits the header.
    ///
    /// # Errors
    /// If writing fails, or if `stats` does not match the layout fixed by the first sample.
    pub fn push(&mut self, tick: u64, stats: &[StatEntry]) -> Result<()> {
        if self.columns.is_none() {
            let columns = plan_columns(stats);
            write!(self.out, "tick")?;
            for column in &columns {
                write!(self.out, ",{}", column.header)?;
            }
            writeln!(self.out)?;
            self.columns = Some(columns);
        }
        let columns = self.columns.as_deref().unwrap_or_default();

        write!(self.out, "{tick}")?;
        for column in columns {
            let Some(entry) = stats.get(column.series) else {
                bail!(
                    "stat series count changed mid-run: column '{}' needs series {} but tick {tick} has {}",
                    column.header,
                    column.series,
                    stats.len()
                );
            };
            let value = extract(&entry.value, column.part).ok_or_else(|| {
                anyhow::anyhow!(
                    "stat series '{}' changed shape mid-run at tick {tick}: column '{}' no longer applies",
                    entry.label,
                    column.header
                )
            })?;
            write!(self.out, ",{}", fmt_f64(value))?;
        }
        writeln!(self.out)?;
        self.rows += 1;
        Ok(())
    }

    /// Flush the underlying writer.
    ///
    /// A `BufWriter` dropped without flushing swallows write errors silently, and a truncated data
    /// file that reports success is worse than a loud failure.
    ///
    /// # Errors
    /// If the final flush fails.
    pub fn finish(mut self) -> Result<u64> {
        self.out.flush()?;
        Ok(self.rows)
    }
}

/// Derive the column layout from one sample.
fn plan_columns(stats: &[StatEntry]) -> Vec<Column> {
    let mut columns = Vec::new();
    for (series, entry) in stats.iter().enumerate() {
        let mut push = |suffix: Option<&str>, part: Part| {
            let name = match suffix {
                Some(suffix) => format!("{}{SUFFIX_SEP}{suffix}", entry.label),
                None => entry.label.to_owned(),
            };
            columns.push(Column {
                header: escape_csv(&name),
                series,
                part,
            });
        };
        match &entry.value {
            StatValue::Scalar(_) => push(None, Part::Scalar),
            StatValue::Vector2D { .. } => {
                push(Some("x"), Part::VecX);
                push(Some("y"), Part::VecY);
                push(Some("magnitude"), Part::VecMagnitude);
            }
            StatValue::Histogram { edges, counts } => {
                // Label each bucket by its own range so the columns stay meaningful without the
                // reader needing the edge list. `edges` is bucket boundaries, so a bucket has a
                // lower and upper edge. Fall back to the index if the edges don't line up.
                for bucket in 0..counts.len() {
                    let range = match (edges.get(bucket), edges.get(bucket + 1)) {
                        (Some(lo), Some(hi)) => format!("[{}, {})", fmt_f64(*lo), fmt_f64(*hi)),
                        _ => format!("bucket {bucket}"),
                    };
                    push(Some(&range), Part::Bucket(bucket));
                }
                push(Some("total"), Part::BucketTotal);
            }
        }
    }
    columns
}

/// Pull one column's scalar out of a stat value. `None` if the value no longer has that part,
/// which means the series changed shape since the layout was fixed.
fn extract(value: &StatValue, part: Part) -> Option<f64> {
    match (value, part) {
        (StatValue::Scalar(v), Part::Scalar) => Some(*v),
        (StatValue::Vector2D { x, .. }, Part::VecX) => Some(*x),
        (StatValue::Vector2D { y, .. }, Part::VecY) => Some(*y),
        (StatValue::Vector2D { x, y }, Part::VecMagnitude) => Some(x.hypot(*y)),
        (StatValue::Histogram { counts, .. }, Part::Bucket(bucket)) => counts.get(bucket).map(|c| *c as f64),
        (StatValue::Histogram { counts, .. }, Part::BucketTotal) => Some(counts.iter().sum::<u64>() as f64),
        _ => None,
    }
}

/// Integral values lose the trailing `.0`, everything else keeps full round-trip precision.
/// Non-finite values become empty cells, since `NaN` and `inf` are not valid numbers to most
/// readers and an empty cell is the conventional missing marker.
fn fmt_f64(value: f64) -> String {
    if !value.is_finite() {
        String::new()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// Quote a CSV field if it contains a comma, quote, or newline, doubling any inner quotes.
/// Stat labels are `&'static str` from model source, so this is belt-and-braces. A label with a
/// comma in it would otherwise silently shift every column right of it.
fn escape_csv(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: [u8; 4] = [0, 0, 0, 255];

    fn scalar(label: &'static str, v: f64) -> StatEntry {
        StatEntry {
            label,
            value: StatValue::Scalar(v),
            color: C,
        }
    }

    fn vec2(label: &'static str, x: f64, y: f64) -> StatEntry {
        StatEntry {
            label,
            value: StatValue::Vector2D { x, y },
            color: C,
        }
    }

    fn hist(label: &'static str, edges: Vec<f64>, counts: Vec<u64>) -> StatEntry {
        StatEntry {
            label,
            value: StatValue::Histogram { edges, counts },
            color: C,
        }
    }

    /// Run samples through a writer and return the CSV text.
    fn render(samples: &[(u64, Vec<StatEntry>)]) -> String {
        let mut buf = Vec::new();
        let mut writer = StatsWriter::new(&mut buf);
        for (tick, stats) in samples {
            writer.push(*tick, stats).expect("push should succeed");
        }
        writer.finish().expect("finish should succeed");
        String::from_utf8(buf).expect("output should be utf8")
    }

    #[test]
    fn scalars_write_one_column_each() {
        let csv = render(&[
            (0, vec![scalar("Alive", 10.0), scalar("Dead", 5.0)]),
            (1, vec![scalar("Alive", 12.0), scalar("Dead", 3.0)]),
        ]);
        assert_eq!(csv, "tick,Alive,Dead\n0,10,5\n1,12,3\n");
    }

    #[test]
    fn vectors_split_into_x_y_and_magnitude() {
        let csv = render(&[(7, vec![vec2("Velocity", 3.0, 4.0)])]);
        assert_eq!(csv, "tick,Velocity.x,Velocity.y,Velocity.magnitude\n7,3,4,5\n");
    }

    #[test]
    fn histograms_label_buckets_by_range_and_add_a_total() {
        let csv = render(&[(0, vec![hist("Speed", vec![0.0, 1.0, 2.0], vec![4, 6])])]);
        assert_eq!(csv, "tick,\"Speed.[0, 1)\",\"Speed.[1, 2)\",Speed.total\n0,4,6,10\n");
    }

    /// A histogram with fewer edges than buckets still produces a column per bucket.
    #[test]
    fn histogram_falls_back_to_bucket_index_without_edges() {
        let csv = render(&[(0, vec![hist("H", vec![], vec![1, 2])])]);
        assert_eq!(csv, "tick,H.bucket 0,H.bucket 1,H.total\n0,1,2,3\n");
    }

    #[test]
    fn header_is_written_once_for_many_rows() {
        let csv = render(&[
            (0, vec![scalar("A", 1.0)]),
            (1, vec![scalar("A", 2.0)]),
            (2, vec![scalar("A", 3.0)]),
        ]);
        assert_eq!(csv.lines().filter(|l| l.starts_with("tick")).count(), 1);
        assert_eq!(csv.lines().count(), 4);
    }

    #[test]
    fn no_samples_writes_nothing() {
        let csv = render(&[]);
        assert!(csv.is_empty(), "expected empty output, got {csv:?}");
    }

    #[test]
    fn fractional_values_keep_precision() {
        let csv = render(&[(0, vec![scalar("A", 0.1 + 0.2)])]);
        // Round-trip precision, not a truncated 0.3.
        assert!(csv.contains("0.30000000000000004"), "got {csv}");
    }

    #[test]
    fn non_finite_values_become_empty_cells() {
        let csv = render(&[(0, vec![scalar("A", f64::NAN), scalar("B", f64::INFINITY)])]);
        assert_eq!(csv, "tick,A,B\n0,,\n");
    }

    #[test]
    fn labels_containing_commas_are_quoted() {
        let csv = render(&[(0, vec![scalar("Susceptible, count", 1.0), scalar("B", 2.0)])]);
        assert_eq!(csv, "tick,\"Susceptible, count\",B\n0,1,2\n");
        // Three fields per row, so the comma did not shift the columns.
        assert_eq!(csv.lines().count(), 2);
    }

    #[test]
    fn finish_returns_the_row_count_excluding_the_header() {
        let mut buf = Vec::new();
        let mut writer = StatsWriter::new(&mut buf);
        writer.push(0, &[scalar("A", 1.0)]).expect("push");
        writer.push(1, &[scalar("A", 2.0)]).expect("push");
        assert_eq!(writer.finish().expect("finish"), 2);
    }

    #[test]
    fn a_series_disappearing_mid_run_is_an_error() {
        let mut buf = Vec::new();
        let mut writer = StatsWriter::new(&mut buf);
        writer.push(0, &[scalar("A", 1.0), scalar("B", 2.0)]).expect("push");
        let err = writer.push(1, &[scalar("A", 1.0)]).expect_err("should reject");
        assert!(err.to_string().contains("changed mid-run"), "got {err}");
    }

    #[test]
    fn a_series_changing_kind_mid_run_is_an_error() {
        let mut buf = Vec::new();
        let mut writer = StatsWriter::new(&mut buf);
        writer.push(0, &[scalar("A", 1.0)]).expect("push");
        let err = writer.push(1, &[vec2("A", 1.0, 2.0)]).expect_err("should reject");
        assert!(err.to_string().contains("changed shape"), "got {err}");
    }

    #[test]
    fn a_histogram_losing_buckets_mid_run_is_an_error() {
        let mut buf = Vec::new();
        let mut writer = StatsWriter::new(&mut buf);
        writer
            .push(0, &[hist("H", vec![0.0, 1.0, 2.0], vec![1, 2])])
            .expect("push");
        let err = writer
            .push(1, &[hist("H", vec![0.0, 1.0], vec![1])])
            .expect_err("should reject");
        assert!(err.to_string().contains("changed shape"), "got {err}");
    }

    #[test]
    fn mixed_kinds_keep_series_order() {
        let csv = render(&[(
            0,
            vec![
                scalar("S", 1.0),
                vec2("V", 0.0, 2.0),
                hist("H", vec![0.0, 1.0], vec![3]),
            ],
        )]);
        assert_eq!(csv, "tick,S,V.x,V.y,V.magnitude,\"H.[0, 1)\",H.total\n0,1,0,2,2,3,3\n");
    }

    #[test]
    fn a_model_with_no_stats_still_writes_ticks() {
        let csv = render(&[(0, vec![]), (5, vec![])]);
        assert_eq!(csv, "tick\n0\n5\n");
    }
}
