//! Text forms that every kind of model file shares: line ends, TOML strings and the `[[param]]` table.

use std::ops::Range;

use serde::Deserialize;

use henad_core::helpers::check_choice_options;
use henad_core::params::{ChoiceOptions, ParamDescriptor, ParamFormat, ParamKind};

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

/// Appends the keys of `param`'s `[[param]]` table to `text`, each on a line of its own.
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
    options: Option<Vec<String>>,
    #[serde(default)]
    format: Option<FormatName>,
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
    /// Returns the descriptor the table declares.
    ///
    /// # Errors
    ///
    /// Returns the message of the first key that does not fit the parameter's kind.
    pub fn into_descriptor(self) -> Result<ParamDescriptor, String> {
        let id = self.id;
        let label = self.label.unwrap_or_else(|| id.clone());
        let unused = |key: &str, present: bool| {
            if present {
                Err(format!("parameter '{id}' of this kind must not set `{key}`"))
            } else {
                Ok(())
            }
        };
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
                        Some(_) => Err(format!("parameter '{id}' needs a whole number for `{key}`")),
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
                    .ok_or_else(|| format!("parameter '{id}' has no option '{name}' to take as its default"))?;
                ParamKind::Choice {
                    options: ChoiceOptions::new(options),
                    default,
                }
            }
        };
        let descriptor = ParamDescriptor::new(id, label, kind);
        Ok(match self.format {
            Some(FormatName::Percent) => descriptor.percent(),
            Some(FormatName::Plain) | None => descriptor,
        })
    }
}
