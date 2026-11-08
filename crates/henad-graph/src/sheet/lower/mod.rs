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
//! - A change line's *fire* is a Boolean output of the tick pass that holds for an element where the line applies.
//!   [`table::FireLines`] gathers the fires as probes, which a GPU program counts beside the document.
//!
//! A sheet of agents lowers the same way over its one kind of agent: the init pass `start_<kind>`, the tick pass of
//! its card, a view pass for its colours, the measures and the buttons, each pass over the agents. [`agents`] holds
//! the looks at agents within a distance, [`motion`] the changes that steer, hold and move an agent, and [`repeat`] the
//! Repeat lines. The ground under the agents lowers to fields, its own passes over the cells and the lines that read
//! and mark it, as [`ground`] describes, and Pick a cell as [`pick`] does. [`fuse`] holds the folds over the cells
//! around. The looks and picks of a card over one neighbourhood share them.
//!
//! The lowering refuses, each at its table, the line kinds this version does not lower, and a sheet past the node
//! limits of [`crate::sheet::limits`].

mod agents;
mod buttons;
mod calculate;
mod card;
mod colours;
mod draw;
mod fuse;
mod ground;
mod ids;
mod keys;
mod look;
mod measures;
mod motion;
mod pick;
mod rates;
mod repeat;
mod start;
pub(super) mod table;
mod value;
mod world;

use std::collections::{HashMap, HashSet};

use self::ids::{Numberer, Owner, SheetKey};
use self::table::{FireLines, LineMap, NodeOrigin};
use self::value::Operand;
use super::check::{Place, SheetReport, Thing};
use super::document::{Edges, LineKind, LineMode, MadeOf, PropertyType, SheetDocument, SheetProperty};
use super::sentence;
use crate::document::{Domain, GraphDocument, Pass, PassStage};
use crate::ids::NodeId;
use crate::node::Node;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};
use crate::wgsl::{FaultNames, Probes};

/// A sheet lowered to a graph document, with the table of its nodes.
#[derive(Debug)]
pub(super) struct Lowered {
    /// The graph document.
    pub document: GraphDocument,
    /// Owner and slot path of every node of [`Self::document`].
    pub map: LineMap,
    /// Name in the author's words of each look at agents, by the id of its fold.
    pub fault_names: FaultNames,
    /// Probes of the change lines of the tick pass, each with the line it counts for.
    pub fires: FireLines,
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
/// Returns every problem found: a line kind, start or table this version does not lower, a key two draws take (V18),
/// and a sheet past the node limits.
pub(super) fn lower(document: &SheetDocument, report: &SheetReport) -> Result<Lowered, Vec<Problem>> {
    let mut builder = Builder::new(document, report);
    if document.world.made_of == MadeOf::Agents {
        refuse_agent_tables(&mut builder);
        if document.agents.is_empty() {
            return builder.finish();
        }
    } else if builder.properties().is_empty() {
        // A grid with no property lowers to its world alone, and the checker's warning says why nothing runs.
        return builder.finish();
    }
    world::declare(&mut builder);
    ground::declare(&mut builder);
    start::lower(&mut builder);
    builder.on_ground(start::lower);
    card::lower_tick(&mut builder);
    colours::lower(&mut builder);
    builder.on_ground(colours::lower);
    measures::lower(&mut builder);
    builder.on_ground(measures::lower);
    buttons::lower(&mut builder);
    builder.finish()
}

/// Records a problem for each table of a sheet of agents that this version does not lower: walls around agents that
/// move by an arrow, and the first Move by line of a sheet whose ground blocks some states.
fn refuse_agent_tables(builder: &mut Builder<'_>) {
    let document = builder.document;
    let first_move_by = document.cards.iter().enumerate().find_map(|(card_index, card)| {
        let relations = super::relations::CardRelations::new(document, card);
        relations
            .order()
            .iter()
            .find(|id| {
                relations
                    .line(id)
                    .is_some_and(|line| line.kind == LineKind::Move && line.mode != Some(LineMode::To))
            })
            .map(|id| Place::Line(card_index, (*id).to_owned()))
    });
    let Some(move_by) = first_move_by else {
        return;
    };
    if document.world.edges == Edges::Walls {
        builder.problem(sentence::NOT_YET_AGENT_WALLS, Place::World);
    }
    if document
        .ground
        .as_ref()
        .is_some_and(|ground| !ground.blocked.is_empty())
    {
        builder.problem(sentence::NOT_YET_BLOCKED_MOVE_BY, move_by);
    }
}

/// Returns whether a card of `document` moves agents to places. The Heading of such agents is a step between cells.
fn moves_to_cells(document: &SheetDocument) -> bool {
    document
        .cards
        .iter()
        .flat_map(|card| card.line_tables.values())
        .any(|line| line.kind == LineKind::Move && line.mode == Some(LineMode::To))
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
    /// The `space.cell_of` of my position in a pass over agents, written at its first use.
    my_cell: Option<NodeId>,
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
    /// Kind of thing the sheet's properties belong to: the cells, or the one kind of agent.
    thing: Thing,
    /// Ids of the properties that have no lane, since nothing reads them.
    dropped: HashSet<String>,
    /// Name in the author's words of each fold whose fault a run reports, by temporary id.
    fault_names: Vec<(NodeId, String)>,
    /// Node whose output is the result of each line lowered so far, as a temporary id, with the line's id.
    results: Vec<(NodeId, String)>,
    /// Whether a card moves the agents to places, so the Heading is a whole step between cells.
    cell_mover: bool,
    /// Field and layer of each property of the ground, by property id.
    ground_layers: HashMap<String, (String, String)>,
    /// Function that fades each field of the ground whose properties fade: its id, the share kept per tick and the
    /// slot path of its nodes.
    fades: Vec<(String, InputValue, String)>,
    /// Index of the measure of each `[[stat]]` written so far, in the order of the stats.
    stat_measures: Vec<usize>,
    /// Boolean output that holds where each change line applies, as a temporary id and a port, with the line's id.
    fires: Vec<(NodeId, Option<String>, String)>,
}

impl<'a> Builder<'a> {
    /// Returns a builder of the lowering of `document` over its world.
    fn new(document: &'a SheetDocument, report: &'a SheetReport) -> Self {
        let mut graph = GraphDocument::new(&document.sheet.id, world::world(document));
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
            thing: match document.world.made_of {
                MadeOf::Cells => Thing::Cells,
                MadeOf::Agents => Thing::Agents(0),
            },
            dropped: HashSet::new(),
            fault_names: Vec::new(),
            results: Vec::new(),
            cell_mover: document.world.made_of == MadeOf::Agents && moves_to_cells(document),
            ground_layers: HashMap::new(),
            fades: Vec::new(),
            stat_measures: Vec::new(),
            fires: Vec::new(),
        }
    }

    /// Runs `lower` over the ground, when the sheet's agents walk on a ground with properties.
    fn on_ground(&mut self, lower: fn(&mut Self)) {
        if !self.is_agents() || self.ground_layers.is_empty() {
            return;
        }
        let thing = self.thing;
        self.thing = Thing::Ground;
        lower(self);
        self.thing = thing;
    }

    /// Returns whether the sheet's kind of thing, as lowered now, is the ground.
    fn is_ground(&self) -> bool {
        self.thing == Thing::Ground
    }

    /// Returns the type of the lane or field of property `property`.
    ///
    /// The Heading of agents that move to places is a step between cells, a `vec2i`.
    fn lane_type(&self, property: &SheetProperty) -> ValueType {
        if property.property_type == PropertyType::Heading && self.cell_mover {
            ValueType::Vec2I
        } else {
            world::lane_type(property)
        }
    }

    /// Returns whether the sheet is a sheet of agents.
    fn is_agents(&self) -> bool {
        matches!(self.thing, Thing::Agents(_))
    }

    /// Properties of the sheet's kind of thing: the cells' properties, or those of the one kind of agent.
    fn properties(&self) -> &'a [SheetProperty] {
        match self.thing {
            Thing::Agents(index) => self
                .document
                .agents
                .get(index)
                .map_or(&[][..], |kind| kind.properties.as_slice()),
            Thing::Ground => self
                .document
                .ground
                .as_ref()
                .map_or(&[][..], |ground| ground.properties.as_slice()),
            Thing::Cells => self.document.cells.as_deref().unwrap_or(&[]),
        }
    }

    /// Returns the property `id` of the sheet's kind of thing with its index.
    fn property(&self, id: &str) -> Option<(usize, &'a SheetProperty)> {
        self.properties()
            .iter()
            .enumerate()
            .find(|(_, property)| property.id == id)
    }

    /// Id of the sheet's kind of thing as a card's `every` names it: `cells`, or the id of the kind of agent.
    fn thing_id(&self) -> &'a str {
        match self.thing {
            Thing::Agents(index) => self.document.agents.get(index).map_or("", |kind| kind.id.as_str()),
            Thing::Ground => "ground",
            Thing::Cells => "cells",
        }
    }

    /// First segment of the slot path of a node the sheet owns for a property: `cells`, `agents` or `ground`.
    fn property_path(&self) -> &'static str {
        match self.thing {
            Thing::Agents(_) => "agents",
            Thing::Ground => "ground",
            Thing::Cells => "cells",
        }
    }

    /// Words naming the sheet's kind of thing in a message: "the cells", "the agents" or "the ground".
    fn thing_words(&self) -> &'static str {
        match self.thing {
            Thing::Agents(_) => "the agents",
            Thing::Ground => "the ground",
            Thing::Cells => "the cells",
        }
    }

    /// Domain of the passes over the sheet's kind of thing: the agents, or the cells of the grid or the ground.
    fn domain(&self) -> Domain {
        if self.is_agents() {
            Domain::Agents
        } else {
            Domain::Cells
        }
    }

    /// Returns the table of the property at `index`.
    fn property_place(&self, index: usize) -> Place {
        Place::Property(self.thing, index)
    }

    /// Returns the node `id` written in pass `pass`, to change its configuration after it was written.
    fn node_mut(&mut self, pass: usize, id: NodeId) -> Option<&mut Node> {
        self.passes[pass]
            .drafts
            .iter_mut()
            .find(|draft| draft.node.id == id)
            .map(|draft| &mut draft.node)
    }

    /// Adds `pass` after the passes written so far and returns its index.
    fn add_pass(&mut self, pass: Pass) -> usize {
        self.passes.push(PassDraft {
            pass,
            drafts: Vec::new(),
            reads: HashMap::new(),
            written: HashSet::new(),
            my_cell: None,
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

    /// Records the temporary node `node` as the node whose output is the result of line `line_id`.
    fn record_result(&mut self, node: NodeId, line_id: &str) {
        self.results.push((node, line_id.to_owned()));
    }

    /// Records the output `port` of the temporary node `node`, or its only output with `None`, as an output that holds
    /// for each element where line `line_id` applies.
    fn record_fire(&mut self, node: NodeId, port: Option<String>, line_id: &str) {
        self.fires.push((node, port, line_id.to_owned()));
    }

    /// Returns the probes of the recorded fires that a tick pass computes outside every region, each with its line,
    /// with `renumber` mapping a temporary id to the node's id.
    ///
    /// A fire of a button's pass is left out, since only a tick pass counts, and so is a node inside a region and a
    /// node whose outputs normalisation copies, an expression or a function call.
    fn fire_lines(&self, renumber: impl Fn(NodeId) -> NodeId) -> FireLines {
        let counted = |node: NodeId| {
            self.passes.iter().any(|pass| {
                pass.pass.stage == PassStage::Tick
                    && pass.drafts.iter().any(|draft| {
                        draft.node.id == node
                            && draft.node.region.is_none()
                            && !matches!(draft.node.op.as_str(), "math.expr" | "fn.call")
                    })
            })
        };
        let (outputs, lines) = self
            .fires
            .iter()
            .filter(|(node, _, _)| counted(*node))
            .map(|(node, port, line)| {
                (
                    PortRef {
                        node: renumber(*node),
                        port: port.clone(),
                    },
                    line.clone(),
                )
            })
            .unzip();
        FireLines::new(Probes::new(outputs), lines)
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
            .ok_or_else(|| format!("'{property}' is no property of {}", self.thing_words()))?;
        if self.dropped.contains(property) {
            return Err(format!("'{property}' has no value to read"));
        }
        let value_type = self.lane_type(found);
        let site = Site {
            pass,
            owner: Owner::Sheet(SheetKey {
                group: ids::GROUP_READ,
                index: index as u32,
            }),
            path: format!("{}.{property}", self.property_path()),
            region: None,
        };
        let node = if self.is_ground() {
            let (config, inputs) = self.ground_access(property)?;
            self.emit(&site, "field.get", config, inputs)
        } else {
            self.emit(&site, "lane.get", vec![("lane", text(property))], Vec::new())
        };
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
        if self.is_ground() {
            let (config, mut inputs) = self.ground_access(property)?;
            inputs.push(("value".to_owned(), value));
            self.emit(site, "field.set", config, inputs);
        } else {
            self.emit(
                site,
                "lane.set",
                vec![("lane", text(property))],
                vec![("value".to_owned(), value)],
            );
        }
        Ok(())
    }

    /// Returns the configuration and the layer input of a node that reads or writes the ground property `property`.
    ///
    /// # Errors
    ///
    /// Returns the message for a property of the ground that has no field.
    fn ground_access(&self, property: &str) -> Result<GroundAccess, String> {
        let (field, layer) = self
            .ground_layers
            .get(property)
            .ok_or_else(|| format!("'{property}' is no property of the ground"))?;
        Ok((
            vec![("field", text(field))],
            vec![(
                "layer".to_owned(),
                InputValue::Literal(Literal::Layer(MemberName {
                    owner: field.clone(),
                    member: layer.clone(),
                })),
            )],
        ))
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
        for (id, kept, path) in std::mem::take(&mut self.fades) {
            let function = ground::fade_function(&id, kept, || {
                numberer
                    .next_of_sheet()
                    .map(|number| NodeId::new(number).unwrap_or_else(|| unreachable!("node ids count from 1")))
                    .map_err(|message| {
                        vec![Problem {
                            message,
                            place: Place::Sheet,
                        }]
                    })
            })?;
            for node in &function.body.nodes {
                map.insert(
                    node.id,
                    NodeOrigin {
                        owner: table::NodeOwner::Sheet,
                        path: path.clone(),
                    },
                );
            }
            self.graph.functions.push(function);
        }
        let renumber = |id: NodeId| renumbered.get(&id).copied().unwrap_or(id);
        let fires = self.fire_lines(renumber);
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
        let mut fault_names = FaultNames::default();
        for (node, name) in self.fault_names {
            fault_names.insert(renumber(node), name);
        }
        for (node, line) in self.results {
            map.insert_result(line, renumber(node));
        }
        Ok(Lowered {
            document: self.graph,
            map,
            fault_names,
            fires,
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

/// Configuration and inputs of a node that reads or writes a property of the ground.
type GroundAccess = (Vec<(&'static str, ConfigValue)>, Vec<(String, InputValue)>);

/// Returns `value` as a configuration string.
fn text(value: &str) -> ConfigValue {
    ConfigValue::String(value.to_owned())
}
