//! Edits of functions: their declarations and ports, making a function from nodes of a body, ungrouping a call, and
//! importing a function from another model file.
//!
//! A *call* is a `fn.call` node, whose inputs and outputs are its function's ports. Making a function moves nodes into
//! a new function and leaves one call in their place. Ungrouping a call puts a copy of its function's body in place of
//! the call. An *import* copies a function, and every function it calls, from another file.

use std::collections::{BTreeMap, BTreeSet};

use henad_core::explore::fingerprint::fnv1a64;

use super::declarations::node_place;
use super::regions::{LeavingOutput, Selection, body_report, named_output};
use super::{EditCommand, EditError, GraphBuilder, Scope};
use crate::catalogue::{Effect, NodeDef, lookup};
use crate::check::CheckReport;
use crate::document::{Function, FunctionPort, GraphDocument};
use crate::editor::{FunctionOrigin, NodeLayout};
use crate::file::GraphFile;
use crate::ids::{NodeId, is_port_segment};
use crate::node::{Node, NodeBody, OP_FUNCTION_CALL, OP_FUNCTION_INPUT, OP_FUNCTION_OUTPUT, OP_REGION_PUT};
use crate::normal::compose_key;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, PortRef};

/// Distance in canvas points between moved nodes and the new `fn.input` and `fn.output` nodes beside them.
const PORT_GAP: f64 = 280.0;

/// Height in canvas points of the row of one new `fn.input` or `fn.output` node.
const PORT_ROW: f64 = 90.0;

/// Side of a function's interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PortSide {
    /// Inputs, which each call writes and `fn.input` nodes read.
    Input,
    /// Outputs, which `fn.output` nodes set and the call's readers read.
    Output,
}

impl PortSide {
    /// Returns the word a message names the side by.
    fn word(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }

    /// Returns the kind of the body node that names a port of the side.
    fn op(self) -> &'static str {
        match self {
            Self::Input => OP_FUNCTION_INPUT,
            Self::Output => OP_FUNCTION_OUTPUT,
        }
    }
}

impl GraphBuilder {
    /// Runs one of the function commands on the builder in place.
    pub(super) fn run_function(&mut self, command: EditCommand) -> Result<Vec<NodeId>, EditError> {
        match command {
            EditCommand::AddFunction(id) => self.add_function(&id).map(|()| Vec::new()),
            EditCommand::RenameFunction { from, to } => self.rename_function(&from, &to).map(|()| Vec::new()),
            EditCommand::RemoveFunction(id) => self.remove_function(&id).map(|()| Vec::new()),
            EditCommand::AddFunctionPort {
                function,
                side,
                port,
                value_type,
            } => self
                .add_function_port(&function, side, &port, value_type)
                .map(|()| Vec::new()),
            EditCommand::RenameFunctionPort {
                function,
                side,
                from,
                to,
            } => self
                .rename_function_port(&function, side, &from, &to)
                .map(|()| Vec::new()),
            EditCommand::SetFunctionPortType {
                function,
                side,
                port,
                value_type,
            } => {
                let target = self.port_mut(&function, side, &port)?;
                target.value_type = value_type;
                Ok(Vec::new())
            }
            EditCommand::RemoveFunctionPort { function, side, port } => {
                self.remove_function_port(&function, side, &port).map(|()| Vec::new())
            }
            EditCommand::MakeFunction { scope, nodes, id } => self.make_function(&scope, &nodes, &id),
            EditCommand::Ungroup { scope, node } => self.ungroup(&scope, node),
            EditCommand::ImportFunction {
                text,
                source,
                function,
                id,
            } => self.import_function(&text, &source, &function, id.as_deref()),
            other => Err(EditError::Invalid(format!("{other:?} is no function command"))),
        }
    }

    /// Returns the position of the function `id` among the file's functions.
    fn function_position(&self, id: &str) -> Result<usize, EditError> {
        self.document
            .functions
            .iter()
            .position(|function| function.id == id)
            .ok_or_else(|| EditError::Missing(format!("the file has no function '{id}'")))
    }

    /// Returns the ports of `side` of the function `id` for writing.
    fn ports_mut(&mut self, id: &str, side: PortSide) -> Result<&mut Vec<FunctionPort>, EditError> {
        let position = self.function_position(id)?;
        let function = &mut self.document.functions[position];
        Ok(match side {
            PortSide::Input => &mut function.inputs,
            PortSide::Output => &mut function.outputs,
        })
    }

    /// Returns the port `port` of `side` of the function `id` for writing.
    fn port_mut(&mut self, id: &str, side: PortSide, port: &str) -> Result<&mut FunctionPort, EditError> {
        let missing = EditError::Missing(format!("the function '{id}' has no {} '{port}'", side.word()));
        self.ports_mut(id, side)?
            .iter_mut()
            .find(|known| known.port == port)
            .ok_or(missing)
    }

    /// Returns every call of the function `id`, with its body.
    fn calls_of(&self, id: &str) -> Vec<(Scope, NodeId)> {
        self.scoped_nodes()
            .into_iter()
            .filter(|(_, node)| node.op == OP_FUNCTION_CALL && called_id(node) == Some(id))
            .map(|(scope, node)| (scope, node.id))
            .collect()
    }

    /// Adds an empty function `id` with no ports after the file's functions.
    fn add_function(&mut self, id: &str) -> Result<(), EditError> {
        Self::check_new_id(
            "function",
            id,
            self.document.functions.iter().map(|function| function.id.as_str()),
        )?;
        self.document.functions.push(Function {
            id: id.to_owned(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            body: NodeBody::default(),
        });
        Ok(())
    }

    /// Renames the function `from` to `to`, every call of it, its canvas and its editor entry.
    fn rename_function(&mut self, from: &str, to: &str) -> Result<(), EditError> {
        let position = self.function_position(from)?;
        Self::check_new_id(
            "function",
            to,
            self.document.functions.iter().map(|function| function.id.as_str()),
        )?;
        self.document.functions[position].id = to.to_owned();
        self.for_each_node(|node| {
            if node.op == OP_FUNCTION_CALL && called_id(node) == Some(from) {
                node.config
                    .insert("function".to_owned(), ConfigValue::String(to.to_owned()));
            }
        });
        self.editor.rename_function(from, to);
        Ok(())
    }

    /// Removes the function `id`, its nodes' placement and its editor entry, when no call names it.
    fn remove_function(&mut self, id: &str) -> Result<(), EditError> {
        let position = self.function_position(id)?;
        let references: Vec<String> = self
            .calls_of(id)
            .iter()
            .map(|(scope, node)| node_place(scope, *node))
            .collect();
        Self::refuse_referenced(format!("the function '{id}'"), references)?;
        let function = self.document.functions.remove(position);
        for node in &function.body.nodes {
            self.editor.remove_node(node.id);
        }
        self.editor.remove_function(id);
        Ok(())
    }

    /// Adds the port `port` of `value_type` after the ports of `side` of the function `id`.
    ///
    /// A function that gains a second output first names the port on every bare link to each of its calls.
    fn add_function_port(
        &mut self,
        id: &str,
        side: PortSide,
        port: &str,
        value_type: ValueType,
    ) -> Result<(), EditError> {
        check_port_name(id, side, port, self.ports_mut(id, side)?)?;
        if side == PortSide::Output {
            for (scope, call) in self.calls_of(id) {
                self.name_bare_links(&scope, call);
            }
        }
        self.ports_mut(id, side)?.push(FunctionPort {
            port: port.to_owned(),
            value_type,
        });
        Ok(())
    }

    /// Renames the port `from` of `side` of the function `id`, the body nodes that name it, and each call's input or
    /// each link and statistic that reads it.
    fn rename_function_port(&mut self, id: &str, side: PortSide, from: &str, to: &str) -> Result<(), EditError> {
        self.port_mut(id, side, from)?;
        check_port_name(id, side, to, self.ports_mut(id, side)?)?;
        self.port_mut(id, side, from)?.port = to.to_owned();
        let position = self.function_position(id)?;
        for node in &mut self.document.functions[position].body.nodes {
            if node.op == side.op() && node.config.get("port").and_then(ConfigValue::as_str) == Some(from) {
                node.config
                    .insert("port".to_owned(), ConfigValue::String(to.to_owned()));
            }
        }
        for (scope, call) in self.calls_of(id) {
            match side {
                PortSide::Input => {
                    let target = self.node_mut(&scope, call)?;
                    if let Some(value) = target.inputs.remove(from) {
                        target.inputs.insert(to.to_owned(), value);
                    }
                }
                PortSide::Output => {
                    for reader in &mut self.body_mut(&scope)?.nodes {
                        for value in reader.inputs.values_mut() {
                            if let InputValue::Link(link) = value
                                && link.node == call
                                && link.port.as_deref() == Some(from)
                            {
                                link.port = Some(to.to_owned());
                            }
                        }
                    }
                    if matches!(scope, Scope::Pass(_)) {
                        for stat in &mut self.document.stats {
                            if stat.from.node == call && stat.from.port.as_deref() == Some(from) {
                                stat.from.port = Some(to.to_owned());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Removes the port `port` of `side` of the function `id`, when no body node names it and nothing reads it. A
    /// removed input leaves every call.
    fn remove_function_port(&mut self, id: &str, side: PortSide, port: &str) -> Result<(), EditError> {
        self.port_mut(id, side, port)?;
        let scope = Scope::Function(id.to_owned());
        let position = self.function_position(id)?;
        let mut references: Vec<String> = self.document.functions[position]
            .body
            .nodes
            .iter()
            .filter(|node| node.op == side.op() && node.config.get("port").and_then(ConfigValue::as_str) == Some(port))
            .map(|node| node_place(&scope, node.id))
            .collect();
        let only = self.document.functions[position].outputs.len() == 1;
        if side == PortSide::Output {
            for (call_scope, call) in self.calls_of(id) {
                let reads = |link: &PortRef| {
                    link.node == call && (link.port.as_deref() == Some(port) || (link.port.is_none() && only))
                };
                for reader in &self.body(&call_scope)?.nodes {
                    if reader
                        .inputs
                        .values()
                        .any(|value| matches!(value, InputValue::Link(link) if reads(link)))
                    {
                        references.push(node_place(&call_scope, reader.id));
                    }
                }
                if matches!(call_scope, Scope::Pass(_)) {
                    for stat in self.document.stats.iter().filter(|stat| reads(&stat.from)) {
                        references.push(format!("the statistic '{}'", stat.label));
                    }
                }
            }
        }
        Self::refuse_referenced(
            format!("the {} '{port}' of the function '{id}'", side.word()),
            references,
        )?;
        self.ports_mut(id, side)?.retain(|known| known.port != port);
        if side == PortSide::Input {
            for (call_scope, call) in self.calls_of(id) {
                self.node_mut(&call_scope, call)?.inputs.remove(port);
            }
        }
        Ok(())
    }

    /// Moves `nodes` of `scope`, with every node inside them, into a new function `id`, and returns the ids of the
    /// nodes it makes: the call that takes their place, then the function's `fn.input` and `fn.output` nodes.
    ///
    /// Each value the nodes read from outside them becomes an input, and each of their outputs that a node outside
    /// reads becomes an output. Their draws take the call's key on top of their own, so they draw other numbers.
    #[expect(
        clippy::too_many_lines,
        reason = "the steps of one edit, in the order they depend on"
    )]
    fn make_function(&mut self, scope: &Scope, nodes: &[NodeId], id: &str) -> Result<Vec<NodeId>, EditError> {
        Self::check_new_id(
            "function",
            id,
            self.document.functions.iter().map(|function| function.id.as_str()),
        )?;
        let Selection { tops, parent, wrapped } = self.selection(scope, nodes, "a function")?;
        self.refuse_unmovable(scope, &wrapped)?;
        let body = self.body(scope)?;
        let leaving = self.leaving_outputs(scope, body, &wrapped)?;
        let report = self.check_report()?;
        let entering = self.entering_values(scope, &report, &wrapped)?;
        let bounds = self.placed_bounds(&wrapped);

        let call = self.mint_id()?;
        let key = self.mint_key();
        let mut made = vec![call];
        let mut function = Function {
            id: id.to_owned(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            body: NodeBody::default(),
        };
        let mut call_node = Node::new(call, OP_FUNCTION_CALL);
        call_node.region = parent;
        call_node
            .config
            .insert("function".to_owned(), ConfigValue::String(id.to_owned()));
        call_node
            .config
            .insert("key".to_owned(), ConfigValue::Integer(i64::from(key)));

        let mut input_nodes: BTreeMap<PortRef, NodeId> = BTreeMap::new();
        let mut taken: Vec<String> = Vec::new();
        let mut new_nodes = Vec::new();
        for (row, (source, value_type)) in entering.iter().enumerate() {
            let port = port_name(source.port.as_deref(), &taken);
            taken.push(port.clone());
            let node = self.mint_id()?;
            made.push(node);
            let mut input = Node::new(node, OP_FUNCTION_INPUT);
            input
                .config
                .insert("port".to_owned(), ConfigValue::String(port.clone()));
            new_nodes.push(input);
            self.editor
                .set_node_pos(node, [bounds[0][0] - PORT_GAP, bounds[0][1] + row as f64 * PORT_ROW]);
            function.inputs.push(FunctionPort {
                port: port.clone(),
                value_type: value_type.clone(),
            });
            call_node.inputs.insert(port, InputValue::Link(source.clone()));
            input_nodes.insert(source.clone(), node);
        }
        let mut outputs: BTreeMap<PortRef, String> = BTreeMap::new();
        let mut taken: Vec<String> = Vec::new();
        for (row, LeavingOutput { source, value_type }) in leaving.iter().enumerate() {
            let port = port_name(source.port.as_deref(), &taken);
            taken.push(port.clone());
            let node = self.mint_id()?;
            made.push(node);
            let mut output = Node::new(node, OP_FUNCTION_OUTPUT);
            output
                .config
                .insert("port".to_owned(), ConfigValue::String(port.clone()));
            output
                .inputs
                .insert("value".to_owned(), InputValue::Link(source.clone()));
            new_nodes.push(output);
            self.editor.set_node_pos(
                node,
                [bounds[1][0] + PORT_GAP / 2.0, bounds[0][1] + row as f64 * PORT_ROW],
            );
            function.outputs.push(FunctionPort {
                port: port.clone(),
                value_type: value_type.clone(),
            });
            outputs.insert(source.clone(), port);
        }

        // Each moved node reads the function's input in place of a value from outside, and each node left behind reads
        // the call in place of a moved node's output, as (position in the body, input, new link).
        let body = self.body(scope)?;
        let mut rewired: Vec<(usize, String, PortRef)> = Vec::new();
        for (position, reader) in body.nodes.iter().enumerate() {
            for (port, value) in &reader.inputs {
                let InputValue::Link(link) = value else { continue };
                let new_link = if wrapped.contains(&reader.id) {
                    if wrapped.contains(&link.node) {
                        continue;
                    }
                    entering_key(&self.document, &body.nodes, link)
                        .and_then(|source| input_nodes.get(&source))
                        .map(|input| PortRef {
                            node: *input,
                            port: Some("value".to_owned()),
                        })
                } else {
                    let Some(source) = body.node(link.node).filter(|source| wrapped.contains(&source.id)) else {
                        continue;
                    };
                    named_output(&self.document, source, link)
                        .and_then(|named| outputs.get(&named))
                        .map(|port| PortRef {
                            node: call,
                            port: Some(port.clone()),
                        })
                };
                if let Some(new_link) = new_link {
                    rewired.push((position, port.clone(), new_link));
                }
            }
        }
        let body = self.body_mut(scope)?;
        for (position, port, new_link) in rewired {
            body.nodes[position].inputs.insert(port, InputValue::Link(new_link));
        }
        let (mut moved, kept): (Vec<Node>, Vec<Node>) = std::mem::take(&mut body.nodes)
            .into_iter()
            .partition(|node| wrapped.contains(&node.id));
        body.nodes = kept;
        for node in &mut moved {
            if tops.contains(&node.id) {
                node.region = None;
            }
        }
        body.nodes.push(call_node);
        moved.extend(new_nodes);
        function.body.nodes = moved;
        self.document.functions.push(function);
        self.editor.set_node_pos(call, bounds[0]);
        let wrapped: Vec<NodeId> = wrapped.into_iter().collect();
        self.editor.move_frame_members(
            &scope.canvas(),
            &wrapped,
            &Scope::Function(id.to_owned()).canvas(),
            call,
        );
        Ok(made)
    }

    /// Returns an error when a node of `wrapped` cannot move into a function: an effect a function cannot hold, a put
    /// into a region outside them, a function's own input or output, or a node a statistic reads.
    fn refuse_unmovable(&self, scope: &Scope, wrapped: &BTreeSet<NodeId>) -> Result<(), EditError> {
        let body = self.body(scope)?;
        for node in body.nodes.iter().filter(|node| wrapped.contains(&node.id)) {
            let def = lookup(&node.op, self.document.library);
            if let Some(what) = def.and_then(NodeDef::sink) {
                return Err(EditError::Invalid(format!(
                    "node {} is {what} and cannot be in a function. Remove it from the selection",
                    node.id
                )));
            }
            if node.op == OP_REGION_PUT && node.region.is_none_or(|region| !wrapped.contains(&region)) {
                return Err(EditError::Invalid(format!(
                    "the put {} feeds a region outside the selection. Select the region too",
                    node.id
                )));
            }
            if node.op == OP_FUNCTION_INPUT || node.op == OP_FUNCTION_OUTPUT {
                return Err(EditError::Invalid(format!(
                    "node {} is an input or output of this function. Remove it from the selection",
                    node.id
                )));
            }
            if matches!(scope, Scope::Pass(_)) && self.document.stats.iter().any(|stat| stat.from.node == node.id) {
                return Err(EditError::Invalid(format!(
                    "a statistic reads node {}. Remove it from the selection",
                    node.id
                )));
            }
        }
        Ok(())
    }

    /// Returns each value that a node of `wrapped` reads from a node of `scope` outside it, once, in the order of the
    /// nodes and their inputs, with its type from `report`.
    fn entering_values(
        &self,
        scope: &Scope,
        report: &CheckReport,
        wrapped: &BTreeSet<NodeId>,
    ) -> Result<Vec<(PortRef, ValueType)>, EditError> {
        let body = self.body(scope)?;
        let types = body_report(report, scope);
        let mut entering: Vec<(PortRef, ValueType)> = Vec::new();
        for reader in body.nodes.iter().filter(|node| wrapped.contains(&node.id)) {
            for value in reader.inputs.values() {
                let InputValue::Link(link) = value else { continue };
                if wrapped.contains(&link.node) {
                    continue;
                }
                let source = entering_key(&self.document, &body.nodes, link).ok_or_else(|| {
                    EditError::Invalid(format!(
                        "node {} reads unknown output {link}. Fix the problems of node {} first",
                        reader.id, reader.id
                    ))
                })?;
                if entering.iter().any(|(known, _)| *known == source) {
                    continue;
                }
                let port = source.port.as_deref().unwrap_or_default();
                let value_type = types
                    .and_then(|types| {
                        types
                            .outputs
                            .get(&source.node)
                            .and_then(|ports| ports.get(port))
                            .or_else(|| types.inner_outputs.get(&source.node)?.get(port))
                    })
                    .and_then(|port| port.value_type.clone())
                    .ok_or_else(|| {
                        EditError::Invalid(format!(
                            "the type of {source} is unknown. Fix the problems of node {} first",
                            source.node
                        ))
                    })?;
                entering.push((source, value_type));
            }
        }
        Ok(entering)
    }

    /// Returns the canvas rectangle of the placed nodes of `nodes`, `[min, max]` of their positions.
    fn placed_bounds(&self, nodes: &BTreeSet<NodeId>) -> [[f64; 2]; 2] {
        nodes
            .iter()
            .filter_map(|node| self.editor.node_pos(*node))
            .map(|pos| [pos, pos])
            .reduce(|left, right| {
                [
                    [left[0][0].min(right[0][0]), left[0][1].min(right[0][1])],
                    [left[1][0].max(right[1][0]), left[1][1].max(right[1][1])],
                ]
            })
            .unwrap_or_default()
    }

    /// Replaces the call `node` of `scope` with a copy of its function's body, and returns the ids of the copies.
    ///
    /// Each copy takes a fresh id, sits where the call sat, and reads the call's inputs where the body read the
    /// function's inputs. Each reader of the call reads the value the function's output took. A draw's key becomes the
    /// key it took inside the call, so the copy draws what the call drew. A call inside the body keeps its own key
    /// composed with the call's, and the draws inside it draw other numbers.
    #[expect(
        clippy::too_many_lines,
        reason = "the steps of one edit, in the order they depend on"
    )]
    fn ungroup(&mut self, scope: &Scope, node: NodeId) -> Result<Vec<NodeId>, EditError> {
        let call = self
            .body(scope)?
            .node(node)
            .ok_or_else(|| EditError::Missing(format!("the {scope} has no node {node}")))?
            .clone();
        if call.op != OP_FUNCTION_CALL {
            return Err(EditError::Invalid(format!("node {node} is not a function call")));
        }
        let function_id = called_id(&call).unwrap_or_default().to_owned();
        let position = self.function_position(&function_id)?;
        let function = self.document.functions[position].clone();
        let call_key = call
            .config
            .get("key")
            .and_then(ConfigValue::as_integer)
            .and_then(|key| u32::try_from(key).ok())
            .unwrap_or(0);

        let mut inputs: BTreeMap<NodeId, InputValue> = BTreeMap::new();
        let mut fresh: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        for inner in &function.body.nodes {
            if inner.op == OP_FUNCTION_INPUT {
                let port = inner
                    .config
                    .get("port")
                    .and_then(ConfigValue::as_str)
                    .unwrap_or_default();
                let value = call.inputs.get(port).cloned().ok_or_else(|| {
                    EditError::Invalid(format!(
                        "the input `{port}` of the call {node} is not set. Wire it first"
                    ))
                })?;
                inputs.insert(inner.id, value);
            } else if inner.op != OP_FUNCTION_OUTPUT {
                fresh.insert(inner.id, self.mint_id()?);
            }
        }
        let map_value = |value: &InputValue| -> InputValue {
            match value {
                InputValue::Link(link) => {
                    if let Some(input) = inputs.get(&link.node) {
                        input.clone()
                    } else if let Some(&id) = fresh.get(&link.node) {
                        InputValue::Link(PortRef {
                            node: id,
                            port: link.port.clone(),
                        })
                    } else {
                        value.clone()
                    }
                }
                other => other.clone(),
            }
        };
        let mut outputs: BTreeMap<String, InputValue> = BTreeMap::new();
        for inner in function
            .body
            .nodes
            .iter()
            .filter(|inner| inner.op == OP_FUNCTION_OUTPUT)
        {
            let port = inner
                .config
                .get("port")
                .and_then(ConfigValue::as_str)
                .unwrap_or_default();
            if let Some(value) = inner.inputs.get("value") {
                outputs.insert(port.to_owned(), map_value(value));
            }
        }

        let library = self.document.library;
        let origin = self.placed_bounds(&function.body.nodes.iter().map(|inner| inner.id).collect())[0];
        let at = self
            .editor
            .node_layout(node)
            .map(|layout| layout.pos)
            .unwrap_or_default();
        let mut copies = Vec::new();
        for inner in &function.body.nodes {
            let Some(&id) = fresh.get(&inner.id) else { continue };
            let mut copy = inner.clone();
            copy.id = id;
            copy.region = match inner.region {
                Some(region) => fresh.get(&region).copied(),
                None => call.region,
            };
            for value in copy.inputs.values_mut() {
                *value = map_value(value);
            }
            let def = lookup(&inner.op, library);
            let keyed = inner.op == OP_FUNCTION_CALL || def.is_some_and(|def| def.has_effect(Effect::DrawsCounter));
            if keyed && let Some(key) = inner.config.get("key").and_then(ConfigValue::as_integer) {
                let key = u32::try_from(key).unwrap_or(0);
                copy.config.insert(
                    "key".to_owned(),
                    ConfigValue::Integer(i64::from(compose_key(call_key, key))),
                );
            }
            if let Some(layout) = self.editor.node_layout(inner.id) {
                let shift = |pos: [f64; 2]| [pos[0] - origin[0] + at[0], pos[1] - origin[1] + at[1]];
                self.editor.place_node(
                    id,
                    &NodeLayout {
                        pos: shift(layout.pos),
                        output_pos: layout.output_pos.map(shift),
                        ..layout
                    },
                );
            }
            copies.push(copy);
        }

        let only = function.outputs.len() == 1;
        let read = |link: &PortRef| -> Option<InputValue> {
            if link.node != node {
                return None;
            }
            match &link.port {
                Some(port) => outputs.get(port).cloned(),
                None if only => outputs.values().next().cloned(),
                None => None,
            }
        };
        if matches!(scope, Scope::Pass(_)) {
            let stats = self.document.stats.iter().filter(|stat| stat.from.node == node);
            for stat in stats {
                match read(&stat.from) {
                    Some(InputValue::Link(_)) => {}
                    _ => {
                        return Err(EditError::Invalid(format!(
                            "the statistic '{}' reads an output of the call {node} that is not an output of any node",
                            stat.label
                        )));
                    }
                }
            }
            for stat in &mut self.document.stats {
                if let Some(InputValue::Link(link)) = read(&stat.from) {
                    stat.from = link;
                }
            }
        }
        let body = self.body_mut(scope)?;
        body.nodes.retain(|known| known.id != node);
        for reader in &mut body.nodes {
            for value in reader.inputs.values_mut() {
                if let InputValue::Link(link) = &*value
                    && link.node == node
                {
                    match read(link) {
                        Some(found) => *value = found,
                        None => {
                            return Err(EditError::Invalid(format!(
                                "node {} reads {link}, which the function '{function_id}' does not set",
                                reader.id
                            )));
                        }
                    }
                }
            }
            if reader.region == Some(node) {
                reader.region = call.region;
            }
        }
        let made: Vec<NodeId> = copies.iter().map(|copy| copy.id).collect();
        body.nodes.extend(copies);
        self.editor.remove_node(node);
        Ok(made)
    }

    /// Copies the function `function` of the model file `text`, and every function it calls, into the file under
    /// `id` or its own id, and records `source` and each function's hash as its origin.
    fn import_function(
        &mut self,
        text: &str,
        source: &str,
        function: &str,
        id: Option<&str>,
    ) -> Result<Vec<NodeId>, EditError> {
        let file = GraphFile::parse(text).map_err(EditError::Import)?;
        let theirs = file.document();
        if theirs.library > self.document.library {
            return Err(EditError::Invalid(format!(
                "the file to import from uses node library {}, but this file uses {}",
                theirs.library, self.document.library
            )));
        }
        let names = imported_functions(theirs, function)?;
        let renamed = id.unwrap_or(function);
        if renamed != function && names.iter().any(|name| name == renamed) {
            return Err(EditError::Invalid(format!(
                "the function '{function}' calls a function named '{renamed}'. Import it under another name"
            )));
        }
        for name in &names {
            let target = if name == function { renamed } else { name.as_str() };
            Self::check_new_id(
                "function",
                target,
                self.document.functions.iter().map(|known| known.id.as_str()),
            )
            .map_err(|_taken| {
                EditError::Invalid(format!(
                    "the file already has the function '{target}'. Import it under another name"
                ))
            })?;
        }
        let mut made = Vec::new();
        for name in &names {
            let Some(original) = theirs.functions.iter().find(|known| known.id == *name) else {
                continue;
            };
            let hash = format!(
                "{:016x}",
                fnv1a64(crate::writer::function_text(theirs, original).as_bytes())
            );
            let mut copy = original.clone();
            if name == function {
                renamed.clone_into(&mut copy.id);
            }
            let mut fresh: BTreeMap<NodeId, NodeId> = BTreeMap::new();
            for node in &copy.body.nodes {
                fresh.insert(node.id, self.mint_id()?);
            }
            for node in &mut copy.body.nodes {
                let old = node.id;
                node.id = fresh[&old];
                node.region = node.region.and_then(|region| fresh.get(&region).copied());
                for value in node.inputs.values_mut() {
                    if let InputValue::Link(link) = value
                        && let Some(&id) = fresh.get(&link.node)
                    {
                        link.node = id;
                    }
                }
                if node.op == OP_FUNCTION_CALL && called_id(node) == Some(function) {
                    node.config
                        .insert("function".to_owned(), ConfigValue::String(renamed.to_owned()));
                }
                if let Some(layout) = file.editor().and_then(|editor| editor.node_layout(old)) {
                    self.editor.place_node(node.id, &layout);
                }
                made.push(node.id);
            }
            self.editor.set_function_origin(
                &copy.id,
                &FunctionOrigin {
                    name: source.to_owned(),
                    hash,
                },
            );
            self.document.functions.push(copy);
        }
        Ok(made)
    }
}

/// Returns the id of the function `node` calls.
fn called_id(node: &Node) -> Option<&str> {
    node.config.get("function").and_then(ConfigValue::as_str)
}

/// Returns an error unless `port` can name a new port of `side` of the function `id`: a port name no port in `ports`
/// holds.
fn check_port_name(id: &str, side: PortSide, port: &str, ports: &[FunctionPort]) -> Result<(), EditError> {
    if !is_port_segment(port) {
        return Err(EditError::Invalid(format!(
            "invalid port name '{port}', expected lowercase letters, digits and `_`, starting with a lowercase letter"
        )));
    }
    if ports.iter().any(|known| known.port == port) {
        return Err(EditError::Invalid(format!(
            "the function '{id}' already has the {} '{port}'",
            side.word()
        )));
    }
    Ok(())
}

/// Returns the name of a new port for an output named `port`: the name, or the name and the first number from 2 that
/// `taken` lacks.
fn port_name(port: Option<&str>, taken: &[String]) -> String {
    let base = port
        .filter(|port| is_port_segment(port))
        .map_or_else(|| "value".to_owned(), str::to_owned)
        .replace('.', "_");
    let free = |name: &str| !taken.iter().any(|known| known == name);
    if free(&base) {
        return base;
    }
    (2..)
        .map(|number| format!("{base}_{number}"))
        .find(|name| free(name))
        .unwrap_or(base)
}

/// Returns the output `link` reads among `nodes`, with its port named: an outer output, or an inner output of a
/// region, which a link always names.
fn entering_key(document: &GraphDocument, nodes: &[Node], link: &PortRef) -> Option<PortRef> {
    let source = nodes.iter().find(|node| node.id == link.node)?;
    named_output(document, source, link).or_else(|| link.port.as_ref().map(|_| link.clone()))
}

/// Returns `function` and every function it calls in `document`, each once, `function` first.
fn imported_functions(document: &GraphDocument, function: &str) -> Result<Vec<String>, EditError> {
    let mut names = vec![function.to_owned()];
    let mut seen: BTreeSet<String> = BTreeSet::from([function.to_owned()]);
    let mut next = 0;
    while let Some(name) = names.get(next).cloned() {
        next += 1;
        let found = document
            .functions
            .iter()
            .find(|known| known.id == name)
            .ok_or_else(|| EditError::Missing(format!("the file to import from has no function '{name}'")))?;
        for node in found.body.nodes.iter().filter(|node| node.op == OP_FUNCTION_CALL) {
            if let Some(called) = called_id(node)
                && seen.insert(called.to_owned())
            {
                names.push(called.to_owned());
            }
        }
    }
    Ok(names)
}
