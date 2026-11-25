//! Words of the sheet editor: the wording table, and the sentence of each line written from it.
//!
//! The *wording table* is every constant and function of this module that returns text the sheet editor shows. On the
//! card: its frame, the line sentences and their parts, the comparisons, the conditions and "Choose when", blanks and
//! their accessible names, pickers, add menus, result names, the notes under a line, "otherwise", the words and notes
//! of rates and chances, the Lines help and the card's messages. Around the card: the sheet and its forms, the New
//! model and New parameter dialogs, the top bar, the world tabs, the help tooltips, the Diagram and the conversion to a
//! node graph. The comments of the sheet writer and every editor take their words from here.
//!
//! Terms the table uses:
//!
//! - A *line number* is a line's place on its card, counted from 1 in display order, the lines inside a When or
//!   Repeat counted after it. It differs from the number in the line's id.
//! - A *line word* names a line inside a sentence: "line 2", or on a button's card "Clear line 1".
//! - A *line prefix* starts an accessible name: "Line 2", or on a button's card "Clear line 1".
//! - A *blank* is an empty slot, written `…` in a sentence written as plain text.
//! - A *chip* is a chosen value in a sentence, such as a state, a parameter or a result.
//!
//! [`Sentences`] writes the sentence of a line and the heading of a card from a [`SheetDocument`]. The sentence comes
//! as plain text or as its [`SentencePart`]s, the words, chips, blanks and selects the card draws.
//!
//! [`SheetDocument`]: crate::sheet::document::SheetDocument

mod card;
mod convert;
mod editor;
mod errors;
mod grammar;
mod notes;
mod parts;
mod pickers;
mod rates;
mod sentences;
mod sheet;

pub use card::*;
pub use convert::*;
pub use editor::*;
pub use errors::*;
pub use grammar::*;
pub use notes::*;
pub use parts::*;
pub use pickers::*;
pub use rates::*;
pub use sentences::*;
pub use sheet::*;
