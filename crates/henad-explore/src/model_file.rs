//! Model files: a grid rule as one TOML file, the form in which a rule is saved, shared and opened.
//!
//! A model file starts with `format = "henad-model"`, `format_version = 1` and `kind = "gpu_grid"`, then the rule's
//! `id`, `name` and `description`. The `[grid]` table holds the default `width` and `height` and the `boundary`,
//! `torus` or `bounded`. Each `[[state]]` table holds a state's `label`, its `color` as three or four bytes, and an
//! optional `initial`, the id of the `f32` parameter that gives its initial share. Each `[[param]]` table holds a
//! parameter in the vocabulary of `--params --json`: `id`, `label`, `kind` (`f32`, `u32`, `bool` or `choice`), its
//! bounds and `default`, an `f32`'s `step` and `format`, and a choice's `options`. The `[step]` table's `wgsl` holds
//! the rule's WGSL in a string the file holds verbatim, a literal string between `'''` as a rule.
//!
//! Every table refuses an unknown key. Line ends read as `\n`, and every diagnostic is placed in the text as passed.
//!
//! The *canonical text* of a file is the file written again by [`ModelFile::to_toml`]: every key in a fixed order,
//! every value in one form, and no comment outside the step. Two files that differ in nothing else have the same
//! canonical text, and its [`ModelCode`] is the rule's code identity in a results folder.

use std::fmt;
use std::ops::Range;

use serde::Deserialize;
use serde::de::IgnoredAny;
use toml::Spanned;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::runtime_grid::{GridRule, RuleDeclaration, RuleState};
use henad_core::authoring::primitives::space::Boundary;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::check_choice_options;
use henad_core::params::{ChoiceOptions, ParamDescriptor, ParamFormat, ParamKind};
use henad_core::provenance::ModelCode;

/// Value of a model file's `format` key.
const FORMAT: &str = "henad-model";

/// Version of the model file format this build reads and writes.
const FORMAT_VERSION: u64 = 1;

/// Value of the `kind` key of a grid rule.
const KIND_GPU_GRID: &str = "gpu_grid";

/// A grid rule read from a model file, with the text it was read from.
#[derive(Clone)]
pub struct ModelFile {
    rule: GridRule,
    /// Text the rule was read from, or the canonical text of a rule that came from no file.
    text: String,
    /// Places of the rule's declarations and step in [`Self::text`] with its line ends made `\n`.
    places: Places,
    /// Map from [`Self::places`] back to [`Self::text`].
    line_ends: LineEnds,
}

/// Places of a rule's declarations and step in a model file, as byte offsets into its text with line ends made
/// `\n`.
#[derive(Debug, Clone)]
struct Places {
    /// The `id` value.
    id: Range<usize>,
    /// The `[grid]` table.
    grid: Range<usize>,
    /// Each `[[state]]` table, in order.
    states: Vec<Range<usize>>,
    /// Each `[[param]]` table, in order.
    params: Vec<Range<usize>>,
    /// Offset of the step's first character.
    step: usize,
}

impl Places {
    /// Returns the bytes of `declaration`.
    fn of(&self, declaration: RuleDeclaration) -> Option<Range<usize>> {
        match declaration {
            RuleDeclaration::Id => Some(self.id.clone()),
            RuleDeclaration::Grid => Some(self.grid.clone()),
            RuleDeclaration::State(index) => self.states.get(index).cloned(),
            RuleDeclaration::Param(index) => self.params.get(index).cloned(),
            _ => None,
        }
    }
}

/// Line ends written `\r\n` that reading a text turned into `\n`.
///
/// [`Self::original`] maps an offset in the read text back to the original text.
#[derive(Debug, Clone, Default)]
struct LineEnds {
    /// Offset in the read text of the `\n` that followed each `\r` taken out, in order.
    removed: Vec<usize>,
}

impl LineEnds {
    /// Returns `text` with each `\r\n` and lone `\r` made `\n`, and the [`LineEnds`] value that maps it back.
    fn normalize(text: &str) -> (String, Self) {
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
    fn original(&self, offset: usize) -> usize {
        offset + self.removed.partition_point(|&at| at <= offset)
    }
}

/// Prints the rule's id and the byte length of the text.
impl fmt::Debug for ModelFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelFile")
            .field("rule", &self.rule)
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

/// Reason a model file cannot be read or built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelFileError {
    /// The text is not a model file: TOML that does not parse, a key that is unknown or missing, or a value of the
    /// wrong type.
    Toml(Box<CodeDiagnostic>),
    /// The file declares another format, version or kind, or a value a model file cannot hold.
    Format(Box<CodeDiagnostic>),
    /// The rule's declarations or its step are refused. A diagnostic about one declaration or about the step is
    /// placed in the file.
    Rule(Vec<CodeDiagnostic>),
}

impl ModelFileError {
    /// Every diagnostic the error carries.
    pub fn diagnostics(&self) -> &[CodeDiagnostic] {
        match self {
            Self::Toml(diagnostic) | Self::Format(diagnostic) => std::slice::from_ref(&**diagnostic),
            Self::Rule(diagnostics) => diagnostics,
        }
    }
}

/// Writes each diagnostic on lines of its own.
impl fmt::Display for ModelFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics().iter().enumerate() {
            if index > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{diagnostic}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ModelFileError {}

impl ModelFile {
    /// Returns the model file of `rule`, its text the canonical text.
    ///
    /// The step's line ends are made `\n`.
    ///
    /// # Errors
    ///
    /// Returns [`ModelFileError::Format`] when the step holds `'''` or an ASCII control character other than a tab
    /// and a line end, since a literal string holds neither, when a parameter holds a float that is not finite, or when
    /// the canonical text does not read back, as for a choice whose options [`check_choice_options`] refuses.
    pub fn from_rule(mut rule: GridRule) -> Result<Self, ModelFileError> {
        rule.step = LineEnds::normalize(&rule.step).0;
        check_step(&rule.step).map_err(|(message, _)| format_error(message))?;
        for param in &rule.params {
            if let ParamKind::F32 {
                min,
                max,
                default,
                step,
            } = param.kind()
                && ![*min, *max, *default, step.unwrap_or(0.0)]
                    .iter()
                    .all(|value| value.is_finite())
            {
                return Err(format_error(format!(
                    "parameter '{}' holds a float that is not finite",
                    param.id()
                )));
            }
        }
        Self::parse(&canonical_text(&rule))
    }

    /// Reads the model file held in `text`.
    ///
    /// # Errors
    ///
    /// Returns [`ModelFileError::Format`] for a file of another format, version or kind, which is checked before
    /// anything else, or for a value a model file cannot hold, and [`ModelFileError::Toml`] for text that is not a
    /// model file. Each diagnostic is placed in `text` when it has a place.
    pub fn parse(text: &str) -> Result<Self, ModelFileError> {
        let (read, line_ends) = LineEnds::normalize(text);
        let place = |diagnostic: CodeDiagnostic, span: Range<usize>| {
            diagnostic.at(text, line_ends.original(span.start)..line_ends.original(span.end))
        };
        let toml_error = |error: toml::de::Error| {
            let diagnostic = CodeDiagnostic::error(error.message().trim_end().to_owned());
            ModelFileError::Toml(Box::new(match error.span() {
                Some(span) => place(diagnostic, span),
                None => diagnostic,
            }))
        };
        let placed = |message: String, span: Range<usize>| {
            ModelFileError::Format(Box::new(place(CodeDiagnostic::error(message), span)))
        };

        // The header first, so that a file of another version reads as one even when its keys are new.
        let header: HeaderTable = toml::from_str(&read).map_err(toml_error)?;
        header.check().map_err(|(message, span)| placed(message, span))?;

        let file: FileTable = toml::from_str(&read).map_err(toml_error)?;
        let step = step_offset(&read, &file.step.wgsl).ok_or_else(|| {
            placed(
                "write the step as a literal string between `'''`, which holds the WGSL as written".to_owned(),
                file.step.wgsl.span(),
            )
        })?;
        if let Err((message, at)) = check_step(file.step.wgsl.get_ref()) {
            return Err(placed(message, step + at.start..step + at.end));
        }

        let mut places = Places {
            id: file.id.span(),
            grid: file.grid.span(),
            states: Vec::with_capacity(file.states.len()),
            params: Vec::with_capacity(file.params.len()),
            step,
        };
        let mut params = Vec::with_capacity(file.params.len());
        for param in file.params {
            let span = param.span();
            params.push(
                param
                    .into_inner()
                    .into_descriptor()
                    .map_err(|message| placed(message, span.clone()))?,
            );
            places.params.push(span);
        }
        let mut states = Vec::with_capacity(file.states.len());
        for state in file.states {
            let span = state.span();
            states.push(
                state
                    .into_inner()
                    .into_state()
                    .map_err(|message| placed(message, span.clone()))?,
            );
            places.states.push(span);
        }
        let id = file.id.into_inner();
        let grid = file.grid.into_inner();
        let mut rule = GridRule::new(id.clone(), file.step.wgsl.into_inner());
        rule.name = file.name.unwrap_or(id);
        rule.description = file.description;
        rule.params = params;
        rule.states = states;
        rule.boundary = match grid.boundary {
            BoundaryName::Torus => Boundary::Torus,
            BoundaryName::Bounded => Boundary::Bounded,
        };
        rule.width = grid.width;
        rule.height = grid.height;
        Ok(Self {
            rule,
            text: text.to_owned(),
            places,
            line_ends,
        })
    }

    /// Rule the file holds.
    pub fn rule(&self) -> &GridRule {
        &self.rule
    }

    /// Returns the file's canonical text: every key in a fixed order, every value in one form, and the step verbatim.
    pub fn to_toml(&self) -> String {
        canonical_text(&self.rule)
    }

    /// Returns the code that identifies the rule, its canonical text in the format [`GridRule::CODE_FORMAT`].
    pub fn code(&self) -> ModelCode {
        ModelCode::new(GridRule::CODE_FORMAT, self.to_toml())
    }

    /// Compiles the rule and returns its entry, whose source records [`Self::code`].
    ///
    /// # Errors
    ///
    /// Returns [`ModelFileError::Rule`] with the problems [`GridRule::compile`] finds. A problem in a declaration is
    /// placed at its table or key, and a problem in the step at its line and column, in the text the rule was read
    /// from, or in the canonical text of a rule that came from no file.
    pub fn entry(&self) -> Result<ModelEntry, ModelFileError> {
        if let Err(problems) = self.rule.check_declarations() {
            return Err(ModelFileError::Rule(
                problems
                    .into_iter()
                    .map(|(declaration, problem)| {
                        match declaration.and_then(|declaration| self.places.of(declaration)) {
                            Some(span) => self.place(problem, span),
                            None => problem,
                        }
                    })
                    .collect(),
            ));
        }
        let step = self.places.step;
        let compiled = self.rule.compile().map_err(|problems| {
            ModelFileError::Rule(
                problems
                    .into_iter()
                    .map(|mut problem| match problem.span.take() {
                        Some(span) => self.place(problem, step + span.start..step + span.end),
                        None => problem,
                    })
                    .collect(),
            )
        })?;
        Ok(compiled.entry(self.code()))
    }

    /// Returns `problem` placed at `span`, a byte range of the text with its line ends made `\n`.
    fn place(&self, problem: CodeDiagnostic, span: Range<usize>) -> CodeDiagnostic {
        problem.at(
            &self.text,
            self.line_ends.original(span.start)..self.line_ends.original(span.end),
        )
    }
}

/// Checks that a literal string holds `step`.
///
/// # Errors
///
/// Returns the message and the bytes in `step` of the first `'''`, which ends a literal string, or of an ASCII control
/// character other than a tab and a line end, which a literal string cannot hold.
fn check_step(step: &str) -> Result<(), (String, Range<usize>)> {
    if let Some(at) = step.find("'''") {
        return Err((
            "the step holds `'''`, which ends a literal string in a model file".to_owned(),
            at..at + 3,
        ));
    }
    match step
        .char_indices()
        .find(|&(_, c)| matches!(c, '\u{0}'..='\u{8}' | '\u{B}'..='\u{1F}' | '\u{7F}'))
    {
        Some((at, c)) => Err((
            format!(
                "the step holds the control character U+{:04X}, which a model file cannot hold",
                u32::from(c)
            ),
            at..at + c.len_utf8(),
        )),
        None => Ok(()),
    }
}

/// Returns a format error with `message` and no place.
fn format_error(message: impl Into<String>) -> ModelFileError {
    ModelFileError::Format(Box::new(CodeDiagnostic::error(message)))
}

/// Returns the byte offset in `text` of the first character of `value`, a string whose literal `text` holds, when the
/// literal holds the string verbatim.
fn step_offset(text: &str, value: &Spanned<String>) -> Option<usize> {
    let span = value.span();
    let literal = text.get(span.clone())?;
    let delimiter = ["'''", "\"\"\"", "'", "\""]
        .into_iter()
        .find(|delimiter| literal.starts_with(delimiter))?;
    let mut start = span.start + delimiter.len();
    // TOML drops a line end right after the opening delimiter of a multi-line string.
    if delimiter.len() == 3 && text[start..].starts_with('\n') {
        start += 1;
    }
    text.get(start..start + value.get_ref().len())
        .is_some_and(|written| written == value.get_ref())
        .then_some(start)
}

/// Returns the canonical text of `rule`.
fn canonical_text(rule: &GridRule) -> String {
    let mut text = String::new();
    let mut line = |key: &str, value: String| {
        text.push_str(key);
        text.push_str(" = ");
        text.push_str(&value);
        text.push('\n');
    };
    line("format", string(FORMAT));
    line("format_version", FORMAT_VERSION.to_string());
    line("kind", string(KIND_GPU_GRID));
    line("id", string(&rule.id));
    line("name", string(&rule.name));
    line("description", string(&rule.description));
    text.push_str("\n[grid]\n");
    text.push_str(&format!("width = {}\nheight = {}\n", rule.width, rule.height));
    let boundary = match rule.boundary {
        Boundary::Torus => "torus",
        Boundary::Bounded => "bounded",
    };
    text.push_str(&format!("boundary = {}\n", string(boundary)));
    for state in &rule.states {
        text.push_str("\n[[state]]\n");
        text.push_str(&format!("label = {}\n", string(&state.label)));
        let [red, green, blue, alpha] = state.color;
        text.push_str(&format!("color = [{red}, {green}, {blue}, {alpha}]\n"));
        if let Some(initial) = &state.initial {
            text.push_str(&format!("initial = {}\n", string(initial)));
        }
    }
    for param in &rule.params {
        text.push_str("\n[[param]]\n");
        text.push_str(&format!("id = {}\n", string(param.id())));
        text.push_str(&format!("label = {}\n", string(param.label())));
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
                let names: Vec<String> = options.iter().map(string).collect();
                text.push_str(&format!("kind = \"choice\"\noptions = [{}]\n", names.join(", ")));
                text.push_str(&format!(
                    "default = {}\n",
                    string(options.get(*default).unwrap_or_default())
                ));
            }
        }
        if param.format() == ParamFormat::Percent {
            text.push_str("format = \"percent\"\n");
        }
    }
    text.push_str("\n[step]\nwgsl = '''\n");
    text.push_str(&rule.step);
    text.push_str("'''\n");
    text
}

/// Returns `value` as a TOML basic string.
fn string(value: &str) -> String {
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

/// The header keys of a model file, read before the rest and ignoring every other key.
#[derive(Deserialize)]
struct HeaderTable {
    #[serde(default)]
    format: Option<Spanned<String>>,
    #[serde(default)]
    format_version: Option<Spanned<u64>>,
    #[serde(default)]
    kind: Option<Spanned<String>>,
}

impl HeaderTable {
    /// Checks the header's values, and returns the message and the place of the first that this build cannot read.
    fn check(self) -> Result<(), (String, Range<usize>)> {
        if let Some(format) = self.format
            && format.get_ref() != FORMAT
        {
            let message = format!(
                "the file's format is '{}', and a model file's is '{FORMAT}'",
                format.get_ref()
            );
            return Err((message, format.span()));
        }
        if let Some(version) = self.format_version
            && *version.get_ref() != FORMAT_VERSION
        {
            let message = format!(
                "the file is model file version {}, and this build reads version {FORMAT_VERSION}",
                version.get_ref()
            );
            return Err((message, version.span()));
        }
        if let Some(kind) = self.kind
            && kind.get_ref() != KIND_GPU_GRID
        {
            let message = format!(
                "the file holds a model of kind '{}', and this build reads '{KIND_GPU_GRID}'",
                kind.get_ref()
            );
            return Err((message, kind.span()));
        }
        Ok(())
    }
}

/// A model file as TOML holds it. [`HeaderTable`] checks the header's values.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTable {
    #[serde(rename = "format")]
    _format: IgnoredAny,
    #[serde(rename = "format_version")]
    _format_version: IgnoredAny,
    #[serde(rename = "kind")]
    _kind: IgnoredAny,
    id: Spanned<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: String,
    grid: Spanned<GridTable>,
    #[serde(default, rename = "state")]
    states: Vec<Spanned<StateTable>>,
    #[serde(default, rename = "param")]
    params: Vec<Spanned<ParamTable>>,
    step: StepTable,
}

/// The `[grid]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GridTable {
    width: u32,
    height: u32,
    #[serde(default)]
    boundary: BoundaryName,
}

/// Value of the `boundary` key.
#[derive(Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum BoundaryName {
    #[default]
    Torus,
    Bounded,
}

/// A `[[state]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateTable {
    label: String,
    color: Vec<u8>,
    #[serde(default)]
    initial: Option<String>,
}

impl StateTable {
    /// Returns the state the table declares, or the message of a colour that is not 3 or 4 bytes.
    fn into_state(self) -> Result<RuleState, String> {
        let Self { label, color, initial } = self;
        let color = match color[..] {
            [red, green, blue] => [red, green, blue, 255],
            [red, green, blue, alpha] => [red, green, blue, alpha],
            _ => {
                return Err(format!(
                    "state '{label}' has a colour of {} bytes, and a colour has 3 or 4",
                    color.len()
                ));
            }
        };
        let mut state = RuleState::new(label, color);
        state.initial = initial;
        Ok(state)
    }
}

/// A `[[param]]` table, in the vocabulary of `--params --json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamTable {
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
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ParamKindName {
    F32,
    U32,
    Bool,
    Choice,
}

/// Value of a parameter's `format` key.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum FormatName {
    Plain,
    Percent,
}

impl ParamTable {
    /// Returns the descriptor the table declares, or the message of the first key that does not fit its kind.
    fn into_descriptor(self) -> Result<ParamDescriptor, String> {
        let id = self.id;
        let label = self.label.unwrap_or_else(|| id.clone());
        let unused = |key: &str, present: bool| {
            if present {
                Err(format!("parameter '{id}' of this kind takes no `{key}`"))
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
                    return Err(format!("parameter '{id}' holds a float that is not finite"));
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

/// The `[step]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepTable {
    wgsl: Spanned<String>,
}
