//! Files of a sweep or search held in memory, byte for byte the files an output directory would hold.

use std::path::Path;
use std::sync::Arc;

use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::plan::Plan;
use henad_core::params::ParamDescriptor;

use crate::output::manifest::Manifest;
use crate::output::runs_csv::RunsWriter;
use crate::output::series_csv::SeriesWriter;
use crate::output::summary_csv::{SummaryError, write_summary};
use crate::output::{
    MANIFEST_FILE, OutputError, OutputWriter, RUNS_FILE, SERIES_FILE, SUMMARY_FILE, manifest_text, write_error_at,
};

/// The four files of a sweep, and the tables a search adds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepFiles {
    pub runs: Vec<u8>,
    pub series: Vec<u8>,
    pub summary: Vec<u8>,
    pub manifest: Vec<u8>,
    /// Tables a search writes, each with its file name, in the order they are listed. Empty for a sweep.
    pub search_tables: Vec<(&'static str, Vec<u8>)>,
}

impl SweepFiles {
    /// Returns the files of the runs `writer` holds, with `summary.csv` rebuilt from them and `manifest` as
    /// `manifest.json`.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Summary`] when the runs cannot be summarized, [`OutputError::Read`] when they are not
    /// UTF-8, and [`OutputError::Manifest`] when the manifest cannot be serialized.
    pub(crate) fn assemble(writer: OutputWriter<Vec<u8>>, manifest: &Manifest) -> Result<Self, OutputError> {
        let (runs, series) = writer.finish().map_err(write_error_at(Path::new(RUNS_FILE)))?;
        let summary = match write_summary(runs.as_slice(), Vec::new()) {
            Ok(summary) => summary,
            Err(SummaryError::Read(source)) => {
                return Err(OutputError::Read {
                    path: Path::new(RUNS_FILE).to_owned(),
                    source,
                });
            }
            Err(SummaryError::Io(source)) => return Err(write_error_at(Path::new(SUMMARY_FILE))(source)),
            Err(error) => return Err(OutputError::Summary(error)),
        };
        Ok(Self {
            runs,
            series,
            summary,
            manifest: manifest_text(manifest)?.into_bytes(),
            search_tables: Vec::new(),
        })
    }

    /// Returns each file with its name in an output directory, in the order runs, series, summary and manifest, then
    /// the search tables.
    pub fn entries(&self) -> Vec<(&'static str, &[u8])> {
        let mut entries: Vec<(&'static str, &[u8])> = vec![
            (RUNS_FILE, &self.runs),
            (SERIES_FILE, &self.series),
            (SUMMARY_FILE, &self.summary),
            (MANIFEST_FILE, &self.manifest),
        ];
        entries.extend(self.search_tables.iter().map(|(name, bytes)| (*name, bytes.as_slice())));
        entries
    }

    /// Returns each file with its name in an output directory, in the form [`crate::result_set::ResultSet::from_files`]
    /// reads.
    pub fn into_entries(self) -> Vec<(String, Vec<u8>)> {
        let mut entries = vec![
            (RUNS_FILE.to_owned(), self.runs),
            (SERIES_FILE.to_owned(), self.series),
            (SUMMARY_FILE.to_owned(), self.summary),
            (MANIFEST_FILE.to_owned(), self.manifest),
        ];
        entries.extend(
            self.search_tables
                .into_iter()
                .map(|(name, bytes)| (name.to_owned(), bytes)),
        );
        entries
    }
}

/// Returns a writer for the runs of `plan` that holds `runs.csv` and `series.csv` in memory, their headers written.
///
/// `params` are the model's parameters, and `measure` fixes the stat and reducer columns.
///
/// # Errors
///
/// Returns [`OutputError::Write`] when a header cannot be written.
pub(crate) fn memory_writer(
    plan: &Arc<Plan>,
    params: &[ParamDescriptor],
    measure: &MeasurePlan,
) -> Result<OutputWriter<Vec<u8>>, OutputError> {
    let runs = RunsWriter::new(Vec::new(), params, plan.actions(), measure.reducers().names())
        .map_err(write_error_at(Path::new(RUNS_FILE)))?;
    let series = SeriesWriter::new(Vec::new(), measure.columns()).map_err(write_error_at(Path::new(SERIES_FILE)))?;
    Ok(OutputWriter::new(Arc::clone(plan), runs, series))
}
