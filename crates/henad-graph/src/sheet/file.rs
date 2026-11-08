//! Sheet files: a sheet read, checked and lowered to the graph it runs as.

use std::fmt;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::{HOST_WORK_BUDGET, PassKind, SUBMISSION_WORK_BUDGET, wgpu};
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamValue;
use henad_core::provenance::ModelCode;

use super::check::{Place, SheetDiagnostic, SheetReport, Thing, span_of};
use super::document::{CardRuns, LineKind, MadeOf, SheetChip, SheetDocument, SheetValue};
use super::lower::{self, Problem};
use super::relations::CardRelations;
use super::sentence::{self, Sentences};
use super::{
    FireLines, LineMap, NodeOrigin, NodeOwner, ParsedSheet, SHEET_CODE_FORMAT, SheetErrors, SheetFileError, SheetPlaces,
};
use crate::GraphFile;
use crate::document::GraphDocument;
use crate::ids::NodeId;
use crate::wgsl::FaultNames;

/// A sheet file: the sheet, its text and places, its warnings, and the graph it lowers to with the table of that
/// graph's nodes.
///
/// The code identity of a sheet is the lowered graph's, [`GraphFile::code`]. The sheet's name, ids, state names,
/// parameter labels and colours enter it through the graph. Notes, comments, `purpose` and the names of results stay
/// outside it. The lowered graph records the sheet's canonical text as its [author text](GraphFile::author), in the
/// format [`SHEET_CODE_FORMAT`], and so does the source of every entry built from it.
///
/// ```
/// use henad_graph::sheet::{SheetFile, starters};
///
/// let file = SheetFile::parse(starters()[0].text).expect("the starter lowers");
/// assert_eq!(file.lowered().document().name, "Game of Life");
/// assert_eq!(file.code(), file.lowered().code());
/// let author = file.lowered().source().author().cloned().expect("the source records the sheet");
/// assert_eq!(author.text(), file.to_toml());
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
    /// Name in the author's words of each look at agents, by the id of its fold in [`Self::lowered`].
    fault_names: FaultNames,
    /// Probes of the change lines of [`Self::lowered`], each with its line.
    fire_lines: FireLines,
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
            lowered: graph.with_author(ModelCode::new(SHEET_CODE_FORMAT, canonical.clone())),
            canonical,
            line_map: lowered.map,
            fault_names: lowered.fault_names,
            fire_lines: lowered.fires,
        })
    }

    /// Returns the problems that stop the sheet `parsed` lowering, each placed at its table and with the card and the
    /// line it is about when it is about a line, in the order the lowering finds them. `report` is the sheet's check,
    /// [`ParsedSheet::check`].
    ///
    /// The list is empty when the sheet lowers, and when `report` holds an error, since the lowering runs only on a
    /// sheet that checks. A card editor draws each problem about a line under that line, as "Line 3 steers Velocity. A
    /// card that steers a property cannot also set it." under a Set of Velocity.
    pub fn lowering_problems(parsed: &ParsedSheet, report: &SheetReport) -> Vec<SheetDiagnostic> {
        if report.has_errors() {
            return Vec::new();
        }
        let Err(problems) = lower::lower(parsed.document(), report) else {
            return Vec::new();
        };
        problems
            .into_iter()
            .map(|problem| {
                let (card, line) = match &problem.place {
                    Place::Line(card, line) => (Some(*card), Some(line.clone())),
                    Place::Card(card) => (Some(*card), None),
                    _ => (None, None),
                };
                let diagnostic = placed_error(parsed, &problem.place, problem.message);
                SheetDiagnostic {
                    diagnostic,
                    card,
                    line,
                    skips_line: false,
                }
            })
            .collect()
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

    /// Name in the author's words of each look at agents of [`Self::lowered`], by the id of its fold, such as "line 1,
    /// Flockmates".
    ///
    /// The names are for [`AgentProgram::compile_with_fault_names`].
    ///
    /// [`AgentProgram::compile_with_fault_names`]: crate::wgsl::AgentProgram::compile_with_fault_names
    pub fn fault_names(&self) -> &FaultNames {
        &self.fault_names
    }

    /// Probes of the change lines of [`Self::lowered`], each with the line it counts for.
    ///
    /// A GPU program compiled with these probes counts, on its counted step, the elements each change line applied to,
    /// and [`FireLines::counts_by_line`] turns the counts into a count per line. The probes leave the lowered graph's
    /// text and code identity alone.
    pub fn fire_lines(&self) -> &FireLines {
        &self.fire_lines
    }

    /// Returns the reasons a device of `limits` cannot run `entry`, the entry this sheet builds, at `params`, with the
    /// work it costs past a budget in the model's words.
    ///
    /// The reasons are those of [`ModelEntry::shortfalls`]. A submission past its budget names the looks at agents of
    /// the heaviest card, as "Line 1, Flockmates: looks at too many birds at 1,000,000 birds. A shorter distance or
    /// fewer birds would fit.", or the card when it looks at no agents, and starts past the host's budget name the
    /// number started. A reason of another kind, such as a buffer past the device's size, is kept as it is.
    pub fn shortfalls(&self, entry: &ModelEntry, params: &[ParamValue], limits: &wgpu::Limits) -> Vec<String> {
        let found = entry.shortfalls(params, limits);
        let Some(demand) = entry.demand(params, limits) else {
            return found;
        };
        let over_submission = demand.submission_work() > SUBMISSION_WORK_BUDGET;
        let over_host = demand.host.iter().any(|run| run.work > HOST_WORK_BUDGET);
        if !over_submission && !over_host {
            return found;
        }
        let document = self.document();
        let sentences = Sentences::new(document);
        let (things, count) = match document.world.made_of {
            MadeOf::Agents => {
                let kind = document.agents.first().map_or("agents", |kind| kind.id.as_str());
                (
                    sentence::plural(&sentences.thing_word(kind)),
                    param_count(entry, params, &["num_agents"]),
                )
            }
            MadeOf::Cells => (
                sentence::plural(&sentences.thing_word("cells")),
                param_count(entry, params, &["grid_width", "grid_height"]),
            ),
        };
        let mut lines: Vec<String> = found
            .into_iter()
            .filter(|line| !line.contains("units of work"))
            .collect();
        if over_submission {
            let heaviest = demand
                .passes
                .iter()
                .filter(|pass| pass.kind == PassKind::Step)
                .max_by_key(|pass| pass.work);
            let prefix = format!("{}_", document.sheet.id);
            let pass = heaviest
                .and_then(|pass| pass.label.strip_prefix(&prefix))
                .and_then(|id| self.lowered.document().passes.iter().find(|pass| pass.id == id));
            let looks: Vec<&str> = pass
                .map(|pass| {
                    pass.body
                        .nodes
                        .iter()
                        .filter_map(|node| self.fault_names.get(node.id))
                        .collect()
                })
                .unwrap_or_default();
            if looks.is_empty() {
                let card = pass.and_then(|pass| pass.id.strip_prefix("tick_")).and_then(|id| {
                    document
                        .cards
                        .iter()
                        .find(|card| card.id == id && card.runs == CardRuns::EachTick)
                });
                let heading = card.map_or_else(|| sentence::each_tick(None), |card| sentences.card_heading(card));
                lines.push(sentence::card_costs_too_much(&heading, &things, count));
            } else {
                lines.extend(
                    looks
                        .into_iter()
                        .map(|look| sentence::look_costs_too_much(look, &things, count)),
                );
            }
        }
        if over_host {
            lines.push(sentence::start_costs_too_much(&things, count));
        }
        lines
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
    /// graph's text is replaced. A pass whose cost per element passes the limit (rule V22) is reworded in the model's
    /// words, as [`sentence::line_costs_too_much`] writes it for the line or card at its place, and loses its notes.
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
        if crate::check::is_pass_cost(&diagnostic.message) {
            diagnostic.message = self.cost_words(place.as_ref());
            diagnostic.notes.clear();
        }
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

    /// Returns the error on a pass whose cost per element passes the limit, in the model's words, naming the line or
    /// card at `place`.
    ///
    /// A Repeat over a parameter suggests a smaller maximum for it, a Repeat of a fixed number of times fewer times,
    /// and any other line or a card fewer lines.
    fn cost_words(&self, place: Option<&Place>) -> String {
        let document = self.document();
        let sentences = Sentences::new(document);
        let world_thing = match document.world.made_of {
            MadeOf::Agents => document.agents.first().map_or("agents", |kind| kind.id.as_str()),
            MadeOf::Cells => "cells",
        };
        match place {
            Some(Place::Line(card, line_id)) => {
                let card = &document.cards[*card];
                let relations = CardRelations::new(document, card);
                let line = relations.line(line_id);
                let name = sentence::look_name(&[(
                    relations.number(line_id).unwrap_or(0),
                    line.and_then(|line| line.name.as_deref()),
                )]);
                let fix = match line
                    .filter(|line| line.kind == LineKind::Repeat)
                    .map(|line| line.times.value())
                {
                    Some(Some(SheetValue::Chip(SheetChip::Param(id)))) => sentence::smaller_maximum(
                        document
                            .parameters
                            .iter()
                            .find(|parameter| parameter.id == *id)
                            .map_or(id.as_str(), |parameter| parameter.name.as_str()),
                    ),
                    Some(_) => sentence::FEWER_TIMES.to_owned(),
                    None => sentence::FEWER_LINES.to_owned(),
                };
                sentence::line_costs_too_much(&name, &sentences.thing_word(&card.every), &fix)
            }
            Some(Place::Card(card)) => {
                let card = &document.cards[*card];
                sentence::line_costs_too_much(
                    &sentences.card_heading(card),
                    &sentences.thing_word(&card.every),
                    sentence::FEWER_LINES,
                )
            }
            Some(Place::Button(button)) => sentence::line_costs_too_much(
                &document.buttons[*button].name,
                &sentences.thing_word(world_thing),
                sentence::FEWER_LINES,
            ),
            _ => sentence::line_costs_too_much(
                &sentence::each_tick(None),
                &sentences.thing_word(world_thing),
                sentence::FEWER_LINES,
            ),
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
                    "agents" => document
                        .agents
                        .first()?
                        .properties
                        .iter()
                        .position(|property| property.id == id)
                        .map(|index| Place::Property(Thing::Agents(0), index)),
                    "ground" => document
                        .ground
                        .as_ref()?
                        .properties
                        .iter()
                        .position(|property| property.id == id)
                        .map(|index| Place::Property(Thing::Ground, index)),
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
        .map(|problem| placed_error(parsed, &problem.place, problem.message))
        .collect()
}

/// Returns the error `message` of the sheet `parsed`, placed at the table of `place` when the text holds one.
fn placed_error(parsed: &ParsedSheet, place: &Place, message: String) -> CodeDiagnostic {
    let error = CodeDiagnostic::error(message);
    match span_of(parsed, place) {
        Some(span) => parsed.place(error, span),
        None => error,
    }
}

/// Returns the product of the parameters of `entry` named `ids` at `params`: the number of agents, or of cells.
fn param_count(entry: &ModelEntry, params: &[ParamValue], ids: &[&str]) -> u64 {
    ids.iter()
        .map(|id| {
            entry
                .param_descriptors()
                .iter()
                .position(|descriptor| descriptor.id() == *id)
                .and_then(|index| params.get(index))
                .and_then(|value| match value {
                    ParamValue::U32(count) => Some(u64::from(*count)),
                    _ => None,
                })
                .unwrap_or(0)
        })
        .product()
}
