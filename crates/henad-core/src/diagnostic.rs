//! Diagnostics about code a program reads while it runs, such as the WGSL of a grid rule or a model file.
//!
//! A [`CodeDiagnostic`] names its file and its place in the file twice: as a byte span, which an editor marks, and as
//! a line and a column, which a person reads. Lines and columns count from 1, and a column counts characters.

use std::fmt;
use std::ops::Range;

/// Severity of a [`CodeDiagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagnosticSeverity {
    /// The code cannot be used.
    Error,
}

impl DiagnosticSeverity {
    /// Returns the word a diagnostic prints for the severity, `error`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
        }
    }
}

/// One finding about a piece of code, with its place in the file when it has one.
///
/// Build one with [`Self::error`] and the `with_` methods.
///
/// ```
/// use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};
///
/// let diagnostic = CodeDiagnostic::error("loops are not allowed in a grid rule")
///     .with_file("life.toml")
///     .with_span(120..124, 7, 5)
///     .with_note("a step without loops does bounded work");
/// assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
/// assert_eq!(
///     diagnostic.to_string(),
///     "life.toml:7:5: error: loops are not allowed in a grid rule\n  note: a step without loops does bounded work"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CodeDiagnostic {
    /// Severity of the finding.
    pub severity: DiagnosticSeverity,
    /// One-line description of the finding.
    pub message: String,
    /// File the finding is in, `None` for text that has no file, such as a rule held in memory.
    pub file: Option<String>,
    /// Bytes of the file the finding is about, `None` when it has no place in the file.
    pub span: Option<Range<usize>>,
    /// Line of the start of [`Self::span`], counted from 1.
    pub line: Option<u32>,
    /// Column of the start of [`Self::span`] in characters, counted from 1.
    pub column: Option<u32>,
    /// Further lines that explain the finding, in the order they print.
    pub notes: Vec<String>,
}

impl CodeDiagnostic {
    /// Returns an error with `message`, no file and no place.
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Error, message.into())
    }

    fn new(severity: DiagnosticSeverity, message: String) -> Self {
        Self {
            severity,
            message,
            file: None,
            span: None,
            line: None,
            column: None,
            notes: Vec::new(),
        }
    }

    /// Returns the diagnostic with `file` as its file.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Returns the diagnostic placed at `span`, which starts on `line` at `column`.
    ///
    /// `span` is in bytes of the file. `line` and `column` count from 1, and `column` counts characters.
    pub fn with_span(mut self, span: Range<usize>, line: u32, column: u32) -> Self {
        self.span = Some(span);
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    /// Returns the diagnostic placed at `span` of `text`, with the line and column of the span's start read from
    /// `text`.
    ///
    /// A line ends at `\n`, and a `\r` counts as a character of its line. A span past the end of `text` is cut at the
    /// end, and a start inside a character moves to the character's start.
    pub fn at(self, text: &str, span: Range<usize>) -> Self {
        let mut start = span.start.min(text.len());
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let end = span.end.clamp(start, text.len());
        let bytes = text.as_bytes();
        let mut line = 1u32;
        let mut line_start = 0;
        for (index, &byte) in bytes[..start].iter().enumerate() {
            if byte == b'\n' {
                line = line.saturating_add(1);
                line_start = index + 1;
            }
        }
        let column = u32::try_from(text[line_start..start].chars().count() + 1).unwrap_or(u32::MAX);
        self.with_span(start..end, line, column)
    }

    /// Returns the diagnostic with `note` added after its other notes.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// Writes `file:line:column: severity: message`, leaving out the parts the diagnostic lacks, then each note on a line
/// of its own.
impl fmt::Display for CodeDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}:")?;
        }
        if let Some(line) = self.line {
            write!(f, "{line}:")?;
            if let Some(column) = self.column {
                write!(f, "{column}:")?;
            }
        }
        if self.file.is_some() || self.line.is_some() {
            f.write_str(" ")?;
        }
        write!(f, "{}: {}", self.severity.as_str(), self.message)?;
        for note in &self.notes {
            write!(f, "\n  note: {note}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CodeDiagnostic;

    #[test]
    fn a_place_reads_its_line_and_character_column_from_the_text() {
        let text = "one\r\ntwo\rthé x\n";
        let placed = CodeDiagnostic::error("bad").at(text, 14..15);
        assert_eq!((placed.line, placed.column), (Some(2), Some(9)));
        assert_eq!(placed.span, Some(14..15));
        let past = CodeDiagnostic::error("bad").at(text, 40..50);
        assert_eq!((past.line, past.span), (Some(3), Some(text.len()..text.len())));
    }

    #[test]
    fn a_diagnostic_prints_only_the_parts_it_has() {
        assert_eq!(CodeDiagnostic::error("bad").to_string(), "error: bad");
        assert_eq!(
            CodeDiagnostic::error("odd").with_file("rule.wgsl").to_string(),
            "rule.wgsl: error: odd"
        );
        assert_eq!(
            CodeDiagnostic::error("bad").with_span(3..4, 2, 1).to_string(),
            "2:1: error: bad"
        );
    }
}
