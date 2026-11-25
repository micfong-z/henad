//! Node ids of a lowered sheet, numbered from the sheet alone.
//!
//! Line `l<k>` owns the ids `n(1000k + j)` for `j` from 1 to [`MAX_NODES_PER_LINE`], in the order the lowering writes
//! that line's nodes. The sheet owns `n1` to `n`[`MAX_SHEET_NODES`], numbered in a fixed walk: the passes in stack
//! order, and in each pass the nodes by their [`SheetKey`]. A node's id then depends on its line and its place inside
//! the line, never on the order of the other lines.

use std::collections::HashMap;

use super::table::NodeOwner;
use crate::ids::NodeId;
use crate::sheet::limits::{MAX_NODES_PER_LINE, MAX_SHEET_NODES};
use crate::sheet::sentence::{LINE_NUMBER_TOO_LARGE, LINE_TOO_LONG, SHEET_TOO_LARGE};

/// Place of a node the sheet owns in the walk that numbers them, inside its pass.
///
/// Nodes order by `group`, then `index`, then the order the lowering wrote them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SheetKey {
    /// Kind of node in the pass: the shared reads first, then the start, measure, colour or step nodes, then the
    /// changes.
    pub group: u32,
    /// Index of the property, measure or step inside the group.
    pub index: u32,
}

/// Group of the read of one of my properties, shared by every line of a pass.
pub(super) const GROUP_READ: u32 = 0;
/// Group of the nodes of a start, a measure, a colour rule or a button step.
pub(super) const GROUP_PART: u32 = 1;
/// Group of the change and the write of one property.
pub(super) const GROUP_CHANGE: u32 = 2;
/// Group of the nodes of one measure that no other measure shares.
pub(super) const GROUP_MEASURE: u32 = 3;
/// Group of the nodes a card's looks at agents share: the largest distance, the index's cell size and each fold.
pub(super) const GROUP_LOOK: u32 = 4;
/// Group of the nodes a card's folds over the cells around share: each fold, its guard and its open test.
pub(super) const GROUP_CELL_FOLD: u32 = 5;
/// Group of the nodes a pass over agents shares for the ground: the cell of my position.
pub(super) const GROUP_GROUND: u32 = 6;

/// Index in [`GROUP_CHANGE`] of the merged deposit of the Leave lines on the first field, past the index of every
/// property so the deposits follow the property changes.
pub(super) const LEAVE_INDEX_BASE: u32 = 10_000;

/// Number of node ids in the block each line owns.
const LINE_BLOCK: u32 = 1000;

/// Owner of a node while the lowering writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Owner {
    /// Line `l<number>`.
    Line {
        /// Id of the line.
        id: String,
        /// Number `k` of the line id `l<k>`.
        number: u32,
    },
    /// The sheet, at a place in the walk.
    Sheet(SheetKey),
}

impl Owner {
    /// Returns the owner of the table.
    pub(super) fn table_owner(&self) -> NodeOwner {
        match self {
            Self::Line { id, .. } => NodeOwner::Line(id.clone()),
            Self::Sheet(_) => NodeOwner::Sheet,
        }
    }
}

/// Returns the number `k` of the line id `l<k>`, `None` for another form.
pub(super) fn line_number(line_id: &str) -> Option<u32> {
    let digits = line_id.strip_prefix('l')?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Numberer of the nodes of a lowering: the next `j` of each line and the next id the sheet owns.
#[derive(Debug, Default)]
pub(super) struct Numberer {
    /// Nodes numbered so far for each line, by line number.
    line_counts: HashMap<u32, u32>,
    /// Nodes the sheet owns numbered so far.
    sheet_count: u32,
}

impl Numberer {
    /// Returns the id of the next node of line `l<number>`.
    ///
    /// # Errors
    ///
    /// Returns the message for a line past [`MAX_NODES_PER_LINE`] nodes, or a line number whose ids pass `u32`. The
    /// message is placed at the line, and names it by neither its id nor its number.
    pub(super) fn next_of_line(&mut self, number: u32) -> Result<NodeId, String> {
        let count = self.line_counts.entry(number).or_insert(0);
        *count += 1;
        if *count as usize > MAX_NODES_PER_LINE {
            return Err(LINE_TOO_LONG.to_owned());
        }
        number
            .checked_mul(LINE_BLOCK)
            .and_then(|base| base.checked_add(*count))
            .map(node_id)
            .ok_or_else(|| LINE_NUMBER_TOO_LARGE.to_owned())
    }

    /// Returns the id of the next node the sheet owns.
    ///
    /// # Errors
    ///
    /// Returns the message for a sheet past [`MAX_SHEET_NODES`] nodes of its own.
    pub(super) fn next_of_sheet(&mut self) -> Result<NodeId, String> {
        self.sheet_count += 1;
        if self.sheet_count as usize > MAX_SHEET_NODES {
            return Err(SHEET_TOO_LARGE.to_owned());
        }
        Ok(node_id(self.sheet_count))
    }
}

/// Returns the node id `number`, which counts from 1.
fn node_id(number: u32) -> NodeId {
    NodeId::new(number).unwrap_or_else(|| unreachable!("node ids count from 1"))
}
