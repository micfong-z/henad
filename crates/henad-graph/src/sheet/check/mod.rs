//! Checker of a sheet: the rules a sheet document meets beyond what the reader checks, each failure reported, all at
//! once.
//!
//! [`check`] reads a [`SheetDocument`] and returns a [`SheetReport`]. A problem of the file as a whole, such as a
//! reference to a parameter that does not exist or a line listed twice, is an error, and an error blocks a build.
//! A problem local to one line, such as a blank slot or a result used above the line that works it out, is a *skip
//! warning*: the line does not run, and neither does any line that uses its result, while the rest of the model runs.
//! The note of a skip warning is the note the card shows under the line, in the words of [`super::sentence`].
//!
//! [`ParsedSheet::check`] checks a sheet read from its text and places every diagnostic in that text, a line's at its
//! line table.

mod declarations;
mod formula;
mod lines;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};

use super::ParsedSheet;
use super::document::{SheetCard, SheetDocument, SheetParameter, SheetProperty};
use super::relations::CardRelations;
use super::sentence::Sentences;

/// Result of checking a sheet.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SheetReport {
    /// Every problem found, errors and warnings. [`ParsedSheet::check`] orders them by their place in the file.
    ///
    /// [`check`] returns them in a stable order: the problems outside the cards, then the line ids of every card,
    /// then for each card its lines lists and its lines in display order.
    pub diagnostics: Vec<SheetDiagnostic>,
}

impl SheetReport {
    /// Returns whether the sheet has an error.
    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }

    /// Returns the errors, in report order.
    pub fn errors(&self) -> impl Iterator<Item = &SheetDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.diagnostic.severity == DiagnosticSeverity::Error)
    }

    /// Returns the warnings, skip warnings included, in report order.
    pub fn warnings(&self) -> impl Iterator<Item = &SheetDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.diagnostic.severity == DiagnosticSeverity::Warning)
    }

    /// Returns whether line `line_id` is skipped: whether it does not run.
    pub fn is_skipped(&self, line_id: &str) -> bool {
        self.skip_note(line_id).is_some()
    }

    /// Returns the note under line `line_id` that says why it does not run, `None` for a line that runs.
    pub fn skip_note(&self, line_id: &str) -> Option<&str> {
        self.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.skips_line && diagnostic.line.as_deref() == Some(line_id))
            .map(|diagnostic| diagnostic.diagnostic.message.as_str())
    }

    /// Returns the ids of the lines that do not run, in report order.
    pub fn skipped(&self) -> impl Iterator<Item = &str> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.skips_line)
            .filter_map(|diagnostic| diagnostic.line.as_deref())
    }
}

/// One problem of a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetDiagnostic {
    /// Severity, message and place of the problem. A skip warning is a warning whose message is the note under the
    /// line.
    pub diagnostic: CodeDiagnostic,
    /// Index in [`SheetDocument::cards`] of the card the problem is in, `None` for a problem outside the cards.
    pub card: Option<usize>,
    /// Id of the line the problem is in, `None` for a problem outside the lines.
    pub line: Option<String>,
    /// Whether the problem keeps [`Self::line`] from running. Such a problem is a skip warning.
    pub skips_line: bool,
}

/// Checks `document` and returns every problem found, with no place in a text.
///
/// ```
/// use henad_graph::sheet::{ParsedSheet, check, starters};
///
/// let sheet = ParsedSheet::parse(starters()[0].text).expect("the starter reads");
/// assert!(check::check(sheet.document()).diagnostics.is_empty());
/// ```
pub fn check(document: &SheetDocument) -> SheetReport {
    report(document, None)
}

impl ParsedSheet {
    /// Checks the sheet and returns every problem found, each placed in the text the sheet was read from and ordered
    /// by its place.
    ///
    /// A problem of a line is placed at the line's table, such as `[card.line.l2]`, and any other problem at the table
    /// it is about.
    pub fn check(&self) -> SheetReport {
        report(self.document(), Some(self))
    }
}

/// Returns the report of `document`, placed in the text of `parsed` when there is one.
fn report(document: &SheetDocument, parsed: Option<&ParsedSheet>) -> SheetReport {
    let mut checker = Checker::new(document);
    declarations::check(&mut checker);
    lines::check(&mut checker);
    let mut placed: Vec<(usize, SheetDiagnostic)> = checker
        .findings
        .into_iter()
        .map(|finding| {
            let base = match finding.severity {
                DiagnosticSeverity::Error => CodeDiagnostic::error(finding.message),
                _ => CodeDiagnostic::warning(finding.message),
            };
            let span = parsed.and_then(|parsed| span_of(parsed, &finding.place));
            let start = span.as_ref().map_or(0, |span| span.start);
            let diagnostic = match (parsed, span) {
                (Some(parsed), Some(span)) => parsed.place(base, span),
                _ => base,
            };
            let (card, line) = match finding.place {
                Place::Card(card) => (Some(card), None),
                Place::Line(card, line) => (Some(card), Some(line)),
                _ => (None, None),
            };
            (
                start,
                SheetDiagnostic {
                    diagnostic,
                    card,
                    line,
                    skips_line: finding.skip,
                },
            )
        })
        .collect();
    if parsed.is_some() {
        placed.sort_by_key(|(start, _)| *start);
    }
    SheetReport {
        diagnostics: placed.into_iter().map(|(_, diagnostic)| diagnostic).collect(),
    }
}

/// Returns the bytes of the table `place` names in the places of `parsed`.
pub(super) fn span_of(parsed: &ParsedSheet, place: &Place) -> Option<Range<usize>> {
    let root = &parsed.places().root;
    let table = match place {
        Place::Sheet => root.table("sheet"),
        Place::World => root.table("world"),
        Place::Time => root.table("time"),
        Place::AgentKind(index) => root.tables("agents").get(*index),
        Place::Property(Thing::Cells, index) => root.table("cells")?.tables("property").get(*index),
        Place::Property(Thing::Agents(kind), index) => root.tables("agents").get(*kind)?.tables("property").get(*index),
        Place::Property(Thing::Ground, index) => root.table("ground")?.tables("property").get(*index),
        Place::Parameter(index) => root.tables("parameter").get(*index),
        Place::Measure(index) => root.tables("measure").get(*index),
        Place::Event(index) => root.tables("event").get(*index),
        Place::Color(thing) => root.table("color")?.table(thing),
        Place::Button(index) => root.tables("button").get(*index),
        Place::Card(index) => parsed.places().card(*index),
        Place::Line(card, line) => parsed.places().line(*card, line),
    };
    table.map(|table| table.table.clone())
}

/// A table of a sheet that a finding is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Place {
    /// The `[sheet]` table.
    Sheet,
    /// The `[world]` table.
    World,
    /// The `[time]` table.
    Time,
    /// The kind of agent at an index of [`SheetDocument::agents`].
    AgentKind(usize),
    /// The property at an index of the properties of a kind of thing.
    Property(Thing, usize),
    /// The parameter at an index of [`SheetDocument::parameters`].
    Parameter(usize),
    /// The measure at an index of [`SheetDocument::measures`].
    Measure(usize),
    /// The event at an index of [`SheetDocument::events`].
    Event(usize),
    /// The colour rule of the kind of thing with an id.
    Color(String),
    /// The button at an index of [`SheetDocument::buttons`].
    Button(usize),
    /// The card at an index of [`SheetDocument::cards`].
    Card(usize),
    /// A line, by the index of its card and its id.
    Line(usize, String),
}

/// A kind of thing whose properties a sheet lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Thing {
    /// The cells of a grid world.
    Cells,
    /// The kind of agent at an index of [`SheetDocument::agents`].
    Agents(usize),
    /// The ground under agents.
    Ground,
}

/// One problem found, before it is placed.
struct Finding {
    /// Error or warning.
    severity: DiagnosticSeverity,
    /// Message, or the note under the line for a skip warning.
    message: String,
    /// Table the problem is about.
    place: Place,
    /// Whether the problem keeps its line from running.
    skip: bool,
}

/// State of one check of a sheet.
struct Checker<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// Words of the sheet's lines, for the notes that name a result.
    sentences: Sentences<'a>,
    /// Problems found so far.
    findings: Vec<Finding>,
    /// Every property, by id, with the id of its kind of thing: `cells`, `ground` or an agent kind's id. The first
    /// property of an id wins.
    properties: HashMap<&'a str, (&'a SheetProperty, &'a str)>,
    /// Every parameter, by id. The first parameter of an id wins.
    parameters: HashMap<&'a str, &'a SheetParameter>,
    /// Ids of the events.
    events: HashSet<&'a str>,
    /// Every card, by id. The first card of an id wins.
    cards: HashMap<&'a str, &'a SheetCard>,
    /// Index of the card that holds each line, by line id. The first card that holds a line wins.
    line_cards: HashMap<&'a str, usize>,
    /// Owner of each draw key, by key.
    keys: BTreeMap<u32, String>,
    /// Relations between the lines of each card, by card index.
    relations: Vec<CardRelations<'a>>,
}

impl<'a> Checker<'a> {
    /// Creates the state of a check of `document`.
    fn new(document: &'a SheetDocument) -> Self {
        let mut properties = HashMap::new();
        for (thing, list) in things(document) {
            for property in list {
                properties.entry(property.id.as_str()).or_insert((property, thing));
            }
        }
        let mut parameters = HashMap::new();
        for parameter in &document.parameters {
            parameters.entry(parameter.id.as_str()).or_insert(parameter);
        }
        let mut cards = HashMap::new();
        let mut line_cards = HashMap::new();
        for (index, card) in document.cards.iter().enumerate() {
            cards.entry(card.id.as_str()).or_insert(card);
            for line_id in card.line_tables.keys() {
                line_cards.entry(line_id.as_str()).or_insert(index);
            }
        }
        Self {
            document,
            sentences: Sentences::new(document),
            findings: Vec::new(),
            properties,
            parameters,
            events: document.events.iter().map(|event| event.id.as_str()).collect(),
            cards,
            line_cards,
            keys: BTreeMap::new(),
            relations: document
                .cards
                .iter()
                .map(|card| CardRelations::new(document, card))
                .collect(),
        }
    }

    /// Records an error about `place`.
    fn error(&mut self, message: impl Into<String>, place: &Place) {
        self.findings.push(Finding {
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            place: place.clone(),
            skip: false,
        });
    }

    /// Records a warning about `place` that does not skip a line.
    fn warning(&mut self, message: impl Into<String>, place: &Place) {
        self.findings.push(Finding {
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            place: place.clone(),
            skip: false,
        });
    }

    /// Records the skip warning of line `line_id` of card `card`, whose note is `note`.
    fn skip(&mut self, note: String, card: usize, line_id: &str) {
        self.findings.push(Finding {
            severity: DiagnosticSeverity::Warning,
            message: note,
            place: Place::Line(card, line_id.to_owned()),
            skip: true,
        });
    }

    /// Returns the property `id` of the kind of thing `thing`, `None` when that kind has no such property.
    fn property_of(&self, id: &str, thing: &str) -> Option<&'a SheetProperty> {
        self.properties
            .get(id)
            .filter(|(_, owner)| *owner == thing)
            .map(|(property, _)| *property)
    }

    /// Returns whether `state` is a state of the property `property`.
    fn has_state(&self, property: &str, state: &str) -> bool {
        self.properties
            .get(property)
            .is_some_and(|(found, _)| found.states.iter().any(|item| item.id == state))
    }

    /// Records the draw key `key` of `owner`, `None` when the owner holds none, where `draws` is whether the owner
    /// draws random numbers.
    ///
    /// A key two owners hold is an error, and an owner that draws and holds no key gets a warning.
    fn key(&mut self, key: Option<u32>, owner: String, draws: bool, place: &Place) {
        match key {
            None if draws => self.warning(format!("{owner} draws and holds no key"), place),
            None => {}
            Some(key) => match self.keys.get(&key) {
                Some(other) => {
                    let message = format!("{owner} holds key {key}, which {other} also holds");
                    self.error(message, place);
                }
                None => {
                    self.keys.insert(key, owner);
                }
            },
        }
    }
}

/// Returns each kind of thing of `document` with its id and properties: the cells, each kind of agent, then the
/// ground.
fn things(document: &SheetDocument) -> Vec<(&str, &[SheetProperty])> {
    let mut out: Vec<(&str, &[SheetProperty])> = Vec::new();
    if let Some(cells) = &document.cells {
        out.push(("cells", cells));
    }
    for kind in &document.agents {
        out.push((kind.id.as_str(), &kind.properties));
    }
    if let Some(ground) = &document.ground {
        out.push(("ground", &ground.properties));
    }
    out
}
