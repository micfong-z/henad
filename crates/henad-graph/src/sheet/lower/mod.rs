//! Lowering of a sheet: the graph document a sheet runs as, built from the sheet alone.
//!
//! The lowering builds the document in a fixed order: the world and its declarations, then one pass per part of the
//! sheet in stack order. The starts make the init pass `start_cells`, the card that runs each tick the tick pass
//! `tick_<card>`, the colour rule a view pass `colours`, the measures the stats pass `measures`, and each button an
//! action and the action pass `button_<button>`. Lane, category, parameter and action ids are the sheet's ids, a
//! state's id is its variant's name, and a Become line's id is its rule's id.
//!
//! - A node's *owner* is the line it was lowered from, or the sheet for a node no line owns: a start, a measure, a
//!   colour rule, a button step, the read of one of my properties that every line of a pass shares, and the change
//!   and write of a property. [`ids`] numbers the nodes from their owners, and [`table`] records each node's owner and
//!   slot path.
//! - A *change* is a Become or Set line. The Becomes of one property lower to one `cat.transition` with a row per
//!   line, the Sets of one property to a `logic.select` chain from the bottom line up. Rows come in display order
//!   where two lines can both apply to one cell, and in line id order where they cannot, so a reorder that cannot
//!   change the result leaves the document as it was.
//! - A *skipped* line, one the checker gives a skip warning, is left out with every line that reads it.
//!
//! The lowering refuses, each at its table, a world of agents, the line kinds this version does not lower, and a sheet
//! past the node limits of [`crate::sheet::limits`].

mod buttons;
mod calculate;
mod card;
mod colours;
mod draw;
mod ids;
mod keys;
mod look;
mod measures;
mod start;
pub(super) mod table;
mod value;
mod world;

use std::collections::{HashMap, HashSet};

use self::ids::{Numberer, Owner, SheetKey};
use self::table::{LineMap, NodeOrigin};
use self::value::Operand;
use super::check::{Place, SheetReport};
use super::document::{MadeOf, SheetDocument, SheetProperty};
use super::sentence;
use crate::document::{GraphDocument, Pass};
use crate::ids::NodeId;
use crate::node::Node;
use crate::value::{ConfigValue, InputValue, PortRef};

/// A sheet lowered to a graph document, with the table of its nodes.
#[derive(Debug)]
pub(super) struct Lowered {
    /// The graph document.
    pub document: GraphDocument,
    /// Owner and slot path of every node of [`Self::document`].
    pub map: LineMap,
}

/// One problem that stops a sheet from lowering, at the table it is about.
#[derive(Debug, Clone)]
pub(super) struct Problem {
    /// Message of the problem.
    pub message: String,
    /// Table the problem is about.
    pub place: Place,
}

/// Lowers `document`, whose check gave `report` with no error.
///
/// # Errors
///
/// Returns every problem found: a world of agents, a line kind or start this version does not lower, a key two draws
/// take (V18), and a sheet past the node limits.
pub(super) fn lower(document: &SheetDocument, report: &SheetReport) -> Result<Lowered, Vec<Problem>> {
    if document.world.made_of == MadeOf::Agents {
        return Err(vec![Problem {
            message: sentence::NOT_YET_AGENTS.to_owned(),
            place: Place::World,
        }]);
    }
    let mut builder = Builder::new(document, report);
    let cells: &[SheetProperty] = document.cells.as_deref().unwrap_or(&[]);
    if cells.is_empty() {
        // A grid with no property lowers to its world alone, and the checker's warning says why nothing runs.
        return builder.finish();
    }
    world::declare(&mut builder);
    start::lower(&mut builder);
    card::lower_tick(&mut builder);
    colours::lower(&mut builder);
    measures::lower(&mut builder);
    buttons::lower(&mut builder);
    builder.finish()
}

/// Where a node is written: its pass, owner, slot path and region.
#[derive(Debug, Clone)]
struct Site {
    /// Index of the pass in [`Builder::passes`].
    pass: usize,
    /// Owner of the node.
    owner: Owner,
    /// Slot path inside the owner.
    path: String,
    /// Region the node sits in, as a temporary id.
    region: Option<NodeId>,
}

impl Site {
    /// Returns the site with `segment` added to its slot path.
    fn at(&self, segment: &str) -> Self {
        let path = if self.path.is_empty() {
            segment.to_owned()
        } else {
            format!("{}.{segment}", self.path)
        };
        Self { path, ..self.clone() }
    }

    /// Returns the site inside the region `region`.
    fn inside(&self, region: NodeId) -> Self {
        Self {
            region: Some(region),
            ..self.clone()
        }
    }
}

/// A node written with a temporary id, its owner and its slot path.
#[derive(Debug)]
struct Draft {
    /// The node, its id and links temporary until [`Builder::finish`] numbers them.
    node: Node,
    /// Owner of the node.
    owner: Owner,
    /// Slot path inside the owner.
    path: String,
}

/// A pass being written.
#[derive(Debug)]
struct PassDraft {
    /// The pass, its body empty until [`Builder::finish`].
    pass: Pass,
    /// Nodes of the pass, in the order they were written.
    drafts: Vec<Draft>,
    /// Read of each of my properties in the pass, by property id.
    reads: HashMap<String, Operand>,
    /// Properties the pass writes.
    written: HashSet<String>,
}

/// State of one lowering.
#[derive(Debug)]
struct Builder<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// The checker's report of the sheet.
    report: &'a SheetReport,
    /// The document being built, its passes empty until [`Self::finish`].
    graph: GraphDocument,
    /// Passes being written, in stack order.
    passes: Vec<PassDraft>,
    /// Number of the last temporary id handed out.
    last_temporary: u32,
    /// Problems found so far.
    problems: Vec<Problem>,
    /// Draws written so far, for the check of their keys.
    draws: Vec<keys::Draw>,
}

impl<'a> Builder<'a> {
    /// Returns a builder of the lowering of `document` over its grid.
    fn new(document: &'a SheetDocument, report: &'a SheetReport) -> Self {
        let mut graph = GraphDocument::new(&document.sheet.id, world::grid(document));
        graph.name.clone_from(&document.sheet.name);
        graph.target.name.clone_from(&document.sheet.name);
        Self {
            document,
            report,
            graph,
            passes: Vec::new(),
            last_temporary: 0,
            problems: Vec::new(),
            draws: Vec::new(),
        }
    }

    /// Cells properties of the sheet.
    fn cells(&self) -> &'a [SheetProperty] {
        self.document.cells.as_deref().unwrap_or(&[])
    }

    /// Returns the cells property `id` with its index.
    fn property(&self, id: &str) -> Option<(usize, &'a SheetProperty)> {
        self.cells().iter().enumerate().find(|(_, property)| property.id == id)
    }

    /// Adds `pass` after the passes written so far and returns its index.
    fn add_pass(&mut self, pass: Pass) -> usize {
        self.passes.push(PassDraft {
            pass,
            drafts: Vec::new(),
            reads: HashMap::new(),
            written: HashSet::new(),
        });
        self.passes.len() - 1
    }

    /// Records a problem at `place`.
    fn problem(&mut self, message: impl Into<String>, place: Place) {
        self.problems.push(Problem {
            message: message.into(),
            place,
        });
    }

    /// Writes a node of kind `op` at `site` with `config` and `inputs`, and returns its temporary id.
    fn emit(
        &mut self,
        site: &Site,
        op: &str,
        config: Vec<(&str, ConfigValue)>,
        inputs: Vec<(String, InputValue)>,
    ) -> NodeId {
        self.last_temporary += 1;
        let id = NodeId::new(self.last_temporary).unwrap_or_else(|| unreachable!("temporary ids count from 1"));
        let mut node = Node::new(id, op);
        node.config = config.into_iter().map(|(key, value)| (key.to_owned(), value)).collect();
        node.inputs = inputs.into_iter().collect();
        node.region = site.region;
        self.passes[site.pass].drafts.push(Draft {
            node,
            owner: site.owner.clone(),
            path: site.path.clone(),
        });
        id
    }

    /// Records a draw with key `key`, owned by `owner` in words, at `place`.
    fn record_draw(&mut self, key: u32, owner: String, place: Place) {
        self.draws.push(keys::Draw { key, owner, place });
    }

    /// Returns the operand of parameter `id`.
    ///
    /// # Errors
    ///
    /// Returns the message for a parameter the sheet does not declare.
    fn param(&self, id: &str) -> Result<Operand, String> {
        self.document
            .parameters
            .iter()
            .find(|parameter| parameter.id == id)
            .map(|parameter| Operand::Param(id.to_owned(), world::parameter_type(parameter)))
            .ok_or_else(|| format!("'{id}' is no parameter"))
    }

    /// Returns the read of my property `property` in pass `pass`, writing the shared `lane.get` the first time.
    fn read(&mut self, pass: usize, property: &str) -> Result<Operand, String> {
        if let Some(read) = self.passes[pass].reads.get(property) {
            return Ok(read.clone());
        }
        let (index, found) = self
            .property(property)
            .ok_or_else(|| format!("'{property}' is no property of the cells"))?;
        let value_type = world::lane_type(found);
        let site = Site {
            pass,
            owner: Owner::Sheet(SheetKey {
                group: ids::GROUP_READ,
                index: index as u32,
            }),
            path: format!("cells.{property}"),
            region: None,
        };
        let node = self.emit(&site, "lane.get", vec![("lane", text(property))], Vec::new());
        let read = Operand::Link(PortRef { node, port: None }, value_type);
        self.passes[pass].reads.insert(property.to_owned(), read.clone());
        Ok(read)
    }

    /// Writes `value` to my property `property` in pass `pass`, at `site`.
    ///
    /// # Errors
    ///
    /// Returns the message for a property the pass writes already.
    fn write(&mut self, site: &Site, property: &str, value: InputValue) -> Result<(), String> {
        if !self.passes[site.pass].written.insert(property.to_owned()) {
            return Err(format!("'{property}' is changed twice by one button"));
        }
        self.emit(
            site,
            "lane.set",
            vec![("lane", text(property))],
            vec![("value".to_owned(), value)],
        );
        Ok(())
    }

    /// Numbers every node, checks the keys of the draws, and returns the document with its table.
    fn finish(mut self) -> Result<Lowered, Vec<Problem>> {
        let clashes = keys::clashes(&self.draws);
        self.problems.extend(clashes);
        if !self.problems.is_empty() {
            return Err(self.problems);
        }
        let mut numberer = Numberer::default();
        let mut renumbered: HashMap<NodeId, NodeId> = HashMap::new();
        let mut map = LineMap::default();
        let numbering = |numberer: &mut Numberer, owner: &Owner| match owner {
            Owner::Line { id, number } => numberer.next_of_line(id, *number),
            Owner::Sheet(_) => numberer.next_of_sheet(),
        };
        for pass in &self.passes {
            // The sheet's nodes in their walk, then every line's in the order each line wrote them.
            let mut order: Vec<usize> = (0..pass.drafts.len()).collect();
            order.sort_by_key(|&index| match &pass.drafts[index].owner {
                Owner::Sheet(key) => (0, *key),
                Owner::Line { .. } => (1, SheetKey { group: 0, index: 0 }),
            });
            for index in order {
                let draft = &pass.drafts[index];
                let number = numbering(&mut numberer, &draft.owner).map_err(|message| {
                    let place = match &draft.owner {
                        Owner::Line { id, .. } => self.line_place(id),
                        Owner::Sheet(_) => Place::Sheet,
                    };
                    vec![Problem { message, place }]
                })?;
                let id = NodeId::new(number).unwrap_or_else(|| unreachable!("node ids count from 1"));
                renumbered.insert(draft.node.id, id);
                map.insert(
                    id,
                    NodeOrigin {
                        owner: draft.owner.table_owner(),
                        path: draft.path.clone(),
                    },
                );
            }
        }
        let renumber = |id: NodeId| renumbered.get(&id).copied().unwrap_or(id);
        for draft in self.passes {
            let mut pass = draft.pass;
            for mut entry in draft.drafts {
                entry.node.id = renumber(entry.node.id);
                entry.node.region = entry.node.region.map(renumber);
                for input in entry.node.inputs.values_mut() {
                    if let InputValue::Link(link) = input {
                        link.node = renumber(link.node);
                    }
                }
                pass.body.nodes.push(entry.node);
            }
            pass.body.nodes.sort_by_key(|node| node.id);
            self.graph.passes.push(pass);
        }
        for stat in &mut self.graph.stats {
            stat.from.node = renumber(stat.from.node);
        }
        Ok(Lowered {
            document: self.graph,
            map,
        })
    }

    /// Returns the place of line `line_id`: its line table in the card that holds it.
    fn line_place(&self, line_id: &str) -> Place {
        match self.document.line(line_id) {
            Some((card, _)) => Place::Line(card, line_id.to_owned()),
            None => Place::Sheet,
        }
    }
}

/// Returns `value` as a configuration string.
fn text(value: &str) -> ConfigValue {
    ConfigValue::String(value.to_owned())
}
