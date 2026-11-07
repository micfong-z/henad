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
//! every table. It does not check what the lines mean.
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
//! ```

use std::fmt;

use henad_core::diagnostic::CodeDiagnostic;

pub mod document;
pub mod limits;
mod reader;

pub use reader::{ParsedSheet, SheetPlaces, TablePlaces};

/// Value of the `kind` key of a sheet.
pub const KIND_SHEET: &str = "sheet";

/// Newest version of the author format this build reads, the `version` key of the `[sheet]` table.
pub const SHEET_VERSION: u32 = 1;

/// Reason a sheet cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetFileError {
    /// The text is not a sheet: TOML that does not parse, a key that is unknown or missing, or a value of the wrong
    /// type.
    Toml(Box<CodeDiagnostic>),
    /// The file declares another format, version, kind or sheet version, or holds a value a sheet cannot hold.
    Format(Box<CodeDiagnostic>),
}

impl SheetFileError {
    /// Diagnostic the error carries.
    pub fn diagnostic(&self) -> &CodeDiagnostic {
        match self {
            Self::Toml(diagnostic) | Self::Format(diagnostic) => diagnostic,
        }
    }
}

/// Writes the diagnostic.
impl fmt::Display for SheetFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for SheetFileError {}
