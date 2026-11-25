//! Sheets: the author document of a model, a model file of kind `sheet` that a researcher builds without nodes.
//!
//! A *sheet* holds the model in the order a reader meets it: its purpose, its world and time scale, the properties of
//! each kind of thing, its parameters, measures, colours and buttons. Each kind of thing has a *card*, the lines it
//! runs every tick, and a button can run a card of its own once.
//!
//! - A *line* is one statement of a card, such as "Become Alive from Dead, when Alive neighbours is 3". Each line has
//!   an id of the form `l<number>`, unique across the file, and lines refer to each other by these ids.
//! - A *slot* is a place in a line that takes a value, such as the condition of a Become line. A slot the author has
//!   not filled is *blank*, and the file leaves its key out.
//! - A *percent* is a number typed with "%" and kept as typed: `{ percent = 30 }` stands for 0.3 wherever a number
//!   can stand.
//! - A *chip* names one thing by id, such as a parameter, the result of a line or a property of mine:
//!   `{ param = "infection_rate" }`.
//! - A *phrase* is an unnamed line inside a slot, a table with a `kind`, such as the comparison "when Alive neighbours
//!   is 3".
//! - A *formula* is the text of a Calculate line or phrase, whose words are ids:
//!   `"trail_falloff * l2.amount + my.reward_held"`.
//!
//! [`ParsedSheet::parse`] reads a sheet into a [`document::SheetDocument`] and records the place of every table and
//! key in [`SheetPlaces`]. The reader checks the header and the `[sheet] version` first and refuses an unknown key in
//! every table. It does not check what the lines mean. [`check`] does: references, types, ids and the lines that
//! cannot run yet, and [`relations`] holds the order of a card's lines and the "otherwise" rule.
//! [`document::SheetDocument::to_text`] writes the canonical text of a sheet, with the sentence of each line from
//! [`sentence`] in a comment above it, and reading that text returns the same document. [`SheetFile::parse`] reads,
//! checks and lowers a sheet to the graph it runs as, whose code identity is the sheet's, and whose entries record
//! the sheet's canonical text as their author text in [`SHEET_CODE_FORMAT`]. [`LineMap`] names the
//! line or part of the sheet each node of that graph comes from. [`SheetFile::place_graph_diagnostic`] places a finding
//! about that graph at the table of the sheet its node comes from. [`convert()`] copies that graph to a node graph file
//! of its own, with frames naming the line each node came from, and [`changes_since_conversion`] lists what an edit of
//! the copy changed. [`starters()`] returns the sheets a new model begins from, [`edit`] holds the commands every edit
//! of the sheet editor runs, and [`rates`] the chance per tick a rate gives and the note that states it.
//!
//! ```
//! use henad_graph::sheet::ParsedSheet;
//!
//! let text = r#"
//! format = "henad-model"
//! format_version = 1
//! kind = "sheet"
//!
//! [sheet]
//! version = 1
//! id = "blank"
//! name = "Untitled model"
//! next_line = 1
//!
//! [world]
//! made_of = "cells"
//! width = 100
//! height = 100
//! edges = "wrap"
//! "#;
//! let sheet = ParsedSheet::parse(text).expect("the sheet reads");
//! assert_eq!(sheet.document().sheet.name, "Untitled model");
//! let written = sheet.document().to_text().expect("the sheet writes");
//! assert_eq!(ParsedSheet::parse(&written).expect("the text reads").document(), sheet.document());
//! ```

use std::fmt;

use henad_core::diagnostic::CodeDiagnostic;

mod changes;
pub mod check;
mod convert;
pub mod document;
pub mod edit;
mod file;
pub mod limits;
mod lower;
pub mod rates;
mod reader;
pub mod relations;
pub mod sentence;
mod starters;
mod writer;

pub use changes::{ChangeGroup, ChangesError, changes_since_conversion};
pub use convert::{ConvertError, ConvertOptions, ConvertRefusal, convert};
pub use file::SheetFile;
pub use lower::table::{FireLines, LineMap, NodeOrigin, NodeOwner};
pub use reader::{ParsedSheet, SheetPlaces, TablePlaces};
pub use starters::starters;
pub use writer::{SheetWriteError, format_number};

/// Value of the `kind` key of a sheet.
pub const KIND_SHEET: &str = "sheet";

/// Newest version of the author format this build reads, the `version` key of the `[sheet]` table.
pub const SHEET_VERSION: u32 = 1;

/// Format of a sheet's canonical text as a model's author text records it, in
/// [`ModelSource::author`](henad_core::provenance::ModelSource::author) and in a results folder.
pub const SHEET_CODE_FORMAT: &str = "henad-sheet/1";

/// Reason a sheet cannot be read, checked or lowered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetFileError {
    /// The text is not a sheet: TOML that does not parse, a key that is unknown or missing, or a value of the wrong
    /// type.
    Toml(Box<CodeDiagnostic>),
    /// The file declares another format, version, kind or sheet version, or holds a value a sheet cannot hold.
    Format(Box<CodeDiagnostic>),
    /// The sheet reads, and its check or its lowering finds errors, each placed at its table.
    Invalid(SheetErrors),
}

impl SheetFileError {
    /// First diagnostic the error carries.
    pub fn diagnostic(&self) -> &CodeDiagnostic {
        match self {
            Self::Toml(diagnostic) | Self::Format(diagnostic) => diagnostic,
            Self::Invalid(errors) => errors.first(),
        }
    }

    /// Every diagnostic the error carries, at least one.
    pub fn diagnostics(&self) -> &[CodeDiagnostic] {
        match self {
            Self::Toml(diagnostic) | Self::Format(diagnostic) => std::slice::from_ref(diagnostic),
            Self::Invalid(errors) => errors.as_slice(),
        }
    }
}

/// Errors of a sheet's check or lowering, at least one, in the order they were found.
///
/// The list is never empty. Only [`SheetErrors::new`] builds one, from a first error and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetErrors {
    /// The errors, never empty.
    diagnostics: Vec<CodeDiagnostic>,
}

impl SheetErrors {
    /// Creates the list of `first` followed by `rest`.
    pub fn new(first: CodeDiagnostic, rest: impl IntoIterator<Item = CodeDiagnostic>) -> Self {
        let mut diagnostics = vec![first];
        diagnostics.extend(rest);
        Self { diagnostics }
    }

    /// First error of the list.
    pub fn first(&self) -> &CodeDiagnostic {
        // `new` puts one error in the list, and nothing removes one.
        &self.diagnostics[0]
    }

    /// Every error of the list, at least one.
    pub fn as_slice(&self) -> &[CodeDiagnostic] {
        &self.diagnostics
    }
}

/// Writes every diagnostic, one per line.
impl fmt::Display for SheetFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics().iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{diagnostic}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SheetFileError {}
