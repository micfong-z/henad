//! Edits of regions: their accumulators, the placement of a region's output half, the regions a node sits in, and
//! wrapping nodes in a new Repeat.
//!
//! A *bare link* names a node without a port, such as `"n7"`, and reads the node's only output. An edit that can give
//! a node a second output, or rename its only one, first names the port on every bare link to the node, so each link
//! keeps reading the output it read before. The *subtree* of a node is the node and every node whose region path
//! holds it.

use std::collections::{BTreeMap, BTreeSet};

use super::{EditCommand, EditError, GraphBuilder, Scope};
use crate::catalogue::lookup;
use crate::check::{CheckReport, PassReport, PortType};
use crate::file::GraphFile;
use crate::ids::{NodeId, is_port_segment};
use crate::limits::{MAX_ACCUMULATORS, MAX_REGION_DEPTH};
use crate::node::{Accumulator, Combine, Node, NodeBody, OP_REGION_PUT, OP_REGION_REPEAT};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};

/// Kind of the neighborhood fold, the one region besides a Repeat that a Repeat may hold or sit in.
const OP_GRID_FOLD: &str = "grid.fold";

/// Distance in canvas points between the wrapped nodes and the halves of a new Repeat.
const WRAP_GAP: f64 = 60.0;

/// Width in canvas points assumed for a half of a new Repeat, and for a node whose drawn size the builder lacks.
const WRAP_NODE_WIDTH: f64 = 220.0;

/// Height in canvas points assumed for a node whose drawn size the builder lacks, and the row of a new put.
const WRAP_NODE_HEIGHT: f64 = 120.0;

/// Nodes an edit wraps.
pub(super) struct Selection {
    /// Selected nodes whose region holds no other selected node.
    pub tops: Vec<NodeId>,
    /// Region the top nodes share, `None` for the top of the body.
    pub parent: Option<NodeId>,
    /// Top nodes and every node inside them.
    pub wrapped: BTreeSet<NodeId>,
}

/// One output of the wrapped nodes that a node outside them reads.
pub(super) struct LeavingOutput {
    /// Output the reader read.
    pub source: PortRef,
    /// Inferred type of the output.
    pub value_type: ValueType,
}

impl GraphBuilder {
    /// Runs one of the region commands on the builder in place.
    pub(super) fn run_region(&mut self, command: EditCommand) -> Result<Vec<NodeId>, EditError> {
        match command {
            EditCommand::MoveRegionOutput { node, pos } => self.move_region_output(node, pos).map(|()| Vec::new()),
            EditCommand::AddAccumulator {
                scope,
                node,
                accumulator,
            } => self.add_accumulator(&scope, node, accumulator).map(|()| Vec::new()),
            EditCommand::SetAccumulator {
                scope,
                node,
                accumulator,
            } => self.set_accumulator(&scope, node, accumulator).map(|()| Vec::new()),
            EditCommand::RenameAccumulator { scope, node, from, to } => {
                self.rename_accumulator(&scope, node, &from, &to).map(|()| Vec::new())
            }
            EditCommand::RemoveAccumulator { scope, node, name } => {
                self.remove_accumulator(&scope, node, &name).map(|()| Vec::new())
            }
            EditCommand::WrapInRepeat { scope, nodes, bounds } => self.wrap_in_repeat(&scope, &nodes, bounds),
            other => Err(EditError::Invalid(format!("{other:?} is no region command"))),
        }
    }

    /// Names the port on every bare link to `node` of `scope`, and on a `[[stat]]` that reads it bare, when the node
    /// has one output.
    pub(super) fn name_bare_links(&mut self, scope: &Scope, node: NodeId) {
        let Ok(body) = self.body(scope) else { return };
        let Some(source) = body.node(node) else { return };
        let Some(def) = lookup(&source.op, self.document.library) else {
            return;
        };
        let only = match def.resolved_outputs(source, &self.document).as_slice() {
            [only] => only.name.clone(),
            _ => return,
        };
        if let Ok(body) = self.body_mut(scope) {
            for reader in &mut body.nodes {
                for value in reader.inputs.values_mut() {
                    if let InputValue::Link(link) = value
                        && link.node == node
                        && link.port.is_none()
                    {
                        link.port = Some(only.clone());
                    }
                }
            }
        }
        if matches!(scope, Scope::Pass(_)) {
            for stat in &mut self.document.stats {
                if stat.from.node == node && stat.from.port.is_none() {
                    stat.from.port = Some(only.clone());
                }
            }
        }
    }

    /// Returns the report of a check of the document as it is now.
    ///
    /// # Errors
    ///
    /// Returns an error when the document's canonical text does not read back.
    pub(super) fn check_report(&self) -> Result<CheckReport, EditError> {
        let file = GraphFile::from_document(&self.document, None)
            .map_err(|error| EditError::Invalid(format!("the file does not read back: {error}")))?;
        Ok(file.check())
    }

    /// Returns the accumulators of the region `node` of `scope`.
    fn region_accumulators(&self, scope: &Scope, node: NodeId) -> Result<Vec<Accumulator>, EditError> {
        self.require_region(scope, node)?;
        let region = self
            .body(scope)?
            .node(node)
            .ok_or_else(|| EditError::Missing(format!("the {scope} holds no node {node}")))?;
        region.accumulators().map_err(EditError::Invalid)
    }

    /// Replaces the accumulators of the region `node` of `scope` by `accumulators`.
    fn set_accumulators(&mut self, scope: &Scope, node: NodeId, accumulators: &[Accumulator]) -> Result<(), EditError> {
        let list = ConfigValue::Array(accumulators.iter().map(Accumulator::to_config).collect());
        self.node_mut(scope, node)?.config.insert("acc".to_owned(), list);
        Ok(())
    }

    /// Returns an error unless `name` can name a new accumulator of the region `node`: a port name that no
    /// accumulator in `accumulators` and no inner output of the region holds.
    fn check_accumulator_name(
        &self,
        scope: &Scope,
        node: NodeId,
        accumulators: &[Accumulator],
        name: &str,
    ) -> Result<(), EditError> {
        if !is_port_segment(name) {
            return Err(EditError::Invalid(format!(
                "'{name}' is no accumulator name, which starts with a letter and holds letters, digits and `_`"
            )));
        }
        if accumulators.iter().any(|known| known.name == name) {
            return Err(EditError::Invalid(format!(
                "node {node} already holds the accumulator '{name}'"
            )));
        }
        let op = self.body(scope)?.node(node).map(|region| region.op.clone());
        let inner = op
            .and_then(|op| lookup(&op, self.document.library))
            .is_some_and(|def| def.inner_outputs.iter().any(|socket| socket.name == name));
        if inner {
            return Err(EditError::Invalid(format!(
                "'{name}' names an inner output of node {node}"
            )));
        }
        Ok(())
    }

    /// Adds `accumulator` after the accumulators of the region `node` of `scope`.
    fn add_accumulator(&mut self, scope: &Scope, node: NodeId, accumulator: Accumulator) -> Result<(), EditError> {
        let mut accumulators = self.region_accumulators(scope, node)?;
        self.check_accumulator_name(scope, node, &accumulators, &accumulator.name)?;
        if accumulators.len() >= MAX_ACCUMULATORS {
            return Err(EditError::Invalid(format!(
                "node {node} holds {MAX_ACCUMULATORS} accumulators, the most a region holds"
            )));
        }
        self.name_bare_links(scope, node);
        accumulators.push(accumulator);
        self.set_accumulators(scope, node, &accumulators)
    }

    /// Replaces the type and the combine of the accumulator of the region `node` that `accumulator` names.
    fn set_accumulator(&mut self, scope: &Scope, node: NodeId, accumulator: Accumulator) -> Result<(), EditError> {
        let mut accumulators = self.region_accumulators(scope, node)?;
        let known = accumulators
            .iter_mut()
            .find(|known| known.name == accumulator.name)
            .ok_or_else(|| EditError::Missing(format!("node {node} holds no accumulator '{}'", accumulator.name)))?;
        *known = accumulator;
        self.set_accumulators(scope, node, &accumulators)
    }

    /// Renames the accumulator `from` of the region `node` to `to`, with its `init.<acc>` input, the puts that feed
    /// it, the links that read it and a `[[stat]]` that reads it.
    fn rename_accumulator(&mut self, scope: &Scope, node: NodeId, from: &str, to: &str) -> Result<(), EditError> {
        let mut accumulators = self.region_accumulators(scope, node)?;
        let position = accumulators
            .iter()
            .position(|known| known.name == from)
            .ok_or_else(|| EditError::Missing(format!("node {node} holds no accumulator '{from}'")))?;
        if from == to {
            return Ok(());
        }
        self.check_accumulator_name(scope, node, &accumulators, to)?;
        self.name_bare_links(scope, node);
        accumulators[position].name = to.to_owned();
        self.set_accumulators(scope, node, &accumulators)?;
        let (from_init, to_init) = (format!("init.{from}"), format!("init.{to}"));
        let (from_prev, to_prev) = (format!("prev.{from}"), format!("prev.{to}"));
        let body = self.body_mut(scope)?;
        for member in &mut body.nodes {
            if member.id == node
                && let Some(value) = member.inputs.remove(&from_init)
            {
                member.inputs.insert(to_init.clone(), value);
            }
            if member.op == OP_REGION_PUT
                && member.region == Some(node)
                && member.config.get("acc").and_then(ConfigValue::as_str) == Some(from)
            {
                member
                    .config
                    .insert("acc".to_owned(), ConfigValue::String(to.to_owned()));
            }
            for value in member.inputs.values_mut() {
                let InputValue::Link(link) = value else { continue };
                if link.node != node {
                    continue;
                }
                if link.port.as_deref() == Some(from) {
                    link.port = Some(to.to_owned());
                } else if link.port.as_deref() == Some(from_prev.as_str()) {
                    link.port = Some(to_prev.clone());
                }
            }
        }
        if matches!(scope, Scope::Pass(_)) {
            for stat in &mut self.document.stats {
                if stat.from.node == node && stat.from.port.as_deref() == Some(from) {
                    stat.from.port = Some(to.to_owned());
                }
            }
        }
        Ok(())
    }

    /// Removes the accumulator `name` of the region `node`, when no put feeds it, no link reads it and no
    /// `init.<acc>` input is written.
    fn remove_accumulator(&mut self, scope: &Scope, node: NodeId, name: &str) -> Result<(), EditError> {
        let mut accumulators = self.region_accumulators(scope, node)?;
        let position = accumulators
            .iter()
            .position(|known| known.name == name)
            .ok_or_else(|| EditError::Missing(format!("node {node} holds no accumulator '{name}'")))?;
        let only = accumulators.len() == 1;
        let prev = format!("prev.{name}");
        let body = self.body(scope)?;
        let mut references = Vec::new();
        for member in &body.nodes {
            if member.id == node && member.inputs.contains_key(&format!("init.{name}")) {
                references.push(format!("the input `init.{name}` of node {node}"));
            }
            if member.op == OP_REGION_PUT
                && member.region == Some(node)
                && member.config.get("acc").and_then(ConfigValue::as_str) == Some(name)
            {
                references.push(format!("the put node {}", member.id));
            }
            let reads = member.inputs.values().any(|value| {
                matches!(value, InputValue::Link(link) if link.node == node
                    && (link.port.as_deref() == Some(name)
                        || link.port.as_deref() == Some(prev.as_str())
                        || (only && link.port.is_none())))
            });
            if reads {
                references.push(format!("node {}", member.id));
            }
        }
        if matches!(scope, Scope::Pass(_)) {
            for stat in &self.document.stats {
                if stat.from.node == node
                    && (stat.from.port.as_deref() == Some(name) || (only && stat.from.port.is_none()))
                {
                    references.push(format!("the statistic '{}'", stat.label));
                }
            }
        }
        if !references.is_empty() {
            return Err(EditError::Referenced {
                subject: format!("the accumulator '{name}' of node {node}"),
                references,
            });
        }
        accumulators.remove(position);
        self.set_accumulators(scope, node, &accumulators)
    }

    /// Places the output half of the region `node` at `pos`.
    fn move_region_output(&mut self, node: NodeId, pos: [f64; 2]) -> Result<(), EditError> {
        let region = self
            .all_nodes()
            .find(|known| known.id == node)
            .ok_or_else(|| EditError::Missing(format!("the file holds no node {node}")))?;
        if !lookup(&region.op, self.document.library).is_some_and(|def| def.is_region) {
            return Err(EditError::Invalid(format!("node {node} is no region")));
        }
        self.editor.set_output_pos(node, pos);
        Ok(())
    }

    /// Names the port on each bare link from the subtree of `node` to a region on `path`, before the subtree moves
    /// under those regions. Read from inside its body, a bare link to a region names nothing.
    pub(super) fn name_links_into_path(&mut self, scope: &Scope, node: NodeId, path: &[NodeId]) {
        let Ok(body) = self.body(scope) else { return };
        let subtree = subtree(body, node);
        let mut sources = BTreeSet::new();
        for member in body.nodes.iter().filter(|member| subtree.contains(&member.id)) {
            for value in member.inputs.values() {
                if let InputValue::Link(link) = value
                    && link.port.is_none()
                    && path.contains(&link.node)
                {
                    sources.insert(link.node);
                }
            }
        }
        for source in sources {
            self.name_bare_links(scope, source);
        }
    }

    /// Returns the selection of `nodes` of `scope` that `what` wraps: the nodes whose region holds no other selected
    /// node, the region they share, and their subtrees.
    ///
    /// # Errors
    ///
    /// Returns an error for no node, a node `scope` lacks, and top nodes in different regions.
    pub(super) fn selection(&self, scope: &Scope, nodes: &[NodeId], what: &str) -> Result<Selection, EditError> {
        if nodes.is_empty() {
            return Err(EditError::Invalid(format!("{what} wraps at least one node")));
        }
        for node in nodes {
            self.require_node(scope, *node)?;
        }
        let body = self.body(scope)?;
        let selected: BTreeSet<NodeId> = nodes.iter().copied().collect();
        let tops: Vec<NodeId> = selected
            .iter()
            .copied()
            .filter(|node| !body.region_path(*node).iter().any(|region| selected.contains(region)))
            .collect();
        let parents: BTreeSet<Option<NodeId>> = tops.iter().map(|node| body.node(*node)?.region).collect();
        let parent = match parents.into_iter().collect::<Vec<_>>().as_slice() {
            [parent] => *parent,
            _ => {
                return Err(EditError::Invalid(
                    "the selected nodes sit in different regions. Select nodes of one region".to_owned(),
                ));
            }
        };
        let wrapped = tops.iter().flat_map(|top| subtree(body, *top)).collect();
        Ok(Selection { tops, parent, wrapped })
    }

    /// Wraps `nodes` of `scope` in a new Repeat, and returns the ids of the nodes it makes: the Repeat, then
    /// one put per accumulator.
    ///
    /// The Repeat runs once (`count` 1 and `max` 1), so the pass computes what it did before. Each output of the
    /// wrapped nodes that a node outside them reads becomes a `carry` accumulator, started at zero or at the first
    /// variant and fed by a put, and the reader reads the accumulator. `bounds` is the canvas rectangle of the wrapped
    /// nodes as drawn, and without it the builder assumes a size for each node.
    ///
    /// A put whose region `nodes` leaves out, a kind a region body refuses, such as `lane.set`, and a Repeat that
    /// library 1 does not let nest where it would sit are refused. Note that a Repeat sits only at the top of a body,
    /// in a Repeat or in a neighborhood fold, holds only Repeats and neighborhood folds as regions, and leaves regions
    /// at most [`MAX_REGION_DEPTH`] deep.
    fn wrap_in_repeat(
        &mut self,
        scope: &Scope,
        nodes: &[NodeId],
        bounds: Option<[[f64; 2]; 2]>,
    ) -> Result<Vec<NodeId>, EditError> {
        let Selection { tops, parent, wrapped } = self.selection(scope, nodes, "a Repeat")?;
        let body = self.body(scope)?;
        for node in body.nodes.iter().filter(|node| wrapped.contains(&node.id)) {
            if lookup(&node.op, self.document.library).is_some_and(|def| !def.contexts.region) {
                return Err(EditError::Invalid(format!(
                    "node {} does not belong in a region body. Leave it out of the selection",
                    node.id
                )));
            }
            if node.op == OP_REGION_PUT && node.region.is_none_or(|region| !wrapped.contains(&region)) {
                return Err(EditError::Invalid(format!(
                    "the put {} feeds a region outside the selection. Select the region too",
                    node.id
                )));
            }
        }
        check_wrap_nesting(body, self.document.library, parent, &wrapped)?;
        let leaving = self.leaving_outputs(scope, body, &wrapped)?;
        let bounds = bounds.unwrap_or_else(|| self.assumed_bounds(&wrapped));

        let repeat = self.add(
            scope,
            OP_REGION_REPEAT,
            parent,
            Some([bounds[0][0] - WRAP_GAP - WRAP_NODE_WIDTH, bounds[0][1]]),
        )?;
        let mut made = vec![repeat];
        let output_x = bounds[1][0] + 2.0 * WRAP_GAP + WRAP_NODE_WIDTH;
        self.editor.set_output_pos(repeat, [output_x, bounds[0][1]]);
        let target = self.node_mut(scope, repeat)?;
        target.config.insert("max".to_owned(), ConfigValue::Integer(1));
        target
            .inputs
            .insert("count".to_owned(), InputValue::Literal(Literal::U32(1)));
        for top in &tops {
            self.node_mut(scope, *top)?.region = Some(repeat);
        }

        let (puts, names) = self.feed_accumulators(scope, repeat, &leaving, bounds)?;
        made.extend(puts);
        self.read_accumulators(scope, repeat, &wrapped, &names)?;
        Ok(made)
    }

    /// Gives the new Repeat `repeat` one `carry` accumulator per output in `leaving`, each fed by a new put placed
    /// right of `bounds`. Returns the puts, and the accumulator's name for each output.
    fn feed_accumulators(
        &mut self,
        scope: &Scope,
        repeat: NodeId,
        leaving: &[LeavingOutput],
        bounds: [[f64; 2]; 2],
    ) -> Result<(Vec<NodeId>, BTreeMap<PortRef, String>), EditError> {
        let mut accumulators = Vec::new();
        let mut names = BTreeMap::new();
        let mut puts = Vec::new();
        for (row, output) in leaving.iter().enumerate() {
            let name = accumulator_name(output, &accumulators);
            accumulators.push(Accumulator::new(
                name.clone(),
                output.value_type.clone(),
                Combine::Carry,
            ));
            if let Some(start) = self.start_value(&output.value_type) {
                self.node_mut(scope, repeat)?
                    .inputs
                    .insert(format!("init.{name}"), InputValue::Literal(start));
            }
            let row_y = bounds[0][1] + row as f64 * WRAP_NODE_HEIGHT;
            let put = self.add(
                scope,
                OP_REGION_PUT,
                Some(repeat),
                Some([bounds[1][0] + WRAP_GAP, row_y]),
            )?;
            let target = self.node_mut(scope, put)?;
            target
                .config
                .insert("acc".to_owned(), ConfigValue::String(name.clone()));
            target
                .inputs
                .insert("value".to_owned(), InputValue::Link(output.source.clone()));
            puts.push(put);
            names.insert(output.source.clone(), name);
        }
        self.set_accumulators(scope, repeat, &accumulators)?;
        Ok((puts, names))
    }

    /// Makes each node outside `wrapped` and outside the Repeat `repeat` read the accumulator in `names` of each
    /// wrapped output it read.
    fn read_accumulators(
        &mut self,
        scope: &Scope,
        repeat: NodeId,
        wrapped: &BTreeSet<NodeId>,
        names: &BTreeMap<PortRef, String>,
    ) -> Result<(), EditError> {
        let body = self.body(scope)?;
        let mut rewired = Vec::new();
        for reader in &body.nodes {
            if wrapped.contains(&reader.id) || reader.region == Some(repeat) || reader.id == repeat {
                continue;
            }
            for (port, value) in &reader.inputs {
                let InputValue::Link(link) = value else { continue };
                let Some(source) = body.node(link.node) else { continue };
                let named = named_output(&self.document, source, link);
                if let Some(name) = named.and_then(|named| names.get(&named)) {
                    rewired.push((reader.id, port.clone(), name.clone()));
                }
            }
        }
        for (reader, port, name) in rewired {
            let link = PortRef {
                node: repeat,
                port: Some(name),
            };
            self.node_mut(scope, reader)?
                .inputs
                .insert(port, InputValue::Link(link));
        }
        Ok(())
    }

    /// Returns each output of `wrapped` that a node of `body` outside it reads, once, in the order of node and port,
    /// with its type from the checker.
    pub(super) fn leaving_outputs(
        &self,
        scope: &Scope,
        body: &NodeBody,
        wrapped: &BTreeSet<NodeId>,
    ) -> Result<Vec<LeavingOutput>, EditError> {
        let mut sources: BTreeSet<PortRef> = BTreeSet::new();
        for reader in body.nodes.iter().filter(|node| !wrapped.contains(&node.id)) {
            for value in reader.inputs.values() {
                let InputValue::Link(link) = value else { continue };
                if !wrapped.contains(&link.node) {
                    continue;
                }
                let Some(source) = body.node(link.node) else { continue };
                if let Some(named) = named_output(&self.document, source, link) {
                    sources.insert(named);
                }
            }
        }
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let report = self.check_report()?;
        let outputs = body_report(&report, scope).map(|known| &known.outputs);
        sources
            .into_iter()
            .map(|source| {
                let value_type = outputs
                    .and_then(|outputs| outputs.get(&source.node))
                    .and_then(|ports| ports.get(source.port.as_deref().unwrap_or_default()))
                    .and_then(|port: &PortType| port.value_type.clone())
                    .ok_or_else(|| {
                        EditError::Invalid(format!(
                            "the type of {source} is unknown. Fix the problems of node {} first",
                            source.node
                        ))
                    })?;
                Ok(LeavingOutput { source, value_type })
            })
            .collect()
    }

    /// Returns the canvas rectangle of `nodes` from their placement, with an assumed size for each node.
    fn assumed_bounds(&self, nodes: &BTreeSet<NodeId>) -> [[f64; 2]; 2] {
        let mut bounds: Option<[[f64; 2]; 2]> = None;
        for node in nodes {
            let pos = self
                .editor
                .node_layout(*node)
                .map(|layout| layout.pos)
                .unwrap_or_default();
            let far = [pos[0] + WRAP_NODE_WIDTH, pos[1] + WRAP_NODE_HEIGHT];
            bounds = Some(match bounds {
                None => [pos, far],
                Some([min, max]) => [
                    [min[0].min(pos[0]), min[1].min(pos[1])],
                    [max[0].max(far[0]), max[1].max(far[1])],
                ],
            });
        }
        bounds.unwrap_or_default()
    }

    /// Returns the start value of a `carry` accumulator of `value_type`: zero, or the first variant of a category.
    fn start_value(&self, value_type: &ValueType) -> Option<Literal> {
        if let ValueType::Category(id) = value_type {
            let category = self.document.category(id)?;
            let first = category.variants.first()?;
            return Some(Literal::Category(MemberName {
                owner: id.clone(),
                member: first.name.clone(),
            }));
        }
        Literal::zero_of(value_type)
    }
}

/// Returns the report of the body `scope` in `report`.
pub(super) fn body_report<'a>(report: &'a CheckReport, scope: &Scope) -> Option<&'a PassReport> {
    match scope {
        Scope::Pass(id) => report.passes.iter().find(|known| known.id == *id),
        Scope::Function(id) => report.functions.iter().find(|known| known.id == *id),
    }
}

/// Returns `link` with its port named, when it reads an outer output of `source`: a port the link names, or the only
/// output of a bare link. An inner output, an unknown port or an unknown kind returns `None`.
pub(super) fn named_output(
    document: &crate::document::GraphDocument,
    source: &Node,
    link: &PortRef,
) -> Option<PortRef> {
    let def = lookup(&source.op, document.library)?;
    let outputs = def.resolved_outputs(source, document);
    let port = match &link.port {
        Some(port) => outputs.iter().find(|output| output.name == *port)?.name.clone(),
        None => match outputs.as_slice() {
            [only] => only.name.clone(),
            _ => return None,
        },
    };
    Some(PortRef {
        node: link.node,
        port: Some(port),
    })
}

/// Returns the name of the accumulator of `output`: the port's name, or the port's name and the first number from 2
/// that no accumulator in `taken` holds.
fn accumulator_name(output: &LeavingOutput, taken: &[Accumulator]) -> String {
    let port = output.source.port.clone().unwrap_or_else(|| "value".to_owned());
    let base = if matches!(port.as_str(), "k" | "total") || !is_port_segment(&port) {
        format!("{port}_value").replace('.', "_")
    } else {
        port
    };
    let free = |name: &str| !taken.iter().any(|known| known.name == name);
    if free(&base) {
        return base;
    }
    (2..)
        .map(|number| format!("{base}_{number}"))
        .find(|name| free(name))
        .unwrap_or(base)
}

/// Returns the subtree of `node` in `body`: the node and every node whose region path holds it.
pub(super) fn subtree(body: &NodeBody, node: NodeId) -> BTreeSet<NodeId> {
    let mut members: BTreeSet<NodeId> = body.region_contents(node).into_iter().collect();
    members.insert(node);
    members
}

/// Returns an error when a new Repeat around `wrapped`, in the region `parent` of `body`, would nest where library 1
/// allows no nesting.
///
/// The Repeat sits in `parent`, so `parent` must be a Repeat or a neighborhood fold. Each region of `wrapped` whose own
/// region is outside `wrapped` comes to sit in the Repeat, so it must be a Repeat or a neighborhood fold. Every region
/// of `wrapped` moves one level deeper, and the deepest must stay within [`MAX_REGION_DEPTH`].
fn check_wrap_nesting(
    body: &NodeBody,
    library: u32,
    parent: Option<NodeId>,
    wrapped: &BTreeSet<NodeId>,
) -> Result<(), EditError> {
    let nests = |op: &str| op == OP_REGION_REPEAT || op == OP_GRID_FOLD;
    if let Some(outer) = parent.and_then(|parent| body.node(parent))
        && !nests(&outer.op)
    {
        return Err(EditError::Invalid(format!(
            "a Repeat cannot sit in the region {}. A Repeat sits in a Repeat or a neighborhood fold",
            outer.id
        )));
    }
    let is_region = |node: &Node| lookup(&node.op, library).is_some_and(|def| def.is_region);
    // The new Repeat's own depth, counted from 1 at the top of the body.
    let mut deepest = parent.map_or(0, |parent| body.region_path(parent).len() + 1) + 1;
    for node in body
        .nodes
        .iter()
        .filter(|node| wrapped.contains(&node.id) && is_region(node))
    {
        let joins_repeat = node.region.is_none_or(|region| !wrapped.contains(&region));
        if joins_repeat && !nests(&node.op) {
            return Err(EditError::Invalid(format!(
                "node {} cannot sit in a Repeat. A Repeat holds Repeats and neighborhood folds",
                node.id
            )));
        }
        deepest = deepest.max(body.region_path(node.id).len() + 2);
    }
    if deepest > MAX_REGION_DEPTH {
        return Err(EditError::Invalid(format!(
            "the Repeat would nest regions {deepest} deep, and regions nest at most {MAX_REGION_DEPTH} deep"
        )));
    }
    Ok(())
}
