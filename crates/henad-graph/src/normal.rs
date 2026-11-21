//! Normalisation of a graph: each expression node expanded and each function call inlined, so the host evaluator and
//! the compiler see only primitive kinds.
//!
//! A *copy* is a node normalisation adds: a node of an expression's expansion, or a node of a function body copied in
//! place of a call. Each copy takes a fresh id past every id of the file, and keeps its *origin*, the calls it was
//! copied through and the node it was copied from, so a problem found in a copy is reported at the node a user
//! wrote. Inside a call, the key `k` of every draw becomes `fold32(mix_seed((call key << 32) | k))`, once per
//! enclosing call, innermost first.

use std::collections::{BTreeMap, VecDeque};

use henad_core::authoring::primitives::rng::mix_seed;

use crate::catalogue::{Effect, called_function, lookup};
use crate::document::{Function, GraphDocument};
use crate::draw::fold32;
use crate::expr;
use crate::ids::NodeId;
use crate::limits::{MAX_CALL_DEPTH, MAX_NODES_INLINED};
use crate::node::{Node, NodeBody, OP_EXPRESSION, OP_FUNCTION_CALL, OP_FUNCTION_INPUT, OP_FUNCTION_OUTPUT};
use crate::value::{ConfigValue, InputValue, PortRef};

/// Where a copy comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Origin {
    /// Calls the copy was inlined through, by their ids as written, the call in the pass first.
    pub calls: Vec<NodeId>,
    /// Function holding [`Self::node`], `None` for a node of the pass itself.
    pub function: Option<String>,
    /// Node as written that the copy comes from: the node of a function body, or the expression node.
    pub node: NodeId,
}

impl Origin {
    /// Returns the node as written the copy comes from: the outermost call, or [`Self::node`] when there is none.
    pub(crate) fn written(&self) -> NodeId {
        self.calls.first().copied().unwrap_or(self.node)
    }
}

/// One pass after normalisation.
#[derive(Debug, Clone)]
pub(crate) struct NormalPass {
    /// Nodes of the pass: its own nodes but its calls and expressions, then the copies.
    pub body: NodeBody,
    /// Origin of each copy, by its id.
    pub origins: BTreeMap<NodeId, Origin>,
    /// Value of each output of each call and expression the pass replaced, by node and port, links resolved.
    pub replaced: BTreeMap<NodeId, BTreeMap<String, InputValue>>,
}

/// Reason normalisation stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalError {
    /// Node as written in the pass, a call or an expression.
    pub node: NodeId,
    /// What went wrong.
    pub reason: String,
}

/// Returns the first id past every node of `document`.
pub(crate) fn first_free_id(document: &GraphDocument) -> u32 {
    document
        .passes
        .iter()
        .map(|pass| &pass.body)
        .chain(document.functions.iter().map(|function| &function.body))
        .flat_map(|body| body.nodes.iter())
        .map(|node| node.id.number())
        .max()
        .map_or(1, |highest| highest.saturating_add(1))
}

/// Returns `body` with every call and expression replaced by its copies, numbering copies from `*next`.
///
/// # Errors
///
/// Returns the error of the first call or expression that cannot be replaced: a call of a function the document
/// does not declare, a call that leaves a function input unwritten, a call chain deeper than the limit, and an
/// expression that does not expand.
pub(crate) fn normalise_body(
    document: &GraphDocument,
    body: &NodeBody,
    next: &mut u32,
) -> Result<NormalPass, NormalError> {
    let mut pending: VecDeque<Entry> = body
        .nodes
        .iter()
        .map(|node| Entry {
            node: node.clone(),
            origin: None,
            keys: Vec::new(),
        })
        .collect();
    let mut done: Vec<Entry> = Vec::with_capacity(pending.len());
    let mut replaced: BTreeMap<NodeId, BTreeMap<String, InputValue>> = BTreeMap::new();
    let mut added = 0usize;
    while let Some(entry) = pending.pop_front() {
        let Entry { node, origin, keys } = &entry;
        if node.op != OP_FUNCTION_CALL && node.op != OP_EXPRESSION {
            done.push(entry);
            continue;
        }
        let fail = |reason: String| NormalError {
            node: entry.written_id(),
            reason,
        };
        let (copies, outputs) = if node.op == OP_EXPRESSION {
            let expansion = expr::expand(node, next).map_err(|reason| fail(format!("expression {reason}")))?;
            let origin = origin.clone().unwrap_or(Origin {
                calls: Vec::new(),
                function: None,
                node: node.id,
            });
            let copies: Vec<Entry> = expansion
                .nodes
                .into_iter()
                .map(|copy| Entry {
                    node: copy,
                    origin: Some(origin.clone()),
                    keys: keys.clone(),
                })
                .collect();
            (copies, expansion.outputs)
        } else {
            let function =
                called_function(node, document).ok_or_else(|| fail("calls an undeclared function".to_owned()))?;
            let mut calls = origin.as_ref().map(|origin| origin.calls.clone()).unwrap_or_default();
            calls.push(origin.as_ref().map_or(node.id, |origin| origin.node));
            if calls.len() > MAX_CALL_DEPTH {
                return Err(fail(format!("nests calls deeper than {MAX_CALL_DEPTH} functions")));
            }
            let mut keys = keys.clone();
            keys.push(key_of(node));
            inline(node, function, &calls, &keys, next).map_err(fail)?
        };
        added = added.saturating_add(copies.len());
        if added > MAX_NODES_INLINED {
            return Err(fail(format!("expands to more than {MAX_NODES_INLINED} nodes")));
        }
        replaced.insert(node.id, outputs);
        pending.extend(copies);
    }
    let mut nodes = Vec::with_capacity(done.len());
    let mut origins = BTreeMap::new();
    for Entry { mut node, origin, keys } in done {
        for value in node.inputs.values_mut() {
            resolve(value, &replaced);
        }
        if !keys.is_empty()
            && lookup(&node.op, document.library).is_some_and(|def| def.has_effect(Effect::DrawsCounter))
        {
            let composed = keys
                .iter()
                .rev()
                .fold(key_of(&node), |inner, call| compose_key(*call, inner));
            node.config
                .insert("key".to_owned(), ConfigValue::Integer(i64::from(composed)));
        }
        if let Some(origin) = origin {
            origins.insert(node.id, origin);
        }
        nodes.push(node);
    }
    let mut resolved = replaced.clone();
    for outputs in resolved.values_mut() {
        for value in outputs.values_mut() {
            resolve(value, &replaced);
        }
    }
    Ok(NormalPass {
        body: NodeBody { nodes },
        origins,
        replaced: resolved,
    })
}

/// A node being normalised.
struct Entry {
    node: Node,
    /// Origin of the node when it is a copy.
    origin: Option<Origin>,
    /// Keys of the calls around the node, outermost first.
    keys: Vec<u32>,
}

impl Entry {
    /// Returns the id as written of the node: its own, or the outermost call it came through.
    fn written_id(&self) -> NodeId {
        self.origin.as_ref().map_or(self.node.id, Origin::written)
    }
}

/// Returns the key of a draw or a call, 0 when it has none.
fn key_of(node: &Node) -> u32 {
    node.config_integer("key")
        .and_then(|key| u32::try_from(key).ok())
        .unwrap_or(0)
}

/// Replaces a link to an output of a replaced node with the output's value, until it names no replaced node.
///
/// A link to a port the replaced node lacks is left as it is, and the checker reports it.
fn resolve(value: &mut InputValue, replaced: &BTreeMap<NodeId, BTreeMap<String, InputValue>>) {
    for _ in 0..=replaced.len() {
        let InputValue::Link(link) = &*value else { return };
        let Some(outputs) = replaced.get(&link.node) else {
            return;
        };
        match link.output_in(outputs) {
            Some(found) => *value = found.clone(),
            None => return,
        }
    }
}

/// Returns the key a draw with key `key` takes inside a call with key `call`.
pub fn compose_key(call: u32, key: u32) -> u32 {
    fold32(mix_seed((u64::from(call) << 32) | u64::from(key)))
}

/// Returns the id `*next` and moves `*next` past it.
///
/// # Errors
///
/// Returns the message for a `*next` of 0 or of `u32::MAX`.
pub(crate) fn mint_id(next: &mut u32) -> Result<NodeId, String> {
    let id = NodeId::new(*next).ok_or_else(|| "runs out of node ids".to_owned())?;
    *next = next.checked_add(1).ok_or_else(|| "runs out of node ids".to_owned())?;
    Ok(id)
}

/// Returns the copies of `function`'s body for the call `call`, and the value of each output.
fn inline(
    call: &Node,
    function: &Function,
    calls: &[NodeId],
    keys: &[u32],
    next: &mut u32,
) -> Result<(Vec<Entry>, BTreeMap<String, InputValue>), String> {
    let nodes = &function.body.nodes;
    let mut fresh: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    let mut inputs: BTreeMap<NodeId, InputValue> = BTreeMap::new();
    for node in nodes {
        if node.op == OP_FUNCTION_INPUT {
            let port = node.config_str("port").unwrap_or_default();
            let value = call
                .inputs
                .get(port)
                .cloned()
                .ok_or_else(|| format!("leaves the input `{port}` of '{}' unwritten", function.id))?;
            inputs.insert(node.id, value);
        } else if node.op != OP_FUNCTION_OUTPUT {
            fresh.insert(node.id, mint_id(next)?);
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
    let mut copies = Vec::new();
    let mut outputs = BTreeMap::new();
    for node in nodes {
        if node.op == OP_FUNCTION_OUTPUT {
            let port = node.config_str("port").unwrap_or_default();
            if let Some(value) = node.inputs.get("value") {
                outputs.insert(port.to_owned(), map_value(value));
            }
            continue;
        }
        let Some(&id) = fresh.get(&node.id) else { continue };
        let mut copy = node.clone();
        copy.id = id;
        copy.region = match node.region {
            Some(region) => fresh.get(&region).copied().or(Some(region)),
            None => call.region,
        };
        for value in copy.inputs.values_mut() {
            *value = map_value(value);
        }
        let origin = Origin {
            calls: calls.to_vec(),
            function: Some(function.id.clone()),
            node: node.id,
        };
        copies.push(Entry {
            node: copy,
            origin: Some(origin),
            keys: keys.to_vec(),
        });
    }
    Ok((copies, outputs))
}

/// Returns `document` with every pass normalised, and the origin of every copy.
///
/// # Errors
///
/// Returns the pass and the error of the first pass that cannot be normalised.
pub(crate) fn normalise(
    document: &GraphDocument,
) -> Result<(GraphDocument, BTreeMap<NodeId, Origin>), (String, NormalError)> {
    let mut normal = document.clone();
    let mut origins = BTreeMap::new();
    let mut next = first_free_id(document);
    for pass in &mut normal.passes {
        let result = normalise_body(document, &pass.body, &mut next).map_err(|error| (pass.id.clone(), error))?;
        pass.body = result.body;
        origins.extend(result.origins);
    }
    Ok((normal, origins))
}
