//! Sheet files: a sheet read, checked and lowered to the graph it runs as.

use std::fmt;

use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::ModelCode;

use super::check::{SheetReport, span_of};
use super::document::SheetDocument;
use super::lower::{self, Problem};
use super::{LineMap, ParsedSheet, SheetErrors, SheetFileError, SheetPlaces};
use crate::GraphFile;

/// A sheet file: the sheet, its text and places, its warnings, and the graph it lowers to with the table of that
/// graph's nodes.
///
/// The code identity of a sheet is the lowered graph's, [`GraphFile::code`]. The sheet's name, ids, state names,
/// parameter labels and colours enter it through the graph. Notes, comments, `purpose` and the names of results stay
/// outside it.
///
/// ```
/// use henad_graph::sheet::{SheetFile, starters};
///
/// let file = SheetFile::parse(starters()[0].text).expect("the starter lowers");
/// assert_eq!(file.lowered().document().name, "Game of Life");
/// assert_eq!(file.code(), file.lowered().code());
/// ```
#[derive(Clone)]
pub struct SheetFile {
    /// The sheet as read.
    parsed: ParsedSheet,
    /// Canonical text of the sheet.
    canonical: String,
    /// Warnings of the sheet's check, each placed in its text.
    warnings: Vec<CodeDiagnostic>,
    /// The graph the sheet lowers to.
    lowered: GraphFile,
    /// Owner and slot path of every node of [`Self::lowered`].
    line_map: LineMap,
}

/// Prints the sheet's id, the number of warnings and the lowered graph's nodes.
impl fmt::Debug for SheetFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SheetFile")
            .field("id", &self.parsed.document().sheet.id)
            .field("warnings", &self.warnings.len())
            .field("lowered_nodes", &self.line_map.len())
            .finish_non_exhaustive()
    }
}

impl SheetFile {
    /// Reads, checks and lowers the sheet held in `text`.
    ///
    /// A line with a skip warning, and every line that reads it, is left out of the lowered graph, and its warning is
    /// among [`Self::warnings`].
    ///
    /// # Errors
    ///
    /// Returns the errors of [`ParsedSheet::parse`], and [`SheetFileError::Invalid`] holding the errors of the sheet's
    /// check, or when it has none the problems of its lowering: a world of agents and the line kinds this version does
    /// not lower, each in the author's words, two draws with one key, a sheet past the node limits, and a lowered graph
    /// that does not read. Each is placed at the table it is about.
    pub fn parse(text: &str) -> Result<Self, SheetFileError> {
        let parsed = ParsedSheet::parse(text)?;
        let report = parsed.check();
        if report.has_errors() {
            return Err(invalid(report.errors().map(|error| error.diagnostic.clone())));
        }
        let canonical = parsed
            .document()
            .to_text()
            .map_err(|error| SheetFileError::Format(Box::new(CodeDiagnostic::error(error.to_string()))))?;
        let lowered =
            lower::lower(parsed.document(), &report).map_err(|problems| invalid(placed(&parsed, problems)))?;
        let graph = GraphFile::from_document(&lowered.document, None).map_err(|error| {
            invalid([CodeDiagnostic::error(format!(
                "the sheet lowers to a graph that does not read: {}",
                error.diagnostic().message
            ))])
        })?;
        Ok(Self {
            warnings: warnings(&report),
            parsed,
            canonical,
            lowered: graph,
            line_map: lowered.map,
        })
    }

    /// Document the sheet holds.
    pub fn document(&self) -> &SheetDocument {
        self.parsed.document()
    }

    /// Text the sheet was read from.
    pub fn text(&self) -> &str {
        self.parsed.text()
    }

    /// Places of the tables and keys of the sheet.
    pub fn places(&self) -> &SheetPlaces {
        self.parsed.places()
    }

    /// The sheet as read: its document, text and places.
    pub fn parsed(&self) -> &ParsedSheet {
        &self.parsed
    }

    /// Graph the sheet lowers to.
    pub fn lowered(&self) -> &GraphFile {
        &self.lowered
    }

    /// Owner and slot path of every node of [`Self::lowered`].
    pub fn line_map(&self) -> &LineMap {
        &self.line_map
    }

    /// Warnings of the sheet's check, skip warnings included, each placed at its table.
    pub fn warnings(&self) -> &[CodeDiagnostic] {
        &self.warnings
    }

    /// Code identity of the sheet: the code identity of the lowered graph.
    pub fn code(&self) -> ModelCode {
        self.lowered.code()
    }

    /// Canonical text of the sheet, as [`SheetDocument::to_text`] writes it.
    pub fn to_toml(&self) -> &str {
        &self.canonical
    }
}

/// Returns [`SheetFileError::Invalid`] holding `errors`, or a general error in their place when `errors` is empty.
fn invalid(errors: impl IntoIterator<Item = CodeDiagnostic>) -> SheetFileError {
    let mut errors = errors.into_iter();
    let first = errors
        .next()
        .unwrap_or_else(|| CodeDiagnostic::error("the sheet does not lower, and its lowering names no problem"));
    SheetFileError::Invalid(SheetErrors::new(first, errors))
}

/// Returns the warnings of `report`, already placed.
fn warnings(report: &SheetReport) -> Vec<CodeDiagnostic> {
    report.warnings().map(|warning| warning.diagnostic.clone()).collect()
}

/// Returns `problems` as errors placed in the text of `parsed`.
fn placed(parsed: &ParsedSheet, problems: Vec<Problem>) -> Vec<CodeDiagnostic> {
    problems
        .into_iter()
        .map(|problem| {
            let error = CodeDiagnostic::error(problem.message);
            match span_of(parsed, &problem.place) {
                Some(span) => parsed.place(error, span),
                None => error,
            }
        })
        .collect()
}
