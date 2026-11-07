//! Sheet files: a sheet read, checked and lowered to the graph it runs as.

use std::fmt;

use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::ModelCode;

use super::check::{Place, SheetReport, Thing, span_of};
use super::document::SheetDocument;
use super::lower::{self, Problem};
use super::{LineMap, NodeOrigin, NodeOwner, ParsedSheet, SheetErrors, SheetFileError, SheetPlaces};
use crate::GraphFile;
use crate::document::GraphDocument;
use crate::ids::NodeId;

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
        Self::parse_with(text, |_| {})
    }

    /// Reads, checks and lowers the sheet held in `text` as [`Self::parse`] does, with a node planted in the lowered
    /// document by `plant` before the graph is read back.
    ///
    /// A test plants a node the graph checker refuses, to reach the placing of [`Self::place_graph_diagnostic`].
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Self::parse`].
    #[cfg(test)]
    pub(crate) fn parse_planted(text: &str, plant: impl FnOnce(&mut GraphDocument)) -> Result<Self, SheetFileError> {
        Self::parse_with(text, plant)
    }

    /// Reads, checks and lowers the sheet held in `text`, and passes the lowered document to `edit` before the graph is
    /// read back.
    fn parse_with(text: &str, edit: impl FnOnce(&mut GraphDocument)) -> Result<Self, SheetFileError> {
        let parsed = ParsedSheet::parse(text)?;
        let report = parsed.check();
        if report.has_errors() {
            return Err(invalid(report.errors().map(|error| error.diagnostic.clone())));
        }
        let canonical = parsed
            .document()
            .to_text()
            .map_err(|error| SheetFileError::Format(Box::new(CodeDiagnostic::error(error.to_string()))))?;
        let mut lowered =
            lower::lower(parsed.document(), &report).map_err(|problems| invalid(placed(&parsed, problems)))?;
        edit(&mut lowered.document);
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

    /// Returns `diagnostic`, a finding about the graph of [`Self::lowered`], placed in the text of the sheet.
    ///
    /// A finding about a node is placed at the table of the line the node was lowered from, through
    /// [`Self::line_map`]. A node the sheet owns places the finding at the property, measure, button, card or colour
    /// rule its slot path names, and at `[sheet]` for any other path. A node the line table does not know, such as a
    /// copy that normalisation made, places the finding at `[sheet]`. A finding with a pass and no node is placed at
    /// the card or button the pass was lowered from, or at `[sheet]`.
    ///
    /// The pass and node of the finding are cleared, since neither is in the sheet, and its place in the lowered
    /// graph's text is replaced.
    pub fn place_graph_diagnostic(&self, mut diagnostic: CodeDiagnostic) -> CodeDiagnostic {
        let node = diagnostic.node.take();
        let pass = diagnostic.pass.take();
        let place = match node {
            Some(node) => node
                .parse::<NodeId>()
                .ok()
                .and_then(|id| self.line_map.origin(id))
                .and_then(|origin| self.place_of_origin(origin)),
            None => pass.and_then(|pass| self.place_of_pass(&pass)),
        };
        diagnostic.span = None;
        diagnostic.line = None;
        diagnostic.column = None;
        let span = place
            .and_then(|place| span_of(&self.parsed, &place))
            .or_else(|| span_of(&self.parsed, &Place::Sheet));
        match span {
            Some(span) => self.parsed.place(diagnostic, span),
            None => diagnostic,
        }
    }

    /// Returns the table of the sheet that `origin` names, `None` when no table matches.
    fn place_of_origin(&self, origin: &NodeOrigin) -> Option<Place> {
        let document = self.document();
        match &origin.owner {
            NodeOwner::Line(line) => document
                .cards
                .iter()
                .position(|card| card.line_tables.contains_key(line))
                .map(|card| Place::Line(card, line.clone())),
            NodeOwner::Sheet => {
                let mut segments = origin.path.split('.');
                let part = segments.next()?;
                let id = segments.next()?;
                match part {
                    "cells" => document
                        .cells
                        .as_deref()?
                        .iter()
                        .position(|property| property.id == id)
                        .map(|index| Place::Property(Thing::Cells, index)),
                    "measure" => document
                        .measures
                        .iter()
                        .position(|measure| measure.id == id)
                        .map(Place::Measure),
                    "button" => document
                        .buttons
                        .iter()
                        .position(|button| button.id == id)
                        .map(Place::Button),
                    "card" => document.cards.iter().position(|card| card.id == id).map(Place::Card),
                    "color" => Some(Place::Color(id.to_owned())),
                    _ => None,
                }
            }
        }
    }

    /// Returns the card or button that the lowered pass `pass` runs, `None` for any other pass.
    fn place_of_pass(&self, pass: &str) -> Option<Place> {
        let document = self.document();
        if let Some(card) = pass.strip_prefix("tick_") {
            return document.cards.iter().position(|each| each.id == card).map(Place::Card);
        }
        let button = pass.strip_prefix("button_").or_else(|| pass.strip_prefix("restart_"))?;
        document
            .buttons
            .iter()
            .position(|each| each.id == button)
            .map(Place::Button)
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
