//! Canonical writer of a [`SheetDocument`]: the one text of each document, and the number form a sheet writes.
//!
//! The canonical text is a function of the document alone. It is UTF-8 with `\n` line ends: the three header keys,
//! then one blank line before every table header. Top-level tables follow the order of the sheet, declarations keep
//! their order, keys follow a fixed order per table, and a table writes its inline keys before its sub-tables. Line
//! tables follow line id order, `l9` before `l10`, whatever the display order. A chip, a literal and a list of either
//! are inline, a list of records with no phrase inside is an inline array with one element per line, and a phrase is a
//! standard sub-table. The form depends on the schema and the value's form alone.
//!
//! A *generated comment* sits above each card, holding its heading, and above each line table, holding the line's
//! sentence from [`Sentences`] with the "otherwise" words of [`CardRelations::otherwise_of`]. A reader ignores
//! comments, and a comment written by hand is dropped on the next save.
//!
//! The *number rule*: a whole number within 2^53 is an integer (`3`, `1024`), and any other number is the shortest text
//! that reads back to the same 64-bit float, in plain notation when 10^-4 <= |x| < 10^16 (`0.3`, `0.0005`) and as a
//! mantissa, `e` and an exponent with no `+` and no leading zero otherwise (`1e-5`, `2.5e-7`). `-0` is written `0`.
//! Colours are `"#RRGGBB"` in upper case, or `"#RRGGBBAA"` when alpha is not `FF`.

use std::fmt;

use super::document::{
    AgentKind, ButtonStep, CELLS_ID, ColorRule, GROUND_ID, LookSummary, PaintedShape, SheetButton, SheetCard,
    SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetMeasure, SheetParameter, SheetProperty, SheetValue, Slot,
    StartsAs,
};
use super::relations::CardRelations;
use super::sentence::Sentences;
use crate::file::{FORMAT, FORMAT_VERSION};
use crate::model_text::{color_hex, multiline_array, push_key, string_array, toml_key, toml_string};

/// Largest magnitude below which a whole number is written as an integer, 2^53.
const WHOLE_LIMIT: f64 = 9_007_199_254_740_992.0;

/// Returns `value` in the sheet's number form, or `None` for NaN and the infinities.
///
/// ```
/// use henad_graph::sheet::format_number;
///
/// assert_eq!(format_number(1024.0).as_deref(), Some("1024"));
/// assert_eq!(format_number(0.0005).as_deref(), Some("0.0005"));
/// assert_eq!(format_number(2.5e-7).as_deref(), Some("2.5e-7"));
/// assert_eq!(format_number(f64::NAN), None);
/// ```
pub fn format_number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == value.trunc() && value.abs() < WHOLE_LIMIT {
        // Within 2^53 the value is an integer of an i64, and `-0.0` turns into `0`.
        return Some(format!("{}", value as i64));
    }
    // `{:e}` writes the shortest digits that read back to the same float, as `d.ddde<exponent>`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if value < 0.0 { "-" } else { "" };
    let magnitude = value.abs();
    if !(1e-4..1e16).contains(&magnitude) {
        return Some(format!("{sign}{mantissa}e{exponent}"));
    }
    let point = exponent + 1;
    let text = if point <= 0 {
        format!("0.{}{digits}", "0".repeat(point.unsigned_abs() as usize))
    } else if point as usize >= digits.len() {
        // A whole number from 2^53 up keeps a fraction, as Python's `repr` writes `9007199254740992.0`.
        format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
    } else {
        let (whole, fraction) = digits.split_at(point as usize);
        format!("{whole}.{fraction}")
    };
    Some(format!("{sign}{text}"))
}

/// Reason a sheet cannot be written: a number that is not finite.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SheetWriteError {
    /// Key whose value is not finite, such as `default`.
    pub key: String,
    /// The value: NaN or an infinity.
    pub value: f64,
}

/// Writes the key and its value.
impl fmt::Display for SheetWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the value of `{}` is {}, and a sheet holds only finite numbers",
            self.key, self.value
        )
    }
}

impl std::error::Error for SheetWriteError {}

impl SheetDocument {
    /// Returns the canonical text of the sheet, with a generated comment above each card and each line table.
    ///
    /// Reading the text returns the same document, and writing an unedited document returns the text it was read from
    /// when that text is canonical.
    ///
    /// # Errors
    ///
    /// Returns the first number that is not finite, in the order of the text.
    pub fn to_text(&self) -> Result<String, SheetWriteError> {
        let writer = Writer {
            document: self,
            sentences: Sentences::new(self),
        };
        let record = writer.top()?;
        let mut lines = Vec::new();
        write_record(&mut lines, &[], &record, false);
        let mut text = lines.join("\n");
        text.push('\n');
        Ok(text)
    }
}

/// A value under a key of a table that is being written.
enum Entry {
    /// A value written on the key's line.
    Inline(String),
    /// An inline array of records, one element per line, each element written as TOML.
    Rows(Vec<String>),
    /// A standard table, `[path.key]`.
    Table(Record),
    /// An array of tables, `[[path.key]]`, one table per element.
    Tables(Vec<Record>),
    /// A table per id, `[path.key.<id>]`, in order.
    Map(Vec<(String, Record)>),
}

/// A table that is being written: its keys in order, and the comment above its header.
#[derive(Default)]
struct Record {
    /// Keys of the table and their values, in the order they are written. A value that is a table follows the inline
    /// values when written.
    entries: Vec<(String, Entry)>,
    /// Comment above the table's header.
    comment: Option<String>,
}

impl Record {
    /// Appends the inline value `value` under `key`.
    fn inline(&mut self, key: &str, value: String) {
        self.entries.push((key.to_owned(), Entry::Inline(value)));
    }

    /// Appends the string `value` under `key`.
    fn string(&mut self, key: &str, value: &str) {
        self.inline(key, toml_string(value));
    }

    /// Appends the string `value` under `key` when it is `Some`.
    fn optional_string(&mut self, key: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.string(key, value);
        }
    }

    /// Appends `entry` under `key` when it is `Some`.
    fn entry(&mut self, key: &str, entry: Option<Entry>) {
        if let Some(entry) = entry {
            self.entries.push((key.to_owned(), entry));
        }
    }

    /// Appends the record `record` as a standard table under `key`.
    fn table(&mut self, key: &str, record: Self) {
        self.entries.push((key.to_owned(), Entry::Table(record)));
    }

    /// Appends `rows` as an inline array with one element per line under `key`, and nothing when there is none.
    fn rows(&mut self, key: &str, rows: Vec<String>) {
        if !rows.is_empty() {
            self.entries.push((key.to_owned(), Entry::Rows(rows)));
        }
    }

    /// Appends the number `value` under `key` in the number rule when it is `Some`.
    ///
    /// # Errors
    ///
    /// Returns NaN or an infinity.
    fn optional_number(&mut self, key: &str, value: Option<f64>) -> Result<(), SheetWriteError> {
        if let Some(value) = value {
            self.inline(key, number(key, value)?);
        }
        Ok(())
    }

    /// Appends the records `records` as an array of tables under `key`, and nothing when there is none.
    fn tables(&mut self, key: &str, records: Vec<Self>) {
        if !records.is_empty() {
            self.entries.push((key.to_owned(), Entry::Tables(records)));
        }
    }
}

/// Appends the lines of `record`, the table at `path`, to `out`: its header, its inline keys, then its tables.
///
/// `array` is whether the table is an element of an array of tables. A table with no inline key that holds tables
/// writes no header of its own, as TOML allows.
fn write_record(out: &mut Vec<String>, path: &[String], record: &Record, array: bool) {
    let inline = || {
        record
            .entries
            .iter()
            .filter(|(_, entry)| matches!(entry, Entry::Inline(_) | Entry::Rows(_)))
    };
    let has_tables = record
        .entries
        .iter()
        .any(|(_, entry)| matches!(entry, Entry::Table(_) | Entry::Tables(_) | Entry::Map(_)));
    if !path.is_empty() && (inline().next().is_some() || array || !has_tables) {
        if out.last().is_some_and(|last| !last.is_empty()) {
            out.push(String::new());
        }
        if let Some(comment) = &record.comment {
            out.push(format!("# {}", comment_text(comment)));
        }
        let header = path.join(".");
        out.push(if array {
            format!("[[{header}]]")
        } else {
            format!("[{header}]")
        });
    }
    for (key, entry) in inline() {
        let value = match entry {
            Entry::Inline(value) => value.clone(),
            Entry::Rows(rows) => multiline_array(rows),
            _ => continue,
        };
        let mut line = String::new();
        push_key(&mut line, &toml_key(key), &value);
        line.pop();
        out.push(line);
    }
    for (key, entry) in &record.entries {
        let child = |segment: &str| {
            let mut child = path.to_vec();
            child.push(toml_key(segment));
            child
        };
        match entry {
            Entry::Table(table) => write_record(out, &child(key), table, false),
            Entry::Tables(tables) => {
                for table in tables {
                    write_record(out, &child(key), table, true);
                }
            }
            Entry::Map(tables) => {
                for (id, table) in tables {
                    let mut path = child(key);
                    path.push(toml_key(id));
                    write_record(out, &path, table, false);
                }
            }
            Entry::Inline(_) | Entry::Rows(_) => {}
        }
    }
}

/// Returns `record` as an inline table on one line, its tables written inline too.
fn inline_record(record: &Record) -> String {
    let entries: Vec<String> = record
        .entries
        .iter()
        .map(|(key, entry)| {
            let value = match entry {
                Entry::Inline(value) => value.clone(),
                Entry::Rows(rows) => format!("[{}]", rows.join(", ")),
                Entry::Table(table) => inline_record(table),
                Entry::Tables(tables) => {
                    let tables: Vec<String> = tables.iter().map(inline_record).collect();
                    format!("[{}]", tables.join(", "))
                }
                Entry::Map(tables) => {
                    let tables: Vec<String> = tables
                        .iter()
                        .map(|(id, table)| format!("{} = {}", toml_key(id), inline_record(table)))
                        .collect();
                    format!("{{ {} }}", tables.join(", "))
                }
            };
            format!("{} = {value}", toml_key(key))
        })
        .collect();
    if entries.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", entries.join(", "))
    }
}

/// Returns `comment` on one line, each control character made a space. A line break would end the TOML comment.
fn comment_text(comment: &str) -> String {
    comment.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Returns the line ids `ids`, which come in lexical order, sorted in line id order: `l<number>` by number, then any
/// other id in lexical order.
fn line_order<'a>(ids: impl Iterator<Item = &'a String>) -> Vec<&'a String> {
    let mut ids: Vec<&String> = ids.collect();
    ids.sort_by_key(|id| {
        let number = id
            .strip_prefix('l')
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u64>().ok());
        (number.is_none(), number)
    });
    ids
}

/// Builder of the records of one sheet.
struct Writer<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// The sentences of the sheet's lines, for the generated comments.
    sentences: Sentences<'a>,
}

impl Writer<'_> {
    /// Returns the record of the whole file.
    fn top(&self) -> Result<Record, SheetWriteError> {
        let document = self.document;
        let mut top = Record::default();
        top.string("format", FORMAT);
        top.inline("format_version", FORMAT_VERSION.to_string());
        top.string("kind", super::KIND_SHEET);

        let mut sheet = Record::default();
        sheet.inline("version", document.sheet.version.to_string());
        sheet.string("id", &document.sheet.id);
        sheet.string("name", &document.sheet.name);
        sheet.optional_string("purpose", document.sheet.purpose.as_deref());
        sheet.inline("next_line", document.sheet.next_line.to_string());
        top.table("sheet", sheet);

        let mut world = Record::default();
        world.string("made_of", document.world.made_of.as_str());
        world.inline("width", document.world.width.to_string());
        world.inline("height", document.world.height.to_string());
        world.string("edges", document.world.edges.as_str());
        top.table("world", world);

        if let Some(time) = &document.time {
            let mut table = Record::default();
            table.inline("tick_length", number("tick_length", time.tick_length)?);
            table.string("tick_unit", time.tick_unit.as_str());
            top.table("time", table);
        }
        if let Some(cells) = &document.cells {
            let mut table = Record::default();
            table.tables("property", self.properties(cells)?);
            top.table("cells", table);
        }
        let agents = document
            .agents
            .iter()
            .map(|kind| self.agent_kind(kind))
            .collect::<Result<_, _>>()?;
        top.tables("agents", agents);
        if let Some(ground) = &document.ground {
            let mut table = Record::default();
            let blocked: Vec<String> = ground
                .blocked
                .iter()
                .map(|blocked| {
                    format!(
                        "{{ property = {}, state = {} }}",
                        toml_string(&blocked.property),
                        toml_string(&blocked.state)
                    )
                })
                .collect();
            table.rows("blocked", blocked);
            table.tables("property", self.properties(&ground.properties)?);
            top.table("ground", table);
        }
        let events = document
            .events
            .iter()
            .map(|event| {
                let mut table = Record::default();
                table.string("id", &event.id);
                table.string("name", &event.name);
                table.optional_string("means", event.means.as_deref());
                table
            })
            .collect();
        top.tables("event", events);
        let parameters = document.parameters.iter().map(parameter).collect::<Result<_, _>>()?;
        top.tables("parameter", parameters);
        let measures = document
            .measures
            .iter()
            .map(|measure| self.measure(measure))
            .collect::<Result<_, _>>()?;
        top.tables("measure", measures);
        let colors = self.colors()?;
        if !colors.is_empty() {
            top.entry("color", Some(Entry::Map(colors)));
        }
        let buttons = document
            .buttons
            .iter()
            .map(|button| self.button(button))
            .collect::<Result<_, _>>()?;
        top.tables("button", buttons);
        let cards = document
            .cards
            .iter()
            .map(|card| self.card(card))
            .collect::<Result<_, _>>()?;
        top.tables("card", cards);
        Ok(top)
    }

    /// Returns a record per property of `properties`.
    fn properties(&self, properties: &[SheetProperty]) -> Result<Vec<Record>, SheetWriteError> {
        properties.iter().map(|property| self.property(property)).collect()
    }

    /// Returns the record of `property`.
    fn property(&self, property: &SheetProperty) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("id", &property.id);
        table.string("name", &property.name);
        table.string("type", property.property_type.as_str());
        let states: Vec<String> = property
            .states
            .iter()
            .map(|state| {
                format!(
                    "{{ id = {}, name = {}, color = {} }}",
                    toml_string(&state.id),
                    toml_string(&state.name),
                    color_hex(&state.color)
                )
            })
            .collect();
        table.rows("states", states);
        if let Some(marks) = property.marks {
            table.string("marks", marks.as_str());
        }
        self.put_slot(&mut table, "kept_per_tick", &property.kept_per_tick)?;
        if let Some(starts_as) = &property.starts_as {
            table.table("starts_as", self.starts_as(starts_as)?);
        }
        table.optional_string("note", property.note.as_deref());
        Ok(table)
    }

    /// Returns the record of a property's `starts_as` table.
    fn starts_as(&self, starts_as: &StartsAs) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("rule", starts_as.rule.as_str());
        if let Some(key) = starts_as.key {
            table.inline("key", key.to_string());
        }
        table.optional_string("base", starts_as.base.as_deref());
        table.optional_string("property", starts_as.property.as_deref());
        table.optional_string("state", starts_as.state.as_deref());
        self.put_slot(&mut table, "value", &starts_as.value)?;
        table.optional_string("otherwise", starts_as.otherwise.as_deref());
        let rows = starts_as
            .rows
            .iter()
            .map(|row| {
                let mut record = Record::default();
                record.optional_string("state", row.state.as_deref());
                self.put_slot(&mut record, "chance", &row.chance)?;
                Ok(record)
            })
            .collect::<Result<_, _>>()?;
        table.tables("row", rows);
        let shapes = starts_as.shapes.iter().map(shape).collect::<Result<_, _>>()?;
        table.tables("shape", shapes);
        Ok(table)
    }

    /// Returns the record of `kind` and its properties.
    fn agent_kind(&self, kind: &AgentKind) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("id", &kind.id);
        table.string("name", &kind.name);
        table.inline("count", kind.count.to_string());
        table.inline("max_count", kind.max_count.to_string());
        table.tables("property", self.properties(&kind.properties)?);
        Ok(table)
    }

    /// Returns the record of `measure`.
    fn measure(&self, measure: &SheetMeasure) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("id", &measure.id);
        table.string("name", &measure.name);
        table.inline("color", color_hex(&measure.color));
        table.string("rule", measure.rule.as_str());
        table.optional_string("over", measure.over.as_deref());
        table.optional_string("property", measure.property.as_deref());
        table.optional_string("state", measure.state.as_deref());
        table.optional_string("event", measure.event.as_deref());
        self.put_slot(&mut table, "value", &measure.value)?;
        Ok(table)
    }

    /// Returns the colour rule of each kind of thing, the cells, then each agent kind, then the ground, then any other
    /// id in lexical order.
    ///
    /// # Errors
    ///
    /// Returns the key of a shading's end that holds a number that is not finite.
    fn colors(&self) -> Result<Vec<(String, Record)>, SheetWriteError> {
        let document = self.document;
        let things: Vec<&str> = std::iter::once(CELLS_ID)
            .chain(document.agents.iter().map(|kind| kind.id.as_str()))
            .chain(std::iter::once(GROUND_ID))
            .collect();
        let mut ids: Vec<&String> = document.colors.keys().collect();
        ids.sort_by_key(|id| {
            let place = things.iter().position(|thing| *thing == id.as_str());
            (place.is_none(), place)
        });
        ids.into_iter()
            .map(|id| Ok((id.clone(), color_rule(&document.colors[id])?)))
            .collect()
    }

    /// Returns the record of `button` and its steps.
    fn button(&self, button: &SheetButton) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("id", &button.id);
        table.string("name", &button.name);
        let steps = button
            .does
            .iter()
            .map(|step| self.button_step(step))
            .collect::<Result<_, _>>()?;
        table.tables("does", steps);
        Ok(table)
    }

    /// Returns the record of one step of a button.
    fn button_step(&self, step: &ButtonStep) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("rule", step.rule.as_str());
        table.optional_string("thing", step.thing.as_deref());
        if !step.properties.is_empty() {
            table.inline("properties", string_array(&step.properties));
        }
        table.optional_string("property", step.property.as_deref());
        table.optional_string("card", step.card.as_deref());
        self.put_slot(&mut table, "value", &step.value)?;
        Ok(table)
    }

    /// Returns the record of `card`, with its heading as its comment and each line table under it in line id order.
    fn card(&self, card: &SheetCard) -> Result<Record, SheetWriteError> {
        let mut table = Record {
            comment: Some(self.sentences.card_heading(card)),
            ..Record::default()
        };
        table.string("id", &card.id);
        table.string("every", &card.every);
        table.string("runs", card.runs.as_str());
        table.inline("lines", string_array(&card.lines));
        let relations = CardRelations::new(self.document, card);
        let lines = line_order(card.line_tables.keys())
            .into_iter()
            .map(|id| {
                let mut record = self.line(&card.line_tables[id])?;
                record.comment = Some(self.sentences.line(id, &relations.otherwise_numbers(id)));
                Ok((id.clone(), record))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !lines.is_empty() {
            table.entry("line", Some(Entry::Map(lines)));
        }
        Ok(table)
    }

    /// Returns the record of a line or phrase, its keys in the fixed order of a line table.
    fn line(&self, line: &SheetLine) -> Result<Record, SheetWriteError> {
        let mut table = Record::default();
        table.string("kind", line.kind.as_str());
        table.optional_string("name", line.name.as_deref());
        if let Some(key) = line.key {
            table.inline("key", key.to_string());
        }
        table.optional_string("property", line.property.as_deref());
        table.optional_string("new_state", line.new_state.as_deref());
        table.optional_string("start_state", line.start_state.as_deref());
        table.optional_string("mode", line.mode.map(|mode| mode.as_str()));
        table.optional_string("around", line.around.map(|around| around.as_str()));
        table.optional_string("agent_kind", line.agent_kind.as_deref());
        self.put_slot(&mut table, "distance", &line.distance)?;
        if let Some(include_me) = line.include_me {
            table.inline("include_me", include_me.to_string());
        }
        if let Some(open_only) = line.open_only {
            table.inline("open_only", open_only.to_string());
        }
        table.optional_string("pick", line.pick.map(|pick| pick.as_str()));
        table.optional_string("ranked_by", line.ranked_by.as_deref());
        table.optional_string("which", line.which.map(|which| which.as_str()));
        self.put_slot(&mut table, "place", &line.place)?;
        table.optional_string("ground_property", line.ground_property.as_deref());
        table.optional_string("event", line.event.as_deref());
        table.optional_string("test", line.test.map(|test| test.as_str()));
        for (key, slot) in [
            ("left", &line.left),
            ("right", &line.right),
            ("tolerance", &line.tolerance),
        ] {
            self.put_slot(&mut table, key, slot)?;
        }
        self.put_slot(&mut table, "chance", &line.chance)?;
        self.put_slot(&mut table, "rate", &line.rate)?;
        table.optional_string("rate_unit", line.rate_unit.map(|unit| unit.as_str()));
        if line.rate_unit_from_tick {
            table.inline("rate_unit_from_tick", "true".to_owned());
        }
        for (key, slot) in [
            ("tries", &line.tries),
            ("low", &line.low),
            ("high", &line.high),
            ("length", &line.length),
        ] {
            self.put_slot(&mut table, key, slot)?;
        }
        table.optional_string("formula", line.formula.as_deref());
        for (key, slot) in [
            ("times", &line.times),
            ("value", &line.value),
            ("strength", &line.strength),
            ("minimum", &line.minimum),
            ("maximum", &line.maximum),
            ("amount", &line.amount),
            ("only", &line.only),
            ("whose", &line.whose),
            ("condition", &line.condition),
        ] {
            self.put_slot(&mut table, key, slot)?;
        }
        if !line.branches.is_empty() {
            table.inline("branches", self.branches(line)?);
        }
        table.entry("summaries", self.summaries(&line.summaries)?);
        table.entry("items", self.items(&line.items)?);
        let rows = line
            .rows
            .iter()
            .map(|row| {
                let mut record = Record::default();
                self.put_slot(&mut record, "value", &row.value)?;
                self.put_slot(&mut record, "condition", &row.condition)?;
                Ok(record)
            })
            .collect::<Result<_, _>>()?;
        table.tables("row", rows);
        self.put_slot(&mut table, "otherwise", &line.otherwise)?;
        if !line.lines.is_empty() {
            table.inline("lines", string_array(&line.lines));
        }
        table.optional_string("note", line.note.as_deref());
        Ok(table)
    }

    /// Returns a Depending on line's branches as an inline table, in the state order of its property, then any other
    /// state id in lexical order.
    fn branches(&self, line: &SheetLine) -> Result<String, SheetWriteError> {
        let states: Vec<&str> = self
            .document
            .cells
            .iter()
            .flatten()
            .chain(self.document.agents.iter().flat_map(|kind| &kind.properties))
            .chain(self.document.ground.iter().flat_map(|ground| &ground.properties))
            .find(|property| line.property.as_deref() == Some(property.id.as_str()))
            .map(|property| property.states.iter().map(|state| state.id.as_str()).collect())
            .unwrap_or_default();
        let mut ids: Vec<&String> = line.branches.keys().collect();
        ids.sort_by_key(|id| {
            let place = states.iter().position(|state| *state == id.as_str());
            (place.is_none(), place)
        });
        let pairs = ids
            .into_iter()
            .map(|id| {
                Ok(format!(
                    "{} = {}",
                    toml_key(id),
                    self.inline_value(id, &line.branches[id])?
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(format!("{{ {} }}", pairs.join(", ")))
    }

    /// Returns a Look around line's summaries: an inline array with one summary per line, or an array of tables when a
    /// summary's value is a phrase. `None` when there is no summary.
    fn summaries(&self, summaries: &[LookSummary]) -> Result<Option<Entry>, SheetWriteError> {
        if summaries.is_empty() {
            return Ok(None);
        }
        let records = summaries
            .iter()
            .map(|summary| {
                let mut record = Record::default();
                record.string("id", &summary.id);
                record.string("name", &summary.name);
                record.optional_string("summary", summary.summary.map(|kind| kind.as_str()));
                self.put_slot(&mut record, "value", &summary.value)?;
                Ok(record)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let holds_phrase = summaries
            .iter()
            .any(|summary| matches!(summary.value.value(), Some(SheetValue::Phrase(_))));
        Ok(Some(if holds_phrase {
            Entry::Tables(records)
        } else {
            Entry::Rows(records.iter().map(inline_record).collect())
        }))
    }

    /// Returns the items of an all of, any of or none of: an array of tables when every item is a chip or a phrase,
    /// and an inline array otherwise. `None` when there is no item.
    fn items(&self, items: &[SheetValue]) -> Result<Option<Entry>, SheetWriteError> {
        if items.is_empty() {
            return Ok(None);
        }
        let records = items
            .iter()
            .map(|item| match item {
                SheetValue::Phrase(phrase) => self.line(phrase).map(Some),
                SheetValue::Chip(chip) => Ok(Some(chip_record(chip))),
                SheetValue::Literal(_) | SheetValue::List(_) => Ok(None),
            })
            .collect::<Result<Option<Vec<_>>, _>>()?;
        if let Some(records) = records {
            return Ok(Some(Entry::Tables(records)));
        }
        let values = items
            .iter()
            .map(|item| self.inline_value("items", item))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(Entry::Inline(format!("[{}]", values.join(", ")))))
    }

    /// Appends `slot` to `table` under `key`: nothing for a blank slot, a standard table for a phrase, and an inline
    /// value otherwise.
    fn put_slot(&self, table: &mut Record, key: &str, slot: &Slot) -> Result<(), SheetWriteError> {
        let entry = match slot.value() {
            None => None,
            Some(SheetValue::Phrase(phrase)) => Some(Entry::Table(self.line(phrase)?)),
            Some(value) => Some(Entry::Inline(self.inline_value(key, value)?)),
        };
        table.entry(key, entry);
        Ok(())
    }

    /// Returns `value`, held under `key`, as an inline value, a phrase as an inline table.
    fn inline_value(&self, key: &str, value: &SheetValue) -> Result<String, SheetWriteError> {
        Ok(match value {
            SheetValue::Literal(literal) => literal_text(key, *literal)?,
            SheetValue::Chip(chip) => inline_record(&chip_record(chip)),
            SheetValue::Phrase(phrase) => inline_record(&self.line(phrase)?),
            SheetValue::List(items) => {
                let items = items
                    .iter()
                    .map(|item| self.inline_value(key, item))
                    .collect::<Result<Vec<_>, _>>()?;
                format!("[{}]", items.join(", "))
            }
        })
    }
}

/// Returns `value`, held under `key`, in the number rule.
///
/// # Errors
///
/// Returns NaN or an infinity.
fn number(key: &str, value: f64) -> Result<String, SheetWriteError> {
    format_number(value).ok_or_else(|| SheetWriteError {
        key: key.to_owned(),
        value,
    })
}

/// Returns `literal`, held under `key`: a number, `{ percent = N }`, `true` or `false`.
fn literal_text(key: &str, literal: SheetLiteral) -> Result<String, SheetWriteError> {
    Ok(match literal {
        SheetLiteral::Number(value) => number(key, value)?,
        SheetLiteral::Percent(value) => format!("{{ percent = {} }}", number(key, value)?),
        SheetLiteral::Bool(flag) => flag.to_string(),
    })
}

/// Returns the record of `chip`, its keys in the order `param`, `result`, `part`, `property`, `my`, `my_new`, `their`,
/// `state`, `relation`.
fn chip_record(chip: &SheetChip) -> Record {
    let mut record = Record::default();
    match chip {
        SheetChip::Param(id) => record.string("param", id),
        SheetChip::Result { line, part, property } => {
            record.string("result", line);
            record.optional_string("part", part.as_deref());
            record.optional_string("property", property.as_deref());
        }
        SheetChip::My(id) => record.string("my", id),
        SheetChip::MyNew(id) => record.string("my_new", id),
        SheetChip::Their(id) => record.string("their", id),
        SheetChip::State(id) => record.string("state", id),
        SheetChip::Relation(relation) => record.string("relation", relation.as_str()),
    }
    record
}

/// Returns the record of `parameter`.
fn parameter(parameter: &SheetParameter) -> Result<Record, SheetWriteError> {
    let mut table = Record::default();
    table.string("id", &parameter.id);
    table.string("name", &parameter.name);
    table.string("type", parameter.parameter_type.as_str());
    table.inline("min", number("min", parameter.min)?);
    table.inline("max", number("max", parameter.max)?);
    table.inline("default", number("default", parameter.default)?);
    table.optional_number("step", parameter.step)?;
    table.optional_string("unit", parameter.unit.as_deref());
    table.optional_string("format", parameter.format.map(|format| format.as_str()));
    table.optional_string("means", parameter.means.as_deref());
    Ok(table)
}

/// Returns the record of one shape of a painted start.
fn shape(shape: &PaintedShape) -> Result<Record, SheetWriteError> {
    let mut table = Record::default();
    table.string("shape", shape.shape.as_str());
    table.string("state", &shape.state);
    for (key, pair) in [("center", shape.center), ("size", shape.size)] {
        if let Some([x, y]) = pair {
            table.inline(key, format!("[{}, {}]", number(key, x)?, number(key, y)?));
        }
    }
    table.optional_number("angle", shape.angle)?;
    Ok(table)
}

/// Returns the record of a colour rule.
///
/// # Errors
///
/// Returns the key of a shading's end that holds a number that is not finite.
fn color_rule(rule: &ColorRule) -> Result<Record, SheetWriteError> {
    let mut table = Record::default();
    table.string("rule", rule.rule.as_str());
    table.optional_string("property", rule.property.as_deref());
    table.optional_string("shade_state", rule.shade_state.as_deref());
    let shades: Vec<String> = rule
        .shades
        .iter()
        .map(|shade| {
            format!(
                "{{ property = {}, color = {} }}",
                toml_string(&shade.property),
                color_hex(&shade.color)
            )
        })
        .collect();
    table.rows("shades", shades);
    table.optional_string("scale", rule.scale.map(|scale| scale.as_str()));
    if let Some(decades) = rule.decades {
        table.inline("decades", decades.to_string());
    }
    table.optional_string("hue", rule.hue.map(|hue| hue.as_str()));
    table.optional_number("darkest", rule.darkest)?;
    table.optional_number("lightest", rule.lightest)?;
    Ok(table)
}
