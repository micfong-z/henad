//! Text forms that every kind of model file shares: the header, line ends, TOML strings, arrays and colours, and the
//! `[[param]]` table.
//!
//! The *header* is the three keys every model file opens with, `format`, `format_version` and `kind`. A reader checks
//! them before the rest of the file, with [`ModelHeader::check`], so a file of another version or kind reads as one
//! even when it holds keys this build does not know.

use std::ops::Range;

use serde::Deserialize;
use toml::Spanned;

use crate::file::{FORMAT, FORMAT_VERSION};
use crate::value::Color;

use henad_core::helpers::check_choice_options;
use henad_core::metadata::TimeUnit;
use henad_core::params::{ChoiceOptions, ParamDescriptor, ParamFormat, ParamKind};

/// Header keys of a model file, and the version key of each kind's own table, read ignoring every other key.
///
/// The version keys are `[graph] library` and `[sheet] version`. A reader checks them with the header, so a newer file
/// is refused on its version before an unknown key is.
#[derive(Debug, Deserialize)]
pub struct ModelHeader {
    #[serde(default)]
    format: Option<Spanned<String>>,
    #[serde(default)]
    format_version: Option<Spanned<i64>>,
    #[serde(default)]
    kind: Option<Spanned<String>>,
    #[serde(default)]
    graph: Option<HeaderGraph>,
    #[serde(default)]
    sheet: Option<HeaderSheet>,
}

/// The `library` key of a graph file's `[graph]` table, read with the header.
#[derive(Debug, Deserialize)]
struct HeaderGraph {
    #[serde(default)]
    library: Option<Spanned<i64>>,
}

/// The `version` key of a sheet's `[sheet]` table, read with the header.
#[derive(Debug, Deserialize)]
struct HeaderSheet {
    #[serde(default)]
    version: Option<Spanned<i64>>,
}

/// Value in a model file's header that this build cannot read, with its bytes in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HeaderProblem {
    /// Message naming the value and the values this build reads.
    pub message: String,
    /// Bytes of the value in the text the header was read from.
    pub span: Range<usize>,
}

impl ModelHeader {
    /// Reads the header of the model file held in `text`.
    ///
    /// # Errors
    ///
    /// Returns the TOML crate's error for text that is not TOML, or for a header key of the wrong type.
    pub fn read(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    /// Value of the `kind` key, `None` when the file writes none.
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_ref().map(|kind| kind.get_ref().as_str())
    }

    /// Checks `format`, `format_version` and `kind` against the values this build reads, where `kinds` lists the kinds
    /// the caller reads.
    ///
    /// A key the file does not write passes, and the reader of the whole file then reports it missing.
    ///
    /// # Errors
    ///
    /// Returns the first value that this build cannot read, in the order of the keys above.
    pub fn check(&self, kinds: &[&str]) -> Result<(), HeaderProblem> {
        if let Some(format) = &self.format
            && format.get_ref() != FORMAT
        {
            return Err(HeaderProblem {
                message: format!("the file's format is '{}', expected '{FORMAT}'", format.get_ref()),
                span: format.span(),
            });
        }
        if let Some(version) = &self.format_version
            && *version.get_ref() != FORMAT_VERSION
        {
            return Err(HeaderProblem {
                message: format!(
                    "the file is model file version {}, expected version {FORMAT_VERSION}",
                    version.get_ref()
                ),
                span: version.span(),
            });
        }
        if let Some(kind) = &self.kind
            && !kinds.contains(&kind.get_ref().as_str())
        {
            return Err(HeaderProblem {
                message: format!(
                    "the file contains a model of kind '{}', expected {}",
                    kind.get_ref(),
                    kind_list(kinds)
                ),
                span: kind.span(),
            });
        }
        Ok(())
    }

    /// The `library` key of the `[graph]` table, `None` when the file writes none.
    pub(crate) fn graph_library(&self) -> Option<&Spanned<i64>> {
        self.graph.as_ref()?.library.as_ref()
    }

    /// The `version` key of the `[sheet]` table, `None` when the file writes none.
    pub(crate) fn sheet_version(&self) -> Option<&Spanned<i64>> {
        self.sheet.as_ref()?.version.as_ref()
    }
}

/// Returns `kinds` quoted and joined by [`or_list`], such as `'gpu_grid', 'graph' or 'sheet'`.
fn kind_list(kinds: &[&str]) -> String {
    if kinds.is_empty() {
        return "no kind".to_owned();
    }
    let quoted: Vec<String> = kinds.iter().map(|kind| format!("'{kind}'")).collect();
    let quoted: Vec<&str> = quoted.iter().map(String::as_str).collect();
    or_list(&quoted)
}

/// Returns `items` joined as a list ending in "or", such as `a, b or c`, and the one item alone.
pub(crate) fn or_list(items: &[&str]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// Returns `count` and `noun`, the noun in the plural unless the count is 1, such as `1 byte` or `3 bytes`.
///
/// `noun` is a singular noun whose plural adds an `s`.
pub(crate) fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// The `kind` key of a model file, read ignoring every other key and its type.
#[derive(Deserialize)]
struct HeaderKind {
    #[serde(default)]
    kind: Option<String>,
}

/// Returns the `kind` of the model file held in `text`, `None` when the text is not TOML or writes no kind as a string.
///
/// The kind reads even from a file whose reader refuses it, a header key of the wrong type included.
pub fn read_header(text: &str) -> Option<String> {
    toml::from_str::<HeaderKind>(text).ok()?.kind
}

/// Line ends written `\r\n` that reading a text turned into `\n`.
///
/// [`Self::original`] maps an offset in the read text back to the original text.
#[derive(Debug, Clone, Default)]
pub struct LineEnds {
    /// Offset in the read text of the `\n` that followed each `\r` taken out, in order.
    removed: Vec<usize>,
}

impl LineEnds {
    /// Returns `text` with each `\r\n` and lone `\r` made `\n`, and the [`LineEnds`] value that maps it back.
    pub fn normalize(text: &str) -> (String, Self) {
        let mut read = String::with_capacity(text.len());
        let mut removed = Vec::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\r' {
                if chars.peek() == Some(&'\n') {
                    removed.push(read.len());
                } else {
                    read.push('\n');
                }
            } else {
                read.push(c);
            }
        }
        (read, Self { removed })
    }

    /// Returns the offset in the original text of offset `offset` of the read text.
    pub fn original(&self, offset: usize) -> usize {
        offset + self.removed.partition_point(|&at| at <= offset)
    }

    /// Returns the bytes in the original text of the bytes `span` of the read text.
    ///
    /// The range ends after the last byte of `span`, which leaves out the `\r` before a `\n` that follows it.
    pub fn original_range(&self, span: Range<usize>) -> Range<usize> {
        let start = self.original(span.start);
        if span.is_empty() {
            return start..start;
        }
        start..self.original(span.end - 1) + 1
    }
}

/// Returns `key` as a bare key when TOML reads it bare, and as a basic string otherwise.
pub(crate) fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if bare { key.to_owned() } else { toml_string(key) }
}

/// Returns `value` as a TOML basic string.
pub fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Appends the line `name = value` to `text`.
pub(crate) fn push_key(text: &mut String, name: &str, value: &str) {
    text.push_str(name);
    text.push_str(" = ");
    text.push_str(value);
    text.push('\n');
}

/// Returns `items` as an inline array of TOML basic strings on one line.
pub(crate) fn string_array(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| toml_string(item)).collect();
    format!("[{}]", items.join(", "))
}

/// Returns an array with one element per line and a trailing comma, or `[]` with no element.
///
/// Each element of `items` is already written as TOML.
pub(crate) fn multiline_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let mut text = String::from("[\n");
    for item in items {
        text.push_str("  ");
        text.push_str(item);
        text.push_str(",\n");
    }
    text.push(']');
    text
}

/// Returns `color` as the four bytes of a graph file, such as `[255, 0, 0, 255]`.
pub(crate) fn color_array(color: &Color) -> String {
    let [red, green, blue, alpha] = color;
    format!("[{red}, {green}, {blue}, {alpha}]")
}

/// Returns `color` as a sheet writes it: `"#RRGGBB"` in upper case, or `"#RRGGBBAA"` when alpha is not 255.
pub(crate) fn color_hex(color: &Color) -> String {
    let [red, green, blue, alpha] = color;
    if *alpha == u8::MAX {
        format!("\"#{red:02X}{green:02X}{blue:02X}\"")
    } else {
        format!("\"#{red:02X}{green:02X}{blue:02X}{alpha:02X}\"")
    }
}

/// Appends the keys of `param`'s `[[param]]` table to `text`, each on a line of its own.
///
/// `hidden = true` is written only for a hidden parameter, and a shown one writes no `hidden` key. `integer = true` is
/// written only for an `f32` marked [`ParamDescriptor::integer`]. A graph file's tick length adds its `tick_unit` after
/// these keys.
pub fn write_param(text: &mut String, param: &ParamDescriptor) {
    text.push_str(&format!("id = {}\n", toml_string(param.id())));
    text.push_str(&format!("label = {}\n", toml_string(param.label())));
    match param.kind() {
        ParamKind::F32 {
            min,
            max,
            default,
            step,
        } => {
            text.push_str("kind = \"f32\"\n");
            text.push_str(&format!("min = {min:?}\nmax = {max:?}\ndefault = {default:?}\n"));
            if let Some(step) = step {
                text.push_str(&format!("step = {step:?}\n"));
            }
            if param.is_integer() {
                text.push_str("integer = true\n");
            }
        }
        ParamKind::U32 { min, max, default } => {
            text.push_str(&format!(
                "kind = \"u32\"\nmin = {min}\nmax = {max}\ndefault = {default}\n"
            ));
        }
        ParamKind::Bool { default } => text.push_str(&format!("kind = \"bool\"\ndefault = {default}\n")),
        ParamKind::Choice { options, default } => {
            let names: Vec<String> = options.iter().map(toml_string).collect();
            text.push_str(&format!("kind = \"choice\"\noptions = [{}]\n", names.join(", ")));
            text.push_str(&format!(
                "default = {}\n",
                toml_string(options.get(*default).unwrap_or_default())
            ));
        }
    }
    if param.format() == ParamFormat::Percent {
        text.push_str("format = \"percent\"\n");
    }
    if param.is_hidden() {
        text.push_str("hidden = true\n");
    }
}

/// A `[[param]]` table, in the vocabulary of `--params --json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamTable {
    id: String,
    #[serde(default)]
    label: Option<String>,
    kind: ParamKindName,
    #[serde(default)]
    min: Option<toml::Value>,
    #[serde(default)]
    max: Option<toml::Value>,
    default: toml::Value,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    integer: Option<bool>,
    #[serde(default)]
    options: Option<Vec<String>>,
    #[serde(default)]
    format: Option<FormatName>,
    #[serde(default)]
    hidden: Option<bool>,
    #[serde(default)]
    tick_unit: Option<String>,
}

/// Value of a parameter's `kind` key.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ParamKindName {
    F32,
    U32,
    Bool,
    Choice,
}

/// Value of a parameter's `format` key.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum FormatName {
    Plain,
    Percent,
}

impl ParamTable {
    /// Returns the descriptor the table declares, refusing the `hidden` and `tick_unit` keys that only a graph file
    /// reads.
    ///
    /// # Errors
    ///
    /// Returns the message of the first key that does not fit the parameter's kind, or of a `hidden` or `tick_unit`
    /// key.
    pub fn into_descriptor(self) -> Result<ParamDescriptor, String> {
        for (key, present) in [
            ("hidden", self.hidden.is_some()),
            ("tick_unit", self.tick_unit.is_some()),
        ] {
            if present {
                return Err(format!(
                    "parameter '{}' cannot set `{key}` outside a graph file",
                    self.id
                ));
            }
        }
        self.into_kind_descriptor()
    }

    /// Returns the descriptor the table declares, hidden when the table sets `hidden = true`, and the unit its
    /// `tick_unit` key names.
    ///
    /// A graph file reads its parameters this way. The checker holds a parameter with a unit to the rules of a tick
    /// length.
    ///
    /// # Errors
    ///
    /// Returns the message of the first key that does not fit the parameter's kind, or of a `tick_unit` that names no
    /// [`TimeUnit`].
    pub fn into_graph_param(mut self) -> Result<(ParamDescriptor, Option<TimeUnit>), String> {
        let unit = match self.tick_unit.take() {
            Some(name) => Some(TimeUnit::parse(&name).ok_or_else(|| {
                let names: Vec<&str> = TimeUnit::ALL.iter().map(|unit| unit.as_str()).collect();
                format!(
                    "parameter '{}' sets `tick_unit` to '{name}', expected {}",
                    self.id,
                    or_list(&names)
                )
            })?),
            None => None,
        };
        let hidden = self.hidden.take().unwrap_or(false);
        let descriptor = self.into_kind_descriptor()?;
        Ok((if hidden { descriptor.hidden() } else { descriptor }, unit))
    }

    /// Returns the descriptor the table declares from every key but `hidden` and `tick_unit`.
    /// Checks that the `min`, `max`, `default` and `step` of an `f32` parameter that sets `integer = true` are whole
    /// numbers. A key left out, or a value that is not a number, is left to the reader of that key.
    ///
    /// # Errors
    ///
    /// Returns the message naming the parameter when a value has a fractional part.
    fn check_whole(&self) -> Result<(), String> {
        if self.kind != ParamKindName::F32 || self.integer != Some(true) {
            return Ok(());
        }
        let fractional = |value: &toml::Value| matches!(value, toml::Value::Float(number) if number.fract() != 0.0);
        let values = [self.min.as_ref(), self.max.as_ref(), Some(&self.default)];
        if values.into_iter().flatten().any(fractional) || self.step.is_some_and(|step| step.fract() != 0.0) {
            return Err(format!(
                "parameter '{}' sets `integer`, expected integers for `min`, `max`, `default` and `step`",
                self.id
            ));
        }
        Ok(())
    }

    fn into_kind_descriptor(self) -> Result<ParamDescriptor, String> {
        self.check_whole()?;
        let id = self.id;
        let label = self.label.unwrap_or_else(|| id.clone());
        let unused = |key: &str, present: bool| {
            if present {
                Err(format!("parameter '{id}' of this kind must not set `{key}`"))
            } else {
                Ok(())
            }
        };
        unused("integer", self.integer.is_some() && self.kind != ParamKindName::F32)?;
        let kind = match self.kind {
            ParamKindName::F32 => {
                unused("options", self.options.is_some())?;
                let float = |key: &str, value: Option<&toml::Value>| -> Result<f32, String> {
                    match value {
                        Some(toml::Value::Float(value)) => Ok(*value as f32),
                        Some(toml::Value::Integer(value)) => Ok(*value as f32),
                        Some(_) => Err(format!("parameter '{id}' needs a number for `{key}`")),
                        None => Err(format!("parameter '{id}' needs `{key}`")),
                    }
                };
                let kind = ParamKind::F32 {
                    min: float("min", self.min.as_ref())?,
                    max: float("max", self.max.as_ref())?,
                    default: float("default", Some(&self.default))?,
                    step: self.step.map(|step| step as f32),
                };
                if let ParamKind::F32 {
                    min,
                    max,
                    default,
                    step,
                } = &kind
                    && ![*min, *max, *default, step.unwrap_or(0.0)]
                        .iter()
                        .all(|value| value.is_finite())
                {
                    return Err(format!("parameter '{id}' contains a float that is not finite"));
                }
                kind
            }
            ParamKindName::U32 => {
                unused("options", self.options.is_some())?;
                unused("step", self.step.is_some())?;
                let whole = |key: &str, value: Option<&toml::Value>| -> Result<u32, String> {
                    match value {
                        Some(toml::Value::Integer(value)) => u32::try_from(*value)
                            .map_err(|error| format!("parameter '{id}' needs `{key}` from 0 to {}: {error}", u32::MAX)),
                        Some(_) => Err(format!("parameter '{id}' needs an integer for `{key}`")),
                        None => Err(format!("parameter '{id}' needs `{key}`")),
                    }
                };
                ParamKind::U32 {
                    min: whole("min", self.min.as_ref())?,
                    max: whole("max", self.max.as_ref())?,
                    default: whole("default", Some(&self.default))?,
                }
            }
            ParamKindName::Bool => {
                unused("min", self.min.is_some())?;
                unused("max", self.max.is_some())?;
                unused("step", self.step.is_some())?;
                unused("options", self.options.is_some())?;
                match self.default {
                    toml::Value::Boolean(default) => ParamKind::Bool { default },
                    _ => return Err(format!("parameter '{id}' needs `true` or `false` for `default`")),
                }
            }
            ParamKindName::Choice => {
                unused("min", self.min.is_some())?;
                unused("max", self.max.is_some())?;
                unused("step", self.step.is_some())?;
                let options = self
                    .options
                    .ok_or_else(|| format!("parameter '{id}' needs `options`"))?;
                check_choice_options(&options).map_err(|reason| format!("in parameter '{id}', {reason}"))?;
                let toml::Value::String(name) = &self.default else {
                    return Err(format!("parameter '{id}' needs an option's name for `default`"));
                };
                let default = options
                    .iter()
                    .position(|option| option == name)
                    .ok_or_else(|| format!("parameter '{id}' has no option '{name}' for its default"))?;
                ParamKind::Choice {
                    options: ChoiceOptions::new(options),
                    default,
                }
            }
        };
        let mut descriptor = ParamDescriptor::new(id, label, kind);
        if self.integer == Some(true) {
            descriptor = descriptor.integer();
        }
        Ok(match self.format {
            Some(FormatName::Percent) => descriptor.percent(),
            Some(FormatName::Plain) | None => descriptor,
        })
    }
}
