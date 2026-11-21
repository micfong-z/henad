//! Limits that hold for an untrusted sheet.
//!
//! The limits of a sheet sit apart from the graph's in [`crate::limits`], since `library.lock` records each of those
//! with the node library.

/// Size in bytes of the largest sheet, checked before the text is parsed.
pub const MAX_SHEET_BYTES: usize = 1024 * 1024;

/// Depth of the tables and lists nested in a sheet, the file itself counted as depth 1.
///
/// Each phrase inside a phrase adds a table, and reading one takes stack space, so the depth is checked before the
/// file's tables are read.
pub const MAX_NESTING: usize = 24;

/// Depth of the groups nested in one formula: parentheses, function calls and the right side of `^`.
///
/// A formula is a plain string bounded only by [`MAX_SHEET_BYTES`], and parsing each group takes stack space.
pub const MAX_FORMULA_NESTING: usize = 32;

/// Depth of the operations nested in one formula that the lowering writes: each operator, negation and call counts
/// one level above its values.
///
/// The lowering writes a formula as the text of one expression node, whose reader nests at most 64 levels.
pub const MAX_FORMULA_DEPTH: usize = 16;

/// Number of line tables of one card, the lines inside When and Repeat lines included.
pub const MAX_LINES_PER_CARD: usize = 256;

/// Number of nodes that the lowering writes for one line.
///
/// Line `l<k>` owns the node ids from `n(1000k + 1)` to `n(1000k + 999)`.
pub const MAX_NODES_PER_LINE: usize = 999;

/// Number of nodes that the lowering writes for the sheet itself, outside every line.
///
/// These nodes own the ids `n1` to `n999`.
pub const MAX_SHEET_NODES: usize = 999;

/// Exponent `k` of the resolution 2^-`k` to which a ground property that adds up its marks rounds each mark.
///
/// A cell adds the marks of one tick up in a 32-bit word, so it holds at most 2^(32 - `k`), 65536, in one tick and
/// starts again from 0 past that.
pub const ADD_UP_RESOLUTION_SHIFT: u8 = 16;

/// Length in bytes of the longest id a sheet gives an item.
///
/// The lowering builds ids such as `tick_<card>` and `button_<button>` from sheet ids, and an id of the lowered graph
/// holds at most 64 bytes.
pub const MAX_SHEET_ID_BYTES: usize = 48;
