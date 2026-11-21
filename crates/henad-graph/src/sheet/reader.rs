//! Reader of a sheet's text into a [`SheetDocument`], with the place of every table and key.
//!
//! The reader checks what the document's own types need: the header, the `[sheet] version`, the shape of every table,
//! the names of every enum, colours, time units, finite numbers and the forms of percents, chips and phrases. It does
//! not check what a sheet means, such as whether a chip names a line that exists.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use serde::de::value::MapAccessDeserializer;
use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use toml::Spanned;

use henad_core::diagnostic::CodeDiagnostic;
use henad_core::metadata::TimeUnit;

use super::document::{
    AgentKind, BlockedState, ButtonStep, ChooseRow, ColorRule, ColorShade, LookSummary, PaintedShape, SheetButton,
    SheetCard, SheetChip, SheetDocument, SheetEvent, SheetGround, SheetHeader, SheetLine, SheetLiteral, SheetMeasure,
    SheetParameter, SheetProperty, SheetState, SheetTime, SheetValue, SheetWorld, Slot, StartsAs, StartsAsRow,
    StoredName,
};
use super::limits::{MAX_LINES_PER_CARD, MAX_NESTING, MAX_SHEET_BYTES};
use super::{KIND_SHEET, SHEET_VERSION, SheetFileError};
use crate::model_text::{LineEnds, ModelHeader, or_list};
use crate::value::Color;

/// A sheet read from its text: the document, the text, and the places of its tables and keys.
#[derive(Clone)]
pub struct ParsedSheet {
    document: SheetDocument,
    /// Text the sheet was read from.
    text: String,
    /// Places in [`Self::text`] with its line ends made `\n`.
    places: SheetPlaces,
    /// Map from [`Self::places`] back to [`Self::text`].
    line_ends: LineEnds,
}

/// Prints the sheet's id and the byte length of the text.
impl fmt::Debug for ParsedSheet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedSheet")
            .field("id", &self.document.sheet.id)
            .field("cards", &self.document.cards.len())
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

impl ParsedSheet {
    /// Reads the sheet held in `text`.
    ///
    /// A line or phrase may leave out any field or slot, and a slot it leaves out reads as [`Slot::Blank`].
    ///
    /// # Errors
    ///
    /// Returns [`SheetFileError::Format`] for a file past [`MAX_SHEET_BYTES`], for a file of another format, version,
    /// kind or newer sheet version, for tables nested past [`MAX_NESTING`], for a card past [`MAX_LINES_PER_CARD`], and
    /// for a value a sheet cannot hold. The size is checked before parsing, then the header and the sheet version, then
    /// the nesting depth, all before the tables are read.
    ///
    /// Returns [`SheetFileError::Toml`] for text that is not a sheet: TOML that does not parse, a key that is unknown
    /// or missing, or a value of the wrong type. Each diagnostic is placed in `text` when it has a place.
    pub fn parse(text: &str) -> Result<Self, SheetFileError> {
        if text.len() > MAX_SHEET_BYTES {
            return Err(SheetFileError::Format(Box::new(CodeDiagnostic::error(format!(
                "the file holds {} bytes, expected at most {MAX_SHEET_BYTES}",
                text.len()
            )))));
        }
        let (read, line_ends) = LineEnds::normalize(text);
        let place =
            |diagnostic: CodeDiagnostic, span: Range<usize>| diagnostic.at(text, line_ends.original_range(span));
        let toml_error = |error: toml::de::Error| {
            let diagnostic = CodeDiagnostic::error(toml_message(&error));
            SheetFileError::Toml(Box::new(match error.span() {
                Some(span) => place(diagnostic, span),
                None => diagnostic,
            }))
        };
        let format_error = |Placed(message, span): Placed| {
            SheetFileError::Format(Box::new(place(CodeDiagnostic::error(message), span)))
        };

        // The header first, so that a file of another version reads as one even when its keys are new.
        let header = ModelHeader::read(&read).map_err(toml_error)?;
        header
            .check(&[KIND_SHEET])
            .map_err(|problem| format_error(Placed(problem.message, problem.span)))?;
        check_version(&header).map_err(format_error)?;
        let root: Spanned<SpanValue> = toml::from_str(&read).map_err(toml_error)?;
        let (depth, deepest) = nesting(&root, 1);
        if depth > MAX_NESTING {
            return Err(format_error(Placed(
                format!("the file nests tables and lists {depth} deep, expected at most {MAX_NESTING}"),
                deepest,
            )));
        }
        let file: FileTable = toml::from_str(&read).map_err(toml_error)?;
        let document = file.into_document().map_err(format_error)?;
        let span = root.span();
        let places = SheetPlaces {
            root: match root.into_inner() {
                SpanValue::Table(entries) => table_places(span, entries),
                SpanValue::Leaf | SpanValue::Array(_) => TablePlaces::empty(span),
            },
        };
        Ok(Self {
            document,
            text: text.to_owned(),
            places,
            line_ends,
        })
    }

    /// Document the sheet holds.
    pub fn document(&self) -> &SheetDocument {
        &self.document
    }

    /// Returns the document the sheet holds, dropping the text and the places.
    pub fn into_document(self) -> SheetDocument {
        self.document
    }

    /// Text the sheet was read from.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Places of the tables and keys of the sheet.
    pub fn places(&self) -> &SheetPlaces {
        &self.places
    }

    /// Returns `problem` placed at `span`, a byte range of [`Self::places`], in the text the sheet was read from.
    pub fn place(&self, problem: CodeDiagnostic, span: Range<usize>) -> CodeDiagnostic {
        problem.at(&self.text, self.line_ends.original_range(span))
    }
}

/// Places of the tables and keys of a sheet, as byte offsets into its text with line ends made `\n`.
///
/// The places follow the file's TOML tables: [`Self::root`] holds a [`TablePlaces`] for each top-level table, and each
/// of those for the tables under it. A list of tables, such as `[[card]]`, matches the order of its list in the
/// [`SheetDocument`]. A place maps to the file as written through [`ParsedSheet::place`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SheetPlaces {
    /// Places of the whole file.
    pub root: TablePlaces,
}

impl SheetPlaces {
    /// Places of the card at `card_index` in [`SheetDocument::cards`].
    pub fn card(&self, card_index: usize) -> Option<&TablePlaces> {
        self.root.tables("card").get(card_index)
    }

    /// Places of the table of line `line_id` of the card at `card_index`.
    pub fn line(&self, card_index: usize, line_id: &str) -> Option<&TablePlaces> {
        self.card(card_index)?.table("line")?.table(line_id)
    }
}

/// Places of one table of a sheet and of the keys and tables it holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TablePlaces {
    /// The table: its header, such as `[card.line.l2]`, its braces for an inline table, or the first key that names
    /// it for a table the file never writes a header for.
    pub table: Range<usize>,
    /// Value of each key the table holds, by key.
    pub keys: BTreeMap<String, Range<usize>>,
    /// Tables under each key that holds a table or a list with a table in it, by key.
    ///
    /// A key holding a table maps to one entry. A key holding a list maps to one entry per element in list order, and
    /// an element that is no table has the element's bytes and no keys.
    pub tables: BTreeMap<String, Vec<Self>>,
}

impl TablePlaces {
    /// Returns the places of a table at `span` with no keys.
    fn empty(span: Range<usize>) -> Self {
        Self {
            table: span,
            keys: BTreeMap::new(),
            tables: BTreeMap::new(),
        }
    }

    /// Returns the bytes of the value of `key`, `None` when the table does not hold it.
    pub fn key(&self, key: &str) -> Option<Range<usize>> {
        self.keys.get(key).cloned()
    }

    /// Returns the places of the table under `key`, or of the first table of a list under it.
    pub fn table(&self, key: &str) -> Option<&Self> {
        self.tables(key).first()
    }

    /// Returns the places of the tables under `key`, empty when the table holds none.
    pub fn tables(&self, key: &str) -> &[Self] {
        self.tables.get(key).map_or(&[], Vec::as_slice)
    }
}

/// Message and bytes of a value the reader refuses.
struct Placed(String, Range<usize>);

/// Key under which the TOML crate passes a datetime to a deserializer.
const TOML_DATETIME_KEY: &str = "$__toml_private_datetime";

/// Returns the message of `error`, with the list of known keys taken off a message about an unknown key.
///
/// A datetime in place of a table reads as an unknown key that the TOML crate keeps private. That message is replaced
/// by one in the sheet's words.
fn toml_message(error: &toml::de::Error) -> String {
    let message = error.message().trim_end();
    if message.contains(TOML_DATETIME_KEY) {
        return "a sheet cannot hold dates or times".to_owned();
    }
    if message.starts_with("unknown field")
        && let Some((head, _)) = message.split_once(", expected")
    {
        return head.to_owned();
    }
    message.to_owned()
}

/// Checks the `[sheet] version` key read with the header, and returns it when this build cannot read it.
fn check_version(header: &ModelHeader) -> Result<(), Placed> {
    let Some(version) = header.sheet_version() else {
        return Ok(());
    };
    let number = *version.get_ref();
    if number < 1 {
        return Err(Placed(
            format!("the file names sheet version {number}, expected at least 1"),
            version.span(),
        ));
    }
    if number > i64::from(SHEET_VERSION) {
        return Err(Placed(
            format!("the file is sheet version {number}, expected at most {SHEET_VERSION}"),
            version.span(),
        ));
    }
    Ok(())
}

/// Returns the value of `T` that `raw` names.
fn named<T: StoredName>(raw: &Spanned<String>) -> Result<T, Placed> {
    if let Some(value) = T::all().iter().copied().find(|value| value.name() == raw.get_ref()) {
        return Ok(value);
    }
    let names: Vec<&str> = T::all().iter().map(|value| value.name()).collect();
    let message = format!("the {} '{}' is not one of {}", T::WHAT, raw.get_ref(), or_list(&names));
    Err(Placed(message, raw.span()))
}

/// Returns the value of `T` that `raw` names, `None` when the key is absent.
fn named_option<T: StoredName>(raw: Option<&Spanned<String>>) -> Result<Option<T>, Placed> {
    raw.map(named).transpose()
}

/// Returns `raw`, refusing a number that is not finite.
fn finite(raw: &Spanned<f64>) -> Result<f64, Placed> {
    finite_number(*raw.get_ref(), raw.span(), "number")
}

/// Returns `number`, refusing at `span` a number that is not finite, where `what` names it in the message, such as
/// "percent".
fn finite_number(number: f64, span: Range<usize>, what: &str) -> Result<f64, Placed> {
    if number.is_finite() {
        Ok(number)
    } else {
        Err(Placed(format!("the {what} {number} is not finite"), span))
    }
}

/// Returns `raw` with each number checked by [`finite`], `None` when the key is absent.
fn finite_pair(raw: Option<Spanned<[f64; 2]>>) -> Result<Option<[f64; 2]>, Placed> {
    let Some(raw) = raw else { return Ok(None) };
    if raw.get_ref().iter().all(|value| value.is_finite()) {
        Ok(Some(*raw.get_ref()))
    } else {
        Err(Placed("a pair must hold only finite numbers".to_owned(), raw.span()))
    }
}

/// Returns the colour that `raw` writes as `#RRGGBB` or `#RRGGBBAA`.
fn color(raw: &Spanned<String>) -> Result<Color, Placed> {
    let text = raw.get_ref();
    let digits = text.strip_prefix('#').filter(|digits| {
        (digits.len() == 6 || digits.len() == 8) && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    let Some(digits) = digits else {
        return Err(Placed(
            format!("invalid colour '{text}', expected #RRGGBB or #RRGGBBAA"),
            raw.span(),
        ));
    };
    let byte = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).unwrap_or(0);
    let alpha = if digits.len() == 8 { byte(6) } else { 255 };
    Ok([byte(0), byte(2), byte(4), alpha])
}

/// Returns the slot that `raw` fills, [`Slot::Blank`] when the key is absent.
fn slot(raw: Option<Spanned<RawValue>>) -> Result<Slot, Placed> {
    raw.map(value)
        .transpose()
        .map(|value| value.map_or(Slot::Blank, Slot::Filled))
}

/// Returns the value that `raw` holds.
fn value(raw: Spanned<RawValue>) -> Result<SheetValue, Placed> {
    let span = raw.span();
    match raw.into_inner() {
        RawValue::Number(number) => {
            finite_number(number, span, "number").map(|number| SheetValue::Literal(SheetLiteral::Number(number)))
        }
        RawValue::Bool(value) => Ok(SheetValue::Literal(SheetLiteral::Bool(value))),
        RawValue::List(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                let item_span = item.span();
                let item = value(item)?;
                if matches!(item, SheetValue::Phrase(_) | SheetValue::List(_)) {
                    return Err(Placed(
                        "a list item must be a number, a percent, true or false, or a chip".to_owned(),
                        item_span,
                    ));
                }
                values.push(item);
            }
            Ok(SheetValue::List(values))
        }
        RawValue::Table(mut table) => {
            if let Some(percent) = table.percent.take() {
                table.into_percent(&percent).map(SheetValue::Literal)
            } else if let Some(kind) = table.kind.take() {
                Ok(SheetValue::Phrase(Box::new(table.into_line(&kind)?)))
            } else {
                table.into_chip(span).map(SheetValue::Chip)
            }
        }
    }
}

/// A sheet as TOML holds it. [`ModelHeader`] checks the header's values.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTable {
    #[serde(rename = "format")]
    _format: IgnoredAny,
    #[serde(rename = "format_version")]
    _format_version: IgnoredAny,
    #[serde(rename = "kind")]
    _kind: IgnoredAny,
    sheet: SheetTable,
    world: WorldTable,
    #[serde(default)]
    time: Option<TimeTable>,
    #[serde(default)]
    cells: Option<CellsTable>,
    #[serde(default)]
    agents: Vec<AgentsTable>,
    #[serde(default)]
    ground: Option<GroundTable>,
    #[serde(default, rename = "event")]
    events: Vec<EventTable>,
    #[serde(default, rename = "parameter")]
    parameters: Vec<ParameterTable>,
    #[serde(default, rename = "measure")]
    measures: Vec<MeasureTable>,
    #[serde(default, rename = "color")]
    colors: BTreeMap<String, ColorTable>,
    #[serde(default, rename = "button")]
    buttons: Vec<ButtonTable>,
    #[serde(default, rename = "card")]
    cards: Vec<Spanned<CardTable>>,
}

/// The `[sheet]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SheetTable {
    version: u32,
    id: String,
    name: String,
    #[serde(default)]
    purpose: Option<String>,
    next_line: u32,
}

/// The `[world]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldTable {
    made_of: Spanned<String>,
    width: u32,
    height: u32,
    edges: Spanned<String>,
}

/// The `[time]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeTable {
    tick_length: Spanned<f64>,
    tick_unit: Spanned<String>,
}

/// The `[cells]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CellsTable {
    #[serde(default)]
    property: Vec<PropertyTable>,
}

/// An `[[agents]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentsTable {
    id: String,
    name: String,
    count: u32,
    max_count: u32,
    #[serde(default)]
    property: Vec<PropertyTable>,
}

/// The `[ground]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundTable {
    #[serde(default)]
    blocked: Vec<BlockedTable>,
    #[serde(default)]
    property: Vec<PropertyTable>,
}

/// An element of the ground's `blocked` list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockedTable {
    property: String,
    state: String,
}

/// A property table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyTable {
    id: String,
    name: String,
    #[serde(rename = "type")]
    property_type: Spanned<String>,
    #[serde(default)]
    states: Vec<StateTable>,
    #[serde(default)]
    marks: Option<Spanned<String>>,
    #[serde(default)]
    kept_per_tick: Option<Spanned<RawValue>>,
    #[serde(default)]
    starts_as: Option<StartsAsTable>,
    #[serde(default)]
    note: Option<String>,
}

/// An element of a property's `states` list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateTable {
    id: String,
    name: String,
    color: Spanned<String>,
}

/// A property's `starts_as` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartsAsTable {
    rule: Spanned<String>,
    #[serde(default)]
    key: Option<u32>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    property: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    value: Option<Spanned<RawValue>>,
    #[serde(default)]
    otherwise: Option<String>,
    #[serde(default)]
    row: Vec<StartsRowTable>,
    #[serde(default)]
    shape: Vec<ShapeTable>,
}

/// A `[[...starts_as.row]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartsRowTable {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    chance: Option<Spanned<RawValue>>,
}

/// A `[[...starts_as.shape]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapeTable {
    shape: Spanned<String>,
    state: String,
    #[serde(default)]
    center: Option<Spanned<[f64; 2]>>,
    #[serde(default)]
    size: Option<Spanned<[f64; 2]>>,
    #[serde(default)]
    angle: Option<Spanned<f64>>,
}

/// An `[[event]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventTable {
    id: String,
    name: String,
    #[serde(default)]
    means: Option<String>,
}

/// A `[[parameter]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterTable {
    id: String,
    name: String,
    #[serde(rename = "type")]
    parameter_type: Spanned<String>,
    min: Spanned<f64>,
    max: Spanned<f64>,
    default: Spanned<f64>,
    #[serde(default)]
    step: Option<Spanned<f64>>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    format: Option<Spanned<String>>,
    #[serde(default)]
    means: Option<String>,
}

/// A `[[measure]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasureTable {
    id: String,
    name: String,
    color: Spanned<String>,
    rule: Spanned<String>,
    #[serde(default)]
    over: Option<String>,
    #[serde(default)]
    property: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    value: Option<Spanned<RawValue>>,
}

/// A `[color.<thing>]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorTable {
    rule: Spanned<String>,
    #[serde(default)]
    property: Option<String>,
    #[serde(default)]
    shade_state: Option<String>,
    #[serde(default)]
    shades: Vec<ShadeTable>,
    #[serde(default)]
    scale: Option<Spanned<String>>,
    #[serde(default)]
    decades: Option<u32>,
    #[serde(default)]
    hue: Option<Spanned<String>>,
    #[serde(default)]
    darkest: Option<Spanned<f64>>,
    #[serde(default)]
    lightest: Option<Spanned<f64>>,
}

/// An element of a colour rule's `shades` list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShadeTable {
    property: String,
    color: Spanned<String>,
}

/// A `[[button]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ButtonTable {
    id: String,
    name: String,
    #[serde(default)]
    does: Vec<StepTable>,
}

/// A `[[button.does]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepTable {
    rule: Spanned<String>,
    #[serde(default)]
    thing: Option<String>,
    #[serde(default)]
    properties: Vec<String>,
    #[serde(default)]
    property: Option<String>,
    #[serde(default)]
    card: Option<String>,
    #[serde(default)]
    value: Option<Spanned<RawValue>>,
}

/// A `[[card]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardTable {
    id: String,
    every: String,
    runs: Spanned<String>,
    #[serde(default)]
    lines: Vec<String>,
    #[serde(default)]
    line: BTreeMap<String, Spanned<ValueTable>>,
}

/// A table in a slot or under `[card.line]`: a line or phrase when it holds `kind`, and a chip otherwise.
///
/// The table holds every key a line, a phrase or a chip can hold. `property` belongs to all three.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ValueTable {
    #[serde(default)]
    kind: Option<Spanned<String>>,
    #[serde(default)]
    name: Option<Spanned<String>>,
    #[serde(default)]
    key: Option<Spanned<u32>>,
    #[serde(default)]
    property: Option<Spanned<String>>,
    #[serde(default)]
    new_state: Option<Spanned<String>>,
    #[serde(default)]
    start_state: Option<Spanned<String>>,
    #[serde(default)]
    mode: Option<Spanned<String>>,
    #[serde(default)]
    around: Option<Spanned<String>>,
    #[serde(default)]
    agent_kind: Option<Spanned<String>>,
    #[serde(default)]
    distance: Option<Spanned<RawValue>>,
    #[serde(default)]
    include_me: Option<Spanned<bool>>,
    #[serde(default)]
    open_only: Option<Spanned<bool>>,
    #[serde(default)]
    pick: Option<Spanned<String>>,
    #[serde(default)]
    ranked_by: Option<Spanned<String>>,
    #[serde(default)]
    which: Option<Spanned<String>>,
    #[serde(default)]
    place: Option<Spanned<RawValue>>,
    #[serde(default)]
    ground_property: Option<Spanned<String>>,
    #[serde(default)]
    event: Option<Spanned<String>>,
    #[serde(default)]
    test: Option<Spanned<String>>,
    #[serde(default)]
    left: Option<Spanned<RawValue>>,
    #[serde(default)]
    right: Option<Spanned<RawValue>>,
    #[serde(default)]
    tolerance: Option<Spanned<RawValue>>,
    #[serde(default)]
    chance: Option<Spanned<RawValue>>,
    #[serde(default)]
    rate: Option<Spanned<RawValue>>,
    #[serde(default)]
    rate_unit: Option<Spanned<String>>,
    #[serde(default)]
    rate_unit_from_tick: Option<Spanned<bool>>,
    #[serde(default)]
    tries: Option<Spanned<RawValue>>,
    #[serde(default)]
    low: Option<Spanned<RawValue>>,
    #[serde(default)]
    high: Option<Spanned<RawValue>>,
    #[serde(default)]
    length: Option<Spanned<RawValue>>,
    #[serde(default)]
    formula: Option<Spanned<String>>,
    #[serde(default)]
    times: Option<Spanned<RawValue>>,
    #[serde(default)]
    value: Option<Spanned<RawValue>>,
    #[serde(default)]
    strength: Option<Spanned<RawValue>>,
    #[serde(default)]
    minimum: Option<Spanned<RawValue>>,
    #[serde(default)]
    maximum: Option<Spanned<RawValue>>,
    #[serde(default)]
    amount: Option<Spanned<RawValue>>,
    #[serde(default)]
    only: Option<Spanned<RawValue>>,
    #[serde(default)]
    whose: Option<Spanned<RawValue>>,
    #[serde(default)]
    condition: Option<Spanned<RawValue>>,
    #[serde(default)]
    branches: Option<Spanned<BTreeMap<String, Spanned<RawValue>>>>,
    #[serde(default)]
    summaries: Option<Spanned<Vec<SummaryTable>>>,
    #[serde(default)]
    items: Option<Spanned<Vec<Spanned<RawValue>>>>,
    #[serde(default)]
    row: Option<Spanned<Vec<ChooseRowTable>>>,
    #[serde(default)]
    otherwise: Option<Spanned<RawValue>>,
    #[serde(default)]
    lines: Option<Spanned<Vec<String>>>,
    #[serde(default)]
    note: Option<Spanned<String>>,
    // The keys of a chip.
    #[serde(default)]
    param: Option<Spanned<String>>,
    #[serde(default)]
    result: Option<Spanned<String>>,
    #[serde(default)]
    part: Option<Spanned<String>>,
    #[serde(default)]
    my: Option<Spanned<String>>,
    #[serde(default)]
    my_new: Option<Spanned<String>>,
    #[serde(default)]
    their: Option<Spanned<String>>,
    #[serde(default)]
    state: Option<Spanned<String>>,
    #[serde(default)]
    relation: Option<Spanned<String>>,
    // The key of a percent.
    #[serde(default)]
    percent: Option<Spanned<PercentNumber>>,
}

/// An element of a Look around line's `summaries` list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SummaryTable {
    id: String,
    name: String,
    #[serde(default)]
    summary: Option<Spanned<String>>,
    #[serde(default)]
    value: Option<Spanned<RawValue>>,
}

/// A `[[...row]]` table of a Choose line.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChooseRowTable {
    #[serde(default)]
    value: Option<Spanned<RawValue>>,
    #[serde(default)]
    condition: Option<Spanned<RawValue>>,
}

/// A value in a slot as TOML holds it.
///
/// A datetime reaches [`ValueTable`] under a private key of the TOML crate. [`toml_message`] names it as a date.
enum RawValue {
    Number(f64),
    Bool(bool),
    List(Vec<Spanned<Self>>),
    Table(Box<ValueTable>),
}

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RawVisitor)
    }
}

/// Visitor of [`RawValue`].
struct RawVisitor;

impl<'de> Visitor<'de> for RawVisitor {
    type Value = RawValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a number, a percent, true or false, a list, a chip or a phrase")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<RawValue, E> {
        Ok(RawValue::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<RawValue, E> {
        Ok(RawValue::Number(value as f64))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<RawValue, E> {
        Ok(RawValue::Number(value as f64))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<RawValue, E> {
        Ok(RawValue::Number(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawValue, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(RawValue::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RawValue, A::Error> {
        ValueTable::deserialize(MapAccessDeserializer::new(map)).map(|table| RawValue::Table(Box::new(table)))
    }
}

/// The number of a percent as TOML holds it, an integer or a float.
struct PercentNumber(f64);

impl<'de> Deserialize<'de> for PercentNumber {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PercentVisitor)
    }
}

/// Visitor of [`PercentNumber`].
struct PercentVisitor;

impl Visitor<'_> for PercentVisitor {
    type Value = PercentNumber;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a number, as in `{ percent = 30 }`")
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<PercentNumber, E> {
        Ok(PercentNumber(value as f64))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<PercentNumber, E> {
        Ok(PercentNumber(value as f64))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<PercentNumber, E> {
        Ok(PercentNumber(value))
    }
}

/// A TOML value reduced to the bytes of everything inside it, for [`SheetPlaces`].
///
/// A datetime reaches [`SpanValue::Table`] under a private key of the TOML crate. The typed read refuses it first.
enum SpanValue {
    Leaf,
    Array(Vec<Spanned<Self>>),
    Table(Vec<(Spanned<String>, Spanned<Self>)>),
}

impl<'de> Deserialize<'de> for SpanValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SpanVisitor)
    }
}

/// Visitor of [`SpanValue`].
struct SpanVisitor;

impl<'de> Visitor<'de> for SpanVisitor {
    type Value = SpanValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a TOML value")
    }

    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<SpanValue, E> {
        Ok(SpanValue::Leaf)
    }

    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<SpanValue, E> {
        Ok(SpanValue::Leaf)
    }

    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<SpanValue, E> {
        Ok(SpanValue::Leaf)
    }

    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<SpanValue, E> {
        Ok(SpanValue::Leaf)
    }

    fn visit_str<E: de::Error>(self, _value: &str) -> Result<SpanValue, E> {
        Ok(SpanValue::Leaf)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<SpanValue, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(SpanValue::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<SpanValue, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key()? {
            entries.push((key, map.next_value()?));
        }
        Ok(SpanValue::Table(entries))
    }
}

/// Returns the depth of the deepest table or list in `value`, which sits at `depth`, and the bytes of that table or
/// list.
fn nesting(value: &Spanned<SpanValue>, depth: usize) -> (usize, Range<usize>) {
    let children: Vec<&Spanned<SpanValue>> = match value.get_ref() {
        SpanValue::Leaf => return (depth - 1, value.span()),
        SpanValue::Array(items) => items.iter().collect(),
        SpanValue::Table(entries) => entries.iter().map(|(_, child)| child).collect(),
    };
    children
        .into_iter()
        .map(|child| nesting(child, depth + 1))
        .fold(
            (depth, value.span()),
            |deepest, child| if child.0 > deepest.0 { child } else { deepest },
        )
}

/// Returns the places of the table at `span` holding `entries`.
fn table_places(span: Range<usize>, entries: Vec<(Spanned<String>, Spanned<SpanValue>)>) -> TablePlaces {
    let mut places = TablePlaces::empty(span);
    for (key, value) in entries {
        let key = key.into_inner();
        let value_span = value.span();
        places.keys.insert(key.clone(), value_span.clone());
        match value.into_inner() {
            SpanValue::Table(entries) => {
                places.tables.insert(key, vec![table_places(value_span, entries)]);
            }
            SpanValue::Array(items) if items.iter().any(|item| matches!(item.get_ref(), SpanValue::Table(_))) => {
                let tables = items
                    .into_iter()
                    .map(|item| {
                        let item_span = item.span();
                        match item.into_inner() {
                            SpanValue::Table(entries) => table_places(item_span, entries),
                            SpanValue::Leaf | SpanValue::Array(_) => TablePlaces::empty(item_span),
                        }
                    })
                    .collect();
                places.tables.insert(key, tables);
            }
            SpanValue::Leaf | SpanValue::Array(_) => {}
        }
    }
    places
}

impl FileTable {
    /// Returns the document the tables hold.
    fn into_document(self) -> Result<SheetDocument, Placed> {
        let mut sheet = SheetHeader::new(self.sheet.id, self.sheet.name, self.sheet.next_line);
        sheet.version = self.sheet.version;
        sheet.purpose = self.sheet.purpose;
        let world = SheetWorld::new(
            named(&self.world.made_of)?,
            self.world.width,
            self.world.height,
            named(&self.world.edges)?,
        );
        let mut document = SheetDocument::new(sheet, world);
        document.time = self.time.map(TimeTable::into_time).transpose()?;
        document.cells = self.cells.map(|cells| properties(cells.property)).transpose()?;
        for agents in self.agents {
            let mut kind = AgentKind::new(agents.id, agents.name, agents.count, agents.max_count);
            kind.properties = properties(agents.property)?;
            document.agents.push(kind);
        }
        document.ground = self.ground.map(GroundTable::into_ground).transpose()?;
        document.events = self.events.into_iter().map(EventTable::into_event).collect();
        for parameter in self.parameters {
            document.parameters.push(parameter.into_parameter()?);
        }
        for measure in self.measures {
            document.measures.push(measure.into_measure()?);
        }
        for (thing, table) in self.colors {
            document.colors.insert(thing, table.into_rule()?);
        }
        for button in self.buttons {
            document.buttons.push(button.into_button()?);
        }
        for card in self.cards {
            document.cards.push(card_of(card)?);
        }
        Ok(document)
    }
}

impl TimeTable {
    /// Returns the time scale the table declares.
    fn into_time(self) -> Result<SheetTime, Placed> {
        Ok(SheetTime::new(
            finite(&self.tick_length)?,
            named::<TimeUnit>(&self.tick_unit)?,
        ))
    }
}

impl GroundTable {
    /// Returns the ground the table declares.
    fn into_ground(self) -> Result<SheetGround, Placed> {
        Ok(SheetGround {
            blocked: self
                .blocked
                .into_iter()
                .map(|blocked| BlockedState::new(blocked.property, blocked.state))
                .collect(),
            properties: properties(self.property)?,
        })
    }
}

impl EventTable {
    /// Returns the event the table declares.
    fn into_event(self) -> SheetEvent {
        let mut event = SheetEvent::new(self.id, self.name);
        event.means = self.means;
        event
    }
}

impl ParameterTable {
    /// Returns the parameter the table declares.
    fn into_parameter(self) -> Result<SheetParameter, Placed> {
        let mut parameter = SheetParameter::new(
            self.id,
            self.name,
            named(&self.parameter_type)?,
            finite(&self.min)?,
            finite(&self.max)?,
            finite(&self.default)?,
        );
        parameter.step = self.step.as_ref().map(finite).transpose()?;
        parameter.unit = self.unit;
        parameter.format = named_option(self.format.as_ref())?;
        parameter.means = self.means;
        Ok(parameter)
    }
}

impl MeasureTable {
    /// Returns the measure the table declares.
    fn into_measure(self) -> Result<SheetMeasure, Placed> {
        let mut measure = SheetMeasure::new(self.id, self.name, color(&self.color)?, named(&self.rule)?);
        measure.over = self.over;
        measure.property = self.property;
        measure.state = self.state;
        measure.event = self.event;
        measure.value = slot(self.value)?;
        Ok(measure)
    }
}

impl ColorTable {
    /// Returns the colour rule the table declares.
    fn into_rule(self) -> Result<ColorRule, Placed> {
        let mut rule = ColorRule::new(named(&self.rule)?);
        rule.property = self.property;
        rule.shade_state = self.shade_state;
        for shade in self.shades {
            rule.shades.push(ColorShade::new(shade.property, color(&shade.color)?));
        }
        rule.scale = named_option(self.scale.as_ref())?;
        rule.decades = self.decades;
        rule.hue = named_option(self.hue.as_ref())?;
        rule.darkest = self.darkest.as_ref().map(finite).transpose()?;
        rule.lightest = self.lightest.as_ref().map(finite).transpose()?;
        Ok(rule)
    }
}

impl ButtonTable {
    /// Returns the button the table declares.
    fn into_button(self) -> Result<SheetButton, Placed> {
        let mut button = SheetButton::new(self.id, self.name);
        for step in self.does {
            let mut button_step = ButtonStep::new(named(&step.rule)?);
            button_step.thing = step.thing;
            button_step.properties = step.properties;
            button_step.property = step.property;
            button_step.card = step.card;
            button_step.value = slot(step.value)?;
            button.does.push(button_step);
        }
        Ok(button)
    }
}

/// Returns the card that `card` declares, refusing one past [`MAX_LINES_PER_CARD`] at its table.
fn card_of(card: Spanned<CardTable>) -> Result<SheetCard, Placed> {
    let card_span = card.span();
    let card = card.into_inner();
    if card.line.len() > MAX_LINES_PER_CARD {
        return Err(Placed(
            format!(
                "card '{}' holds {} lines, expected at most {MAX_LINES_PER_CARD}",
                card.id,
                card.line.len()
            ),
            card_span,
        ));
    }
    let mut sheet_card = SheetCard::new(card.id, card.every, named(&card.runs)?);
    sheet_card.lines = card.lines;
    for (line_id, table) in card.line {
        let span = table.span();
        let mut table = table.into_inner();
        let Some(kind) = table.kind.take() else {
            return Err(Placed(format!("line {line_id} has no `kind`"), span));
        };
        sheet_card.line_tables.insert(line_id, table.into_line(&kind)?);
    }
    Ok(sheet_card)
}

/// Returns the properties `tables` declare.
fn properties(tables: Vec<PropertyTable>) -> Result<Vec<SheetProperty>, Placed> {
    let mut properties = Vec::with_capacity(tables.len());
    for table in tables {
        let mut property = SheetProperty::new(table.id, table.name, named(&table.property_type)?);
        for state in table.states {
            property
                .states
                .push(SheetState::new(state.id, state.name, color(&state.color)?));
        }
        property.marks = named_option(table.marks.as_ref())?;
        property.kept_per_tick = slot(table.kept_per_tick)?;
        property.starts_as = table.starts_as.map(StartsAsTable::into_starts_as).transpose()?;
        property.note = table.note;
        properties.push(property);
    }
    Ok(properties)
}

impl StartsAsTable {
    /// Returns the start the table declares.
    fn into_starts_as(self) -> Result<StartsAs, Placed> {
        let mut starts_as = StartsAs::new(named(&self.rule)?);
        starts_as.key = self.key;
        starts_as.base = self.base;
        starts_as.property = self.property;
        starts_as.state = self.state;
        starts_as.value = slot(self.value)?;
        starts_as.otherwise = self.otherwise;
        for row in self.row {
            starts_as.rows.push(StartsAsRow {
                state: row.state,
                chance: slot(row.chance)?,
            });
        }
        for shape in self.shape {
            let mut painted = PaintedShape::new(named(&shape.shape)?, shape.state);
            painted.center = finite_pair(shape.center)?;
            painted.size = finite_pair(shape.size)?;
            painted.angle = shape.angle.as_ref().map(finite).transpose()?;
            starts_as.shapes.push(painted);
        }
        Ok(starts_as)
    }
}

/// Returns the value of `raw`, `None` when the key is absent.
fn inner<T>(raw: Option<Spanned<T>>) -> Option<T> {
    raw.map(Spanned::into_inner)
}

impl ValueTable {
    /// Returns the first key of a chip the table holds, with its bytes.
    fn chip_key(&self) -> Option<(&'static str, Range<usize>)> {
        [
            ("param", &self.param),
            ("result", &self.result),
            ("part", &self.part),
            ("my", &self.my),
            ("my_new", &self.my_new),
            ("their", &self.their),
            ("state", &self.state),
            ("relation", &self.relation),
        ]
        .into_iter()
        .find_map(|(key, value)| value.as_ref().map(|value| (key, value.span())))
    }

    /// Returns the first key the table holds that a chip cannot hold, with its bytes.
    fn line_key(&self) -> Option<(&'static str, Range<usize>)> {
        fn span<T>(value: Option<&Spanned<T>>) -> Option<Range<usize>> {
            value.map(Spanned::span)
        }
        [
            ("name", span(self.name.as_ref())),
            ("key", span(self.key.as_ref())),
            ("new_state", span(self.new_state.as_ref())),
            ("start_state", span(self.start_state.as_ref())),
            ("mode", span(self.mode.as_ref())),
            ("around", span(self.around.as_ref())),
            ("agent_kind", span(self.agent_kind.as_ref())),
            ("distance", span(self.distance.as_ref())),
            ("include_me", span(self.include_me.as_ref())),
            ("open_only", span(self.open_only.as_ref())),
            ("pick", span(self.pick.as_ref())),
            ("ranked_by", span(self.ranked_by.as_ref())),
            ("which", span(self.which.as_ref())),
            ("place", span(self.place.as_ref())),
            ("ground_property", span(self.ground_property.as_ref())),
            ("event", span(self.event.as_ref())),
            ("test", span(self.test.as_ref())),
            ("left", span(self.left.as_ref())),
            ("right", span(self.right.as_ref())),
            ("tolerance", span(self.tolerance.as_ref())),
            ("chance", span(self.chance.as_ref())),
            ("rate", span(self.rate.as_ref())),
            ("rate_unit", span(self.rate_unit.as_ref())),
            ("rate_unit_from_tick", span(self.rate_unit_from_tick.as_ref())),
            ("tries", span(self.tries.as_ref())),
            ("low", span(self.low.as_ref())),
            ("high", span(self.high.as_ref())),
            ("length", span(self.length.as_ref())),
            ("formula", span(self.formula.as_ref())),
            ("times", span(self.times.as_ref())),
            ("value", span(self.value.as_ref())),
            ("strength", span(self.strength.as_ref())),
            ("minimum", span(self.minimum.as_ref())),
            ("maximum", span(self.maximum.as_ref())),
            ("amount", span(self.amount.as_ref())),
            ("only", span(self.only.as_ref())),
            ("whose", span(self.whose.as_ref())),
            ("condition", span(self.condition.as_ref())),
            ("branches", span(self.branches.as_ref())),
            ("summaries", span(self.summaries.as_ref())),
            ("items", span(self.items.as_ref())),
            ("row", span(self.row.as_ref())),
            ("otherwise", span(self.otherwise.as_ref())),
            ("lines", span(self.lines.as_ref())),
            ("note", span(self.note.as_ref())),
        ]
        .into_iter()
        .find_map(|(key, span)| span.map(|span| (key, span)))
    }

    /// Returns the line or phrase the table holds, where `kind` is the table's `kind` key, taken out of it.
    fn into_line(self, kind: &Spanned<String>) -> Result<SheetLine, Placed> {
        if let Some((key, key_span)) = self.chip_key() {
            return Err(Placed(
                format!("a line or phrase cannot hold the chip key `{key}`"),
                key_span,
            ));
        }
        if let Some(percent) = &self.percent {
            return Err(Placed(
                "a line or phrase cannot hold the percent key `percent`".to_owned(),
                percent.span(),
            ));
        }
        let mut line = SheetLine::new(named(kind)?);
        line.name = inner(self.name);
        line.key = inner(self.key);
        line.property = inner(self.property);
        line.new_state = inner(self.new_state);
        line.start_state = inner(self.start_state);
        line.mode = named_option(self.mode.as_ref())?;
        line.around = named_option(self.around.as_ref())?;
        line.agent_kind = inner(self.agent_kind);
        line.distance = slot(self.distance)?;
        line.include_me = inner(self.include_me);
        line.open_only = inner(self.open_only);
        line.pick = named_option(self.pick.as_ref())?;
        line.ranked_by = inner(self.ranked_by);
        line.which = named_option(self.which.as_ref())?;
        line.place = slot(self.place)?;
        line.ground_property = inner(self.ground_property);
        line.event = inner(self.event);
        line.test = named_option(self.test.as_ref())?;
        line.left = slot(self.left)?;
        line.right = slot(self.right)?;
        line.tolerance = slot(self.tolerance)?;
        line.chance = slot(self.chance)?;
        line.rate = slot(self.rate)?;
        line.rate_unit = named_option::<TimeUnit>(self.rate_unit.as_ref())?;
        line.rate_unit_from_tick = inner(self.rate_unit_from_tick).unwrap_or(false);
        line.tries = slot(self.tries)?;
        line.low = slot(self.low)?;
        line.high = slot(self.high)?;
        line.length = slot(self.length)?;
        line.formula = inner(self.formula);
        line.times = slot(self.times)?;
        line.value = slot(self.value)?;
        line.strength = slot(self.strength)?;
        line.minimum = slot(self.minimum)?;
        line.maximum = slot(self.maximum)?;
        line.amount = slot(self.amount)?;
        line.only = slot(self.only)?;
        line.whose = slot(self.whose)?;
        line.condition = slot(self.condition)?;
        for (state, raw) in inner(self.branches).unwrap_or_default() {
            line.branches.insert(state, value(raw)?);
        }
        for summary in inner(self.summaries).unwrap_or_default() {
            let mut look = LookSummary::new(summary.id, summary.name);
            look.summary = named_option(summary.summary.as_ref())?;
            look.value = slot(summary.value)?;
            line.summaries.push(look);
        }
        for item in inner(self.items).unwrap_or_default() {
            line.items.push(value(item)?);
        }
        for row in inner(self.row).unwrap_or_default() {
            line.rows.push(ChooseRow {
                value: slot(row.value)?,
                condition: slot(row.condition)?,
            });
        }
        line.otherwise = slot(self.otherwise)?;
        line.lines = inner(self.lines).unwrap_or_default();
        line.note = inner(self.note);
        Ok(line)
    }

    /// Returns the percent the table holds, where `percent` is the table's `percent` key, taken out of it.
    ///
    /// A percent holds `percent` alone and a finite number.
    fn into_percent(self, percent: &Spanned<PercentNumber>) -> Result<SheetLiteral, Placed> {
        let other = [("kind", &self.kind), ("property", &self.property)]
            .into_iter()
            .find_map(|(key, value)| value.as_ref().map(|value| (key, value.span())))
            .or_else(|| self.chip_key())
            .or_else(|| self.line_key());
        if let Some((key, key_span)) = other {
            return Err(Placed(
                format!("a percent cannot hold `{key}` in addition to `percent`"),
                key_span,
            ));
        }
        finite_number(percent.get_ref().0, percent.span(), "percent").map(SheetLiteral::Percent)
    }

    /// Returns the chip the table holds, where `span` is the table's bytes and the table holds no `kind`.
    fn into_chip(self, span: Range<usize>) -> Result<SheetChip, Placed> {
        if let Some((key, key_span)) = self.line_key() {
            return Err(Placed(
                format!("`{key}` is a line or phrase key, but the table has no `kind`"),
                key_span,
            ));
        }
        if self.result.is_none() {
            for (key, value) in [("part", &self.part), ("property", &self.property)] {
                if let Some(value) = value {
                    return Err(Placed(
                        format!("`{key}` requires `result` in the same chip"),
                        value.span(),
                    ));
                }
            }
        }
        let mut names = [
            ("param", self.param),
            ("result", self.result),
            ("my", self.my),
            ("my_new", self.my_new),
            ("their", self.their),
            ("relation", self.relation),
            ("state", self.state),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)));
        let Some((key, name)) = names.next() else {
            return Err(Placed(
                "a chip must have one of `param`, `result`, `my`, `my_new`, `their`, `relation` or `state`".to_owned(),
                span,
            ));
        };
        if let Some((second, second_name)) = names.next() {
            return Err(Placed(
                format!("a chip can name only one thing, got both `{key}` and `{second}`"),
                second_name.span(),
            ));
        }
        Ok(match key {
            "result" => SheetChip::Result {
                line: name.into_inner(),
                part: inner(self.part),
                property: inner(self.property),
            },
            "param" => SheetChip::Param(name.into_inner()),
            "my" => SheetChip::My(name.into_inner()),
            "my_new" => SheetChip::MyNew(name.into_inner()),
            "their" => SheetChip::Their(name.into_inner()),
            "relation" => SheetChip::Relation(named(&name)?),
            _ => SheetChip::State(name.into_inner()),
        })
    }
}
