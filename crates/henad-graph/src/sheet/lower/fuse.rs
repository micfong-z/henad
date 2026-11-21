//! Folds over the cells around: the `grid.fold` nodes shared by a card's Count, Look around and Pick a cell lines
//! over one neighbourhood (F1).
//!
//! A *family* of neighbourhoods is walked by one stencil: the 8 cells around and the 3 by 3 block share the Moore
//! stencil, and the 4 cells around take the von Neumann stencil. The first look of a family on a card writes a fold the
//! sheet owns. A later look of that family joins it while the fold holds at most [`MAX_ACCUMULATORS`] accumulators and
//! the look reads no line from the one that opened the fold onwards. Any other look opens a new fold of the family, so
//! a card past that number splits its looks into two folds, and a look that reads an earlier look's result never
//! shares that look's fold. A Moore fold includes the cell itself when the card looks at a 3 by 3 block through a fold
//! anywhere, and then each look at the 8 cells around skips offset (0, 0) through a guard joined into its
//! `region.put`s.
//!
//! Each look names its accumulators from its line, `<line>_count` and so on for a line and `<line>_p<k>_count` for
//! the `k`th look in a phrase of that line, and the fold lists them by line id. A reorder of looks that share one fold
//! leaves the fold as it was. On a card of agents the fold walks the ground around my cell, from the column and row of
//! my position, and stops at walls when the world has them. Its open test, the cells no blocked state of the ground
//! covers, is written once per fold.

use std::collections::BTreeMap;

use super::card::CardLowering;
use super::ids::{GROUP_CELL_FOLD, Owner};
use super::look::{Fold, FoldKind, stencil, write_accumulators};
use super::value::{Operand, port};
use super::{Site, text, visit_chips};
use crate::ids::NodeId;
use crate::limits::MAX_ACCUMULATORS;
use crate::sheet::document::{Around, Edges, LineKind, SheetChip, SheetLine, SheetValue};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal};

/// The folds over the cells around of one card.
#[derive(Debug, Default)]
pub(super) struct CellFolds {
    /// Every fold, in the order its first look was lowered.
    folds: Vec<CellFold>,
    /// Whether the card looks at a 3 by 3 block, so its Moore folds include the cell itself, once known.
    center: Option<bool>,
    /// Looks written so far in the phrases of each line, by line id.
    phrase_looks: BTreeMap<String, u32>,
}

/// One `grid.fold` and the looks it holds.
#[derive(Debug)]
struct CellFold {
    /// Temporary id of the fold.
    node: NodeId,
    /// Index in display order of the line whose look opened the fold.
    opened_at: usize,
    /// Whether the fold walks the Moore stencil, the 8 cells around with the cell itself when it includes it.
    moore: bool,
    /// Accumulators of the fold, each with its line's number, its look's place in the line and its place in the look.
    accumulators: Vec<((u32, u32, usize), ConfigValue)>,
    /// Number of accumulators the looks that joined the fold take.
    taken: usize,
    /// Test that skips the cell itself, written at its first use.
    guard: Option<Operand>,
    /// Test that a cell of the ground is open, written at its first use.
    open: Option<Operand>,
}

/// One look's place in a fold over the cells around.
#[derive(Debug, Clone)]
pub(super) struct CellLook {
    /// Temporary id of the fold.
    pub(super) node: NodeId,
    /// Index of the fold in [`CellFolds`].
    fold: usize,
    /// Start of the names of the look's accumulators.
    pub(super) prefix: String,
    /// Number of the look's line and the look's place among the line's looks, for the order of the accumulators.
    key: (u32, u32),
    /// Test that keeps the cells the look's neighbourhood names, `true` unless it must skip the cell itself.
    pub(super) within: Operand,
}

impl CardLowering<'_, '_> {
    /// Opens the look `line` over the cells around, which takes `needed` accumulators, at `site`: joins or writes its
    /// fold, pushes the fold on [`CardLowering::folds`] for the reads inside it, and returns the look.
    ///
    /// The caller pops the fold when the look's body is written. A look joins no fold that is open around it, and no
    /// fold opened at or above the last line it reads. Otherwise the fold would read its own result.
    ///
    /// # Errors
    ///
    /// Returns the message for a neighbourhood of agents, for a look that takes more accumulators than a fold holds,
    /// and for a look outside a line.
    pub(super) fn enter_cells(&mut self, site: &Site, line: &SheetLine, needed: usize) -> Result<CellLook, String> {
        let around = line.around;
        let (moore, _) = stencil(around)?;
        if needed > MAX_ACCUMULATORS {
            return Err(format!(
                "the line holds {needed} summaries with its count, expected at most {MAX_ACCUMULATORS}"
            ));
        }
        let (line_id, number) = match &site.owner {
            Owner::Line { id, number } => (id.clone(), *number),
            Owner::Sheet(_) => return Err("a look stands outside a line".to_owned()),
        };
        let own_index = self.relations.index(&line_id).unwrap_or(0);
        let last_read = self.last_line_read(own_index, line);
        let center = if let Some(center) = self.cell_folds.center {
            center
        } else {
            let center = self.card.line_tables.values().any(|table| self.folds_block(table));
            self.cell_folds.center = Some(center);
            center
        };
        let open_now: Vec<NodeId> = self.folds.iter().map(|open| open.node).collect();
        let joined = self.cell_folds.folds.iter().rposition(|fold| {
            fold.moore == moore
                && fold.taken + needed <= MAX_ACCUMULATORS
                && !open_now.contains(&fold.node)
                && last_read.is_none_or(|read| read < fold.opened_at)
        });
        let fold = match joined {
            Some(fold) => fold,
            None => self.write_cell_fold(moore, moore && center, own_index)?,
        };
        self.cell_folds.folds[fold].taken += needed;
        let node = self.cell_folds.folds[fold].node;
        let (prefix, place) = if site.path.is_empty() {
            (line_id, 0)
        } else {
            let count = self.cell_folds.phrase_looks.entry(line_id.clone()).or_insert(0);
            *count += 1;
            (format!("{line_id}_p{count}"), *count)
        };
        let within = if moore && center && around == Some(Around::Cells8) {
            self.center_guard(fold)
        } else {
            Operand::Constant(Literal::Bool(true))
        };
        self.folds.push(Fold {
            node,
            walled: None,
            reads: std::collections::HashMap::new(),
            kind: if self.builder.is_agents() {
                FoldKind::Ground
            } else {
                FoldKind::Cells
            },
        });
        Ok(CellLook {
            node,
            fold,
            prefix,
            key: (number, place),
            within,
        })
    }

    /// Returns the index in display order of the last line that the look `line` of the line at `own_index` reads, or
    /// `None` when it reads no line.
    ///
    /// The look reads each line that a result chip or a formula names, in its phrases at any depth too. For each
    /// property it reads as the lines above have changed it, the look reads every line above it that names that
    /// property.
    fn last_line_read(&self, own_index: usize, line: &SheetLine) -> Option<usize> {
        let relations = &self.relations;
        let card = self.card;
        let mut last: Option<usize> = None;
        let mut note = |index: Option<usize>| {
            if let Some(index) = index {
                last = Some(last.map_or(index, |known| known.max(index)));
            }
        };
        visit_chips(self.builder.document, line, &mut |chip| match chip {
            SheetChip::Result { line: read, .. } => note(relations.index(read)),
            SheetChip::MyNew(property) => {
                for (id, table) in &card.line_tables {
                    let index = relations.index(id);
                    if table.property.as_ref() == Some(property) && index.is_some_and(|index| index < own_index) {
                        note(index);
                    }
                }
            }
            _ => {}
        });
        last
    }

    /// Returns whether `line`, or a phrase in one of its slots at any depth, looks at a 3 by 3 block through a fold.
    ///
    /// A Count of their States property in some states looks through `grid.count` nodes instead.
    fn folds_block(&self, line: &SheetLine) -> bool {
        let own = match line.kind {
            LineKind::LookAround => line.around == Some(Around::Block3x3),
            LineKind::Count => {
                line.around == Some(Around::Block3x3)
                    && !(line.only.is_blank()
                        && line
                            .whose
                            .value()
                            .is_some_and(|whose| self.counted_states(whose).is_some()))
            }
            _ => false,
        };
        own || line.slot_values().any(|value| self.value_folds_block(value))
    }

    /// Returns whether `value` holds a phrase that looks at a 3 by 3 block through a fold.
    fn value_folds_block(&self, value: &SheetValue) -> bool {
        match value {
            SheetValue::Phrase(phrase) => self.folds_block(phrase),
            SheetValue::List(items) => items.iter().any(|item| self.value_folds_block(item)),
            SheetValue::Literal(_) | SheetValue::Chip(_) => false,
        }
    }

    /// Writes a fold the sheet owns over the Moore stencil, with the cell itself when `center` holds, or the von
    /// Neumann stencil, opened by the line at `opened_at` in display order, and returns its index.
    fn write_cell_fold(&mut self, moore: bool, center: bool, opened_at: usize) -> Result<usize, String> {
        let index = self.cell_folds.folds.len();
        let site = self.cell_fold_site(index, None);
        let agents = self.builder.is_agents();
        let boundary = if agents && self.builder.document.world.edges == Edges::Walls {
            "bounded"
        } else {
            "domain"
        };
        let mut inputs = vec![("when".to_owned(), InputValue::Literal(Literal::Bool(true)))];
        if agents {
            let cell = self.builder.my_cell(self.pass)?;
            inputs.push(("at".to_owned(), port(cell, "xy")));
        }
        let node = self.builder.emit(
            &site,
            "grid.fold",
            vec![
                ("order", text(if moore { "moore_row_major" } else { "von_neumann" })),
                ("include_center", ConfigValue::Bool(center)),
                ("boundary", text(boundary)),
                ("acc", ConfigValue::Array(Vec::new())),
            ],
            inputs,
        );
        self.cell_folds.folds.push(CellFold {
            node,
            opened_at,
            moore,
            accumulators: Vec::new(),
            taken: 0,
            guard: None,
            open: None,
        });
        Ok(index)
    }

    /// Returns the site of the nodes the sheet owns for the fold at `index`, inside `region`.
    fn cell_fold_site(&self, index: usize, region: Option<NodeId>) -> Site {
        Site {
            region,
            ..Site::sheet(
                self.pass,
                GROUP_CELL_FOLD,
                index as u32,
                format!("card.{}", self.card.id),
            )
        }
    }

    /// Returns the test of the fold at `index` that skips the cell itself, writing it the first time.
    fn center_guard(&mut self, index: usize) -> Operand {
        if let Some(guard) = &self.cell_folds.folds[index].guard {
            return guard.clone();
        }
        let node = self.cell_folds.folds[index].node;
        let site = self.cell_fold_site(index, Some(node));
        let guard = self.builder.node_value(
            &site,
            "cmp.ne",
            vec![
                ("a", port(node, "offset")),
                ("b", InputValue::Literal(Literal::Vec2I([0, 0]))),
            ],
            ValueType::Bool,
        );
        self.cell_folds.folds[index].guard = Some(guard.clone());
        guard
    }

    /// Returns the test that the cell the fold of `look` visits is open, writing it once per fold.
    pub(super) fn fold_open(&mut self, look: &CellLook) -> Result<Operand, String> {
        if let Some(open) = &self.cell_folds.folds[look.fold].open {
            return Ok(open.clone());
        }
        let site = self.cell_fold_site(look.fold, Some(look.node));
        let cell = port(look.node, "cell");
        let open = self.open_at(&site, &cell)?;
        self.cell_folds.folds[look.fold].open = Some(open.clone());
        Ok(open)
    }

    /// Records accumulator `config`, the `order`th of `look`, in the fold of the look.
    pub(super) fn add_cell_accumulator(&mut self, look: &CellLook, order: usize, config: ConfigValue) {
        self.cell_folds.folds[look.fold]
            .accumulators
            .push(((look.key.0, look.key.1, order), config));
    }

    /// Sets the input `init.<name>` of the fold of `look`, the value accumulator `name` starts from.
    pub(super) fn set_cell_init(&mut self, look: &CellLook, name: &str, value: InputValue) {
        let pass = self.pass;
        if let Some(node) = self.builder.node_mut(pass, look.node) {
            node.inputs.insert(format!("init.{name}"), value);
        }
    }

    /// Writes the accumulators of every fold over the cells around, in line id order.
    pub(super) fn finish_cell_folds(&mut self) {
        for fold in &mut self.cell_folds.folds {
            write_accumulators(self.builder, self.pass, fold.node, &mut fold.accumulators);
        }
    }
}
