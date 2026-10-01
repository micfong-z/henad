//! Tables of an output directory read back, for a resume or a merge.
//!
//! A process that ends mid-write can leave a partial last record in `runs.csv` or `series.csv`, and series rows of a
//! run whose row in `runs.csv` was never written. The readers here keep complete records only. A complete record
//! ends in a line feed outside quotes.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead as _, BufReader, Read as _, Seek as _, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use henad_core::explore::outcome::RunStatus;
use henad_core::export::csv::{CsvError, parse_records};

use crate::output::runs_csv::ID_COLUMNS;

/// One complete record of a `runs.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run_id: u64,
    pub config_id: u64,
    pub rep: u64,
    pub run_key: u64,
    pub status: RunStatus,
    /// Text of the record as written, its line ending included.
    pub text: String,
}

impl RunRecord {
    /// Returns the text of the record with its run id replaced by `run_id`.
    pub fn renumbered(&self, run_id: u64) -> String {
        let rest = self.text.find(',').map_or("\n", |comma| &self.text[comma..]);
        format!("{run_id}{rest}")
    }
}

/// Complete records of a `runs.csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunsCsv {
    /// Column names before CSV escaping, `None` for a file with no complete header.
    pub header: Option<Vec<String>>,
    /// Records after the header, in file order.
    pub records: Vec<RunRecord>,
    /// Bytes of the file up to the end of its last complete record.
    pub complete_bytes: u64,
    /// Bytes of the file, 0 for a missing file.
    pub file_bytes: u64,
}

impl RunsCsv {
    /// Reads the complete records of the `runs.csv` at `path`. A missing file has no header and no records.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] when the file cannot be read, or a complete record is not a row of `runs.csv`.
    pub fn read(path: &Path) -> Result<Self, ReadError> {
        let io_error = |source| ReadError::Io {
            path: path.to_owned(),
            source,
        };
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(source) => return Err(io_error(source)),
        };
        let ends = record_ends(&bytes);
        let complete_bytes = ends.last().copied().unwrap_or(0);
        // A cut can split a character, so only the complete records need to be text.
        let text = std::str::from_utf8(&bytes[..complete_bytes])
            .map_err(|error| io_error(io::Error::new(io::ErrorKind::InvalidData, error)))?;
        let mut table = Self {
            header: None,
            records: Vec::with_capacity(ends.len().saturating_sub(1)),
            complete_bytes: complete_bytes as u64,
            file_bytes: bytes.len() as u64,
        };
        let Some((&header_end, record_ends)) = ends.split_first() else {
            return Ok(table);
        };
        let header = parse_one(text, 0..header_end, path, 1)?;
        let layout = RecordLayout::find(&header, path)?;
        let mut start = header_end;
        for (index, &end) in record_ends.iter().enumerate() {
            let record_number = index + 2;
            let record_text = &text[start..end];
            let fields = parse_one(text, start..end, path, record_number)?;
            if fields.len() != header.len() {
                return Err(ReadError::FieldCount {
                    path: path.to_owned(),
                    record_number,
                    found: fields.len(),
                    expected: header.len(),
                });
            }
            table
                .records
                .push(layout.record(&header, &fields, record_text, path, record_number)?);
            start = end;
        }
        table.header = Some(header);
        Ok(table)
    }
}

/// Positions of the columns of a `runs.csv` header that identify a run.
struct RecordLayout {
    run_id: usize,
    config_id: usize,
    rep: usize,
    run_key: usize,
    status: usize,
}

impl RecordLayout {
    /// Finds the columns in `header`. The ids come first, and `status` is found from the end, since a parameter can
    /// share its name.
    fn find(header: &[String], path: &Path) -> Result<Self, ReadError> {
        let first = |column: &'static str| {
            header
                .iter()
                .take(ID_COLUMNS.len())
                .position(|name| name == column)
                .ok_or_else(|| ReadError::MissingColumn {
                    path: path.to_owned(),
                    column,
                })
        };
        Ok(Self {
            run_id: first("run_id")?,
            config_id: first("config_id")?,
            rep: first("rep")?,
            run_key: first("run_key")?,
            status: header
                .iter()
                .rposition(|name| name == "status")
                .ok_or_else(|| ReadError::MissingColumn {
                    path: path.to_owned(),
                    column: "status",
                })?,
        })
    }

    fn record(
        &self,
        header: &[String],
        fields: &[String],
        text: &str,
        path: &Path,
        record_number: usize,
    ) -> Result<RunRecord, ReadError> {
        let bad_field = |column: usize| ReadError::BadField {
            path: path.to_owned(),
            record_number,
            column: header[column].clone(),
            text: fields[column].clone(),
        };
        let number = |column: usize| fields[column].parse::<u64>().ok().ok_or_else(|| bad_field(column));
        Ok(RunRecord {
            run_id: number(self.run_id)?,
            config_id: number(self.config_id)?,
            rep: number(self.rep)?,
            run_key: u64::from_str_radix(&fields[self.run_key], 16)
                .ok()
                .ok_or_else(|| bad_field(self.run_key))?,
            status: fields[self.status].parse().ok().ok_or_else(|| bad_field(self.status))?,
            text: text.to_owned(),
        })
    }
}

/// Returns the fields of the one complete record at `record` in `text`, record `record_number` of the file at `path`.
///
/// The line of a [`CsvError`] counts from the start of `text`.
pub(crate) fn parse_one(
    text: &str,
    record: Range<usize>,
    path: &Path,
    record_number: usize,
) -> Result<Vec<String>, ReadError> {
    parse_records(&text[record.clone()])
        .map_err(|error| ReadError::Csv {
            path: path.to_owned(),
            record_number,
            source: shifted(&error, text[..record.start].matches('\n').count()),
        })?
        .into_iter()
        .next()
        .ok_or_else(|| ReadError::FieldCount {
            path: path.to_owned(),
            record_number,
            found: 0,
            expected: 1,
        })
}

/// Returns `error`, found in a text that starts after `lines` line feeds, with its line counted from the start of
/// the whole text.
pub(crate) fn shifted(error: &CsvError, lines: usize) -> CsvError {
    match *error {
        CsvError::UnterminatedQuote { line } => CsvError::UnterminatedQuote { line: line + lines },
        CsvError::MisplacedQuote { line } => CsvError::MisplacedQuote { line: line + lines },
    }
}

/// Returns the end of each complete record of `bytes`, just past its line feed.
pub(crate) fn record_ends(bytes: &[u8]) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut scan = RecordScan::FieldStart;
    for (index, &byte) in bytes.iter().enumerate() {
        let (next, record_ended) = scan.advance(byte);
        scan = next;
        if record_ended {
            ends.push(index + 1);
        }
    }
    ends
}

/// Position of a scan within a CSV record, in the grammar [`parse_records`] reads.
///
/// Note that a quote inside an unquoted field changes nothing. The record holding it ends at its line feed, and
/// [`parse_records`] refuses it.
///
/// A quote that opens a field and is never closed makes the rest of the text one partial record. [`record_ends`]
/// finds no end after it, and a reader of complete records leaves that record out without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordScan {
    /// At the start of a field.
    FieldStart,
    /// Inside a field that does not open with a quote.
    Unquoted,
    /// Inside a quoted field.
    Quoted,
    /// Just past a quote inside a quoted field, which either closes the field or starts a doubled quote.
    QuoteClosed,
}

impl RecordScan {
    /// Returns the position after `byte`, and whether `byte` ends a record.
    pub(crate) fn advance(self, byte: u8) -> (Self, bool) {
        match (self, byte) {
            (Self::Quoted, b'"') => (Self::QuoteClosed, false),
            (Self::QuoteClosed | Self::FieldStart, b'"') | (Self::Quoted, _) => (Self::Quoted, false),
            (_, b',') => (Self::FieldStart, false),
            (_, b'\n') => (Self::FieldStart, true),
            _ => (Self::Unquoted, false),
        }
    }
}

/// Layout of a `series.csv`: its header, and the stretches of complete lines in order of their run ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesScan {
    pub path: PathBuf,
    /// Header line as written with its line ending, `None` for a file with no complete header.
    pub header: Option<String>,
    /// Byte ranges of the longest stretches of complete lines whose run ids never decrease, in file order.
    pub segments: Vec<Range<u64>>,
    /// Bytes of the file up to the end of its last complete line.
    pub complete_bytes: u64,
    /// Bytes of the file, 0 for a missing file.
    pub file_bytes: u64,
    /// Offset of the first complete line the scan's rule left out.
    pub first_dropped_offset: Option<u64>,
    /// Whether a line the rule kept comes after one it left out.
    pub kept_after_dropped: bool,
}

impl SeriesScan {
    /// Scans the `series.csv` at `path`, marking each complete line `keep` refuses by its run id as left out. A
    /// missing file has no header and no lines.
    ///
    /// # Errors
    ///
    /// Returns [`ReadError`] when the file cannot be read, or a complete line does not start with a run id.
    pub fn read(path: &Path, keep: impl Fn(u64) -> bool) -> Result<Self, ReadError> {
        let io_error = |source| ReadError::Io {
            path: path.to_owned(),
            source,
        };
        let mut scan = Self {
            path: path.to_owned(),
            header: None,
            segments: Vec::new(),
            complete_bytes: 0,
            file_bytes: 0,
            first_dropped_offset: None,
            kept_after_dropped: false,
        };
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(scan),
            Err(source) => return Err(io_error(source)),
        };
        scan.file_bytes = file.metadata().map_err(io_error)?.len();
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line).map_err(io_error)? == 0 || !line.ends_with(b"\n") {
            return Ok(scan);
        }
        scan.header = Some(String::from_utf8_lossy(&line).into_owned());
        let mut offset = line.len() as u64;
        scan.complete_bytes = offset;
        let mut previous_id = None;
        let mut record_number = 1;
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line).map_err(io_error)?;
            if read == 0 || !line.ends_with(b"\n") {
                break;
            }
            record_number += 1;
            let run_id = line_run_id(&line).ok_or_else(|| ReadError::BadField {
                path: path.to_owned(),
                record_number,
                column: "run_id".to_owned(),
                text: String::from_utf8_lossy(&line).trim_end().to_owned(),
            })?;
            let end = offset + read as u64;
            match (previous_id, scan.segments.last_mut()) {
                (Some(previous), Some(segment)) if run_id >= previous => segment.end = end,
                _ => scan.segments.push(offset..end),
            }
            if keep(run_id) {
                scan.kept_after_dropped |= scan.first_dropped_offset.is_some();
            } else if scan.first_dropped_offset.is_none() {
                scan.first_dropped_offset = Some(offset);
            }
            previous_id = Some(run_id);
            offset = end;
        }
        scan.complete_bytes = offset;
        Ok(scan)
    }
}

/// Returns the run id at the start of a series line, or `None` when it has none.
fn line_run_id(line: &[u8]) -> Option<u64> {
    let comma = line.iter().position(|&byte| byte == b',')?;
    std::str::from_utf8(&line[..comma]).ok()?.parse().ok()
}

/// Byte range of a series file to merge, and the index of its input directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesSegment {
    pub path: PathBuf,
    pub range: Range<u64>,
    /// Index of the input directory the segment comes from, handed to the renumbering rule.
    pub input_index: usize,
}

/// Writes `header`, then the lines of `segments` merged in order of the run ids they are written with, to `dest`.
///
/// `renumber` maps a line's input index and run id to the run id it is written with, or to `None` to leave the line
/// out. The lines of each segment must come in order of their new run ids. Lines of one run keep their order, and two
/// segments giving one run id are taken in segment order.
///
/// # Errors
///
/// Returns the error of a read or a write, and [`io::ErrorKind::InvalidData`] for a line with no run id.
pub fn merge_series(
    dest: &mut dyn Write,
    header: &str,
    segments: &[SeriesSegment],
    renumber: impl Fn(usize, u64) -> Option<u64>,
) -> io::Result<()> {
    dest.write_all(header.as_bytes())?;
    let mut readers = segments
        .iter()
        .map(SegmentReader::open)
        .collect::<io::Result<Vec<_>>>()?;
    let mut queue = BinaryHeap::with_capacity(readers.len());
    for (index, reader) in readers.iter_mut().enumerate() {
        if let Some(run_id) = reader.advance(&renumber)? {
            queue.push(Reverse((run_id, index)));
        }
    }
    while let Some(Reverse((run_id, index))) = queue.pop() {
        let reader = &mut readers[index];
        write!(dest, "{run_id}")?;
        dest.write_all(&reader.rest)?;
        if let Some(next_id) = reader.advance(&renumber)? {
            queue.push(Reverse((next_id, index)));
        }
    }
    Ok(())
}

/// Reader of the lines of one segment.
struct SegmentReader {
    lines: io::Take<BufReader<File>>,
    /// Index of the input directory the segment comes from.
    input_index: usize,
    /// Line being read.
    line: Vec<u8>,
    /// Text of the current line after its run id, from its first comma to its line ending.
    rest: Vec<u8>,
}

impl SegmentReader {
    fn open(segment: &SeriesSegment) -> io::Result<Self> {
        let mut file = File::open(&segment.path)?;
        file.seek(SeekFrom::Start(segment.range.start))?;
        Ok(Self {
            lines: BufReader::new(file).take(segment.range.end - segment.range.start),
            input_index: segment.input_index,
            line: Vec::new(),
            rest: Vec::new(),
        })
    }

    /// Reads up to the next line `renumber` keeps, and returns its new run id, or `None` at the end of the segment.
    fn advance(&mut self, renumber: &impl Fn(usize, u64) -> Option<u64>) -> io::Result<Option<u64>> {
        loop {
            self.line.clear();
            if self.lines.read_until(b'\n', &mut self.line)? == 0 {
                return Ok(None);
            }
            let comma = self.line.iter().position(|&byte| byte == b',');
            let run_id = line_run_id(&self.line)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a series line has no run id"))?;
            if let (Some(new_id), Some(comma)) = (renumber(self.input_index, run_id), comma) {
                self.rest.clear();
                self.rest.extend_from_slice(&self.line[comma..]);
                return Ok(Some(new_id));
            }
        }
    }
}

/// A table that cannot be read back.
#[derive(Debug)]
pub enum ReadError {
    /// Reading `path` failed.
    Io { path: PathBuf, source: io::Error },
    /// Record `record_number` of `path`, counted from 1 for the header, is not valid CSV.
    Csv {
        path: PathBuf,
        record_number: usize,
        source: CsvError,
    },
    /// A header of `path` without the column `column`.
    MissingColumn { path: PathBuf, column: &'static str },
    /// Record `record_number` of `path` with a different number of fields from its header.
    FieldCount {
        path: PathBuf,
        record_number: usize,
        /// Fields in the record.
        found: usize,
        /// Fields in the header.
        expected: usize,
    },
    /// Field `column` of record `record_number` of `path`, holding `text` the column cannot take.
    BadField {
        path: PathBuf,
        record_number: usize,
        column: String,
        text: String,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, .. } => write!(f, "cannot read '{}'", path.display()),
            Self::Csv {
                path, record_number, ..
            } => write!(f, "record {record_number} of '{}' is not valid CSV", path.display()),
            Self::MissingColumn { path, column } => write!(f, "'{}' has no '{column}' column", path.display()),
            Self::FieldCount {
                path,
                record_number,
                found,
                expected,
            } => write!(
                f,
                "record {record_number} of '{}' has {found} fields, expected {expected}",
                path.display()
            ),
            Self::BadField {
                path,
                record_number,
                column,
                text,
            } => write!(
                f,
                "record {record_number} of '{}' has invalid value '{text}' in column '{column}'",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Csv { source, .. } => Some(source),
            Self::MissingColumn { .. } | Self::FieldCount { .. } | Self::BadField { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use henad_core::explore::outcome::RunStatus;
    use henad_core::export::csv::CsvError;

    use super::{ReadError, RunsCsv, SeriesScan, SeriesSegment, merge_series, record_ends};
    use crate::tests::support::ScratchDir;

    const RUNS: &str = "\
run_id,config_id,block,rep,seed,run_key,rate,status,note
0,0,0,0,11,00000000000000aa,0.1,ok,
1,0,0,1,12,00000000000000ab,0.1,panicked,\"while stepping,
 it panicked\"
2,1,0,0,11,00000000000000ba,0.2,ok,
3,1,0,1,12,00000000000000bb,0.2,timed_";

    /// Returns the length of `text` up to the end of its last line feed.
    fn table_end(text: &str) -> usize {
        text.rfind('\n').map_or(0, |index| index + 1)
    }

    #[test]
    fn a_partial_last_record_is_left_out() {
        let scratch = ScratchDir::new("partial-runs");
        fs::create_dir_all(scratch.path()).expect("a scratch directory");
        let path = scratch.path().join("runs.csv");
        fs::write(&path, RUNS).expect("the table writes");
        let table = RunsCsv::read(&path).expect("a valid runs.csv");
        let ids: Vec<(u64, u64, u64, RunStatus)> = table
            .records
            .iter()
            .map(|record| (record.run_id, record.config_id, record.rep, record.status))
            .collect();
        assert_eq!(
            ids,
            [
                (0, 0, 0, RunStatus::Ok),
                (1, 0, 1, RunStatus::Panicked),
                (2, 1, 0, RunStatus::Ok)
            ]
        );
        assert_eq!(table.records[2].run_key, 0xba);
        assert_eq!(table.file_bytes, RUNS.len() as u64);
        assert_eq!(
            &RUNS[table.complete_bytes as usize..],
            "3,1,0,1,12,00000000000000bb,0.2,timed_"
        );
        assert_eq!(
            table.records[1].renumbered(7),
            "7,0,0,1,12,00000000000000ab,0.1,panicked,\"while stepping,\n it panicked\"\n",
            "a quoted line feed stays inside its record"
        );

        let cut_in_quotes = &RUNS[..RUNS.find("\n it").expect("the quoted line feed") + 1];
        fs::write(&path, cut_in_quotes).expect("the table writes");
        let table = RunsCsv::read(&path).expect("a valid runs.csv");
        assert_eq!(table.records.len(), 1, "a line feed inside quotes ends no record");

        let mut split_character = RUNS.as_bytes()[..table_end(RUNS)].to_vec();
        split_character.extend_from_slice(&"4,2,0,0,11,00000000000000ca,0.3,ok,\u{e9}".as_bytes()[..36]);
        fs::write(&path, &split_character).expect("the table writes");
        let table = RunsCsv::read(&path).expect("a character cut in two is in the partial record");
        assert_eq!(table.records.len(), 3);

        let missing = RunsCsv::read(&scratch.path().join("absent.csv")).expect("a missing table reads as empty");
        assert_eq!(
            (missing.header, missing.records.len(), missing.file_bytes),
            (None, 0, 0)
        );
    }

    #[test]
    fn a_stray_quote_is_refused_instead_of_hiding_the_records_after_it() {
        let scratch = ScratchDir::new("stray-quote");
        fs::create_dir_all(scratch.path()).expect("a scratch directory");
        let path = scratch.path().join("runs.csv");
        let stray = RUNS.replace("00000000000000ba,0.2,ok", "00000000000000ba,0.2\",ok");
        fs::write(&path, &stray).expect("the table writes");
        let error = RunsCsv::read(&path).expect_err("a quote inside an unquoted field");
        assert!(
            matches!(
                error,
                ReadError::Csv {
                    record_number: 4,
                    source: CsvError::MisplacedQuote { line: 5 },
                    ..
                }
            ),
            "the line counts the quoted line feed before it: {error:?}"
        );

        let text = "a,b\n\"say \"\"hi\"\"\",\"x\ny\"\n1,2";
        assert_eq!(
            record_ends(text.as_bytes()),
            [4, 23],
            "doubled and multi-line quotes stay in their field"
        );
    }

    #[test]
    fn series_segments_merge_in_run_order() {
        let scratch = ScratchDir::new("series-merge");
        fs::create_dir_all(scratch.path()).expect("a scratch directory");
        let first = scratch.path().join("first.csv");
        let second = scratch.path().join("second.csv");
        fs::write(&first, "run_id,tick,Cells\n0,0,1\n0,5,2\n4,0,1\n1,0,7\n1,5,8\n9,0,3").expect("the table writes");
        fs::write(&second, "run_id,tick,Cells\n2,0,4\n3,0,5\n").expect("the table writes");

        let scan = SeriesScan::read(&first, |run_id| run_id != 4).expect("a valid series");
        assert_eq!(scan.header.as_deref(), Some("run_id,tick,Cells\n"));
        assert_eq!(scan.segments.len(), 2, "run 1 follows run 4");
        assert!(scan.kept_after_dropped);
        assert_eq!(scan.complete_bytes, scan.file_bytes - "9,0,3".len() as u64);
        let other = SeriesScan::read(&second, |_| true).expect("a valid series");
        assert_eq!((other.first_dropped_offset, other.kept_after_dropped), (None, false));

        let mut segments: Vec<SeriesSegment> = scan
            .segments
            .iter()
            .map(|range| SeriesSegment {
                path: first.clone(),
                range: range.clone(),
                input_index: 0,
            })
            .collect();
        segments.extend(other.segments.iter().map(|range| SeriesSegment {
            path: second.clone(),
            range: range.clone(),
            input_index: 1,
        }));
        let mut merged = Vec::new();
        merge_series(&mut merged, "run_id,tick,Cells\n", &segments, |input_index, run_id| {
            (run_id != 4).then_some(run_id * 10 + input_index as u64)
        })
        .expect("the segments merge");
        assert_eq!(
            String::from_utf8(merged).expect("the lines are UTF-8"),
            "run_id,tick,Cells\n0,0,1\n0,5,2\n10,0,7\n10,5,8\n21,0,4\n31,0,5\n"
        );
    }
}
