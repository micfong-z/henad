//! Types, rates, contexts and region structure of one body: rules V3 to V10, V13, V17 in part, V19 and V27.
//!
//! Every output type follows from a node's configuration and inputs, so one forward walk in link order types a body. A
//! node on a cycle reads a hole from any source the walk has not reached.

use std::collections::BTreeMap;

use super::bounds;
use super::config::{self, check_literal, param_type};
use super::view::BodyView;
use super::{Checker, PortType};
use crate::catalogue::{
    ConfigPorts, ConfigType, ContextSet, Effect, InputDefault, NodeDef, OutputRate, ResolvedInput, ResolvedOutput,
    SocketType, transition_rule_ids,
};
use crate::document::{Pass, PassStage};
use crate::ids::NodeId;
use crate::node::{Accumulator, Combine, OP_REGION_PUT};
use crate::types::{Rate, ValueType};
use crate::value::{ConfigValue, InputValue, PortRef};

/// Inferred outputs of a body.
#[derive(Debug, Default)]
pub(super) struct BodyPorts {
    /// Outputs by node id and port name.
    pub outputs: BTreeMap<NodeId, BTreeMap<String, PortType>>,
    /// Inner outputs by node id and port name.
    pub inner_outputs: BTreeMap<NodeId, BTreeMap<String, PortType>>,
}

/// A value an input reads.
#[derive(Debug, Clone)]
struct ReadValue {
    value_type: Option<ValueType>,
    rate: Rate,
}

impl ReadValue {
    /// A value of unknown type, which fits every socket.
    const HOLE: Self = Self {
        value_type: None,
        rate: Rate::Constant,
    };
}

/// Checks one body and returns its inferred outputs.
pub(super) fn check(checker: &mut Checker<'_>, view: &BodyView<'_>) -> BodyPorts {
    check_cycles(checker, view);
    let mut walk = BodyWalk {
        checker,
        view,
        ports: BodyPorts::default(),
    };
    for position in link_order(view) {
        walk.check_node(position);
    }
    let ports = walk.ports;
    check_accumulator_puts(checker, view);
    if view.pass().is_some() {
        warn_unreached(checker, view);
    }
    bounds::warn_counts(checker, view);
    ports
}

/// Returns each link of the node at `position` to a node of the body, with the source's position.
fn links<'a>(view: &BodyView<'a>, position: usize) -> Vec<(usize, &'a PortRef)> {
    let nodes: &'a [crate::node::Node] = view.nodes;
    nodes[position]
        .inputs
        .values()
        .filter_map(|value| match value {
            InputValue::Link(link) => view.positions.get(&link.node).map(|&source| (source, link)),
            _ => None,
        })
        .collect()
}

/// Returns the positions of the body with each source before its readers, and a node on a cycle after the others.
fn link_order(view: &BodyView<'_>) -> Vec<usize> {
    let count = view.nodes.len();
    let mut waiting = vec![0usize; count];
    let mut readers: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (position, waits) in waiting.iter_mut().enumerate() {
        for (source, _) in links(view, position) {
            readers[source].push(position);
            *waits += 1;
        }
    }
    let mut ready: Vec<usize> = (0..count).rev().filter(|&position| waiting[position] == 0).collect();
    let mut order = Vec::with_capacity(count);
    let mut placed = vec![false; count];
    while let Some(position) = ready.pop() {
        order.push(position);
        placed[position] = true;
        for &reader in &readers[position] {
            waiting[reader] -= 1;
            if waiting[reader] == 0 {
                ready.push(reader);
            }
        }
    }
    order.extend((0..count).filter(|&position| !placed[position]));
    order
}

/// Reports each dependency cycle of the body, with a region taken as one node whose inputs include its body (V10).
fn check_cycles(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let count = view.nodes.len();
    let mut waiting = vec![0usize; count];
    let mut waited_by = vec![0usize; count];
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (position, _) in view.nodes.iter().enumerate() {
        for (source, _) in links(view, position) {
            let Some((lifted_source, lifted_reader)) = lift(view, source, position) else {
                continue;
            };
            edges.push((lifted_source, lifted_reader));
            waiting[lifted_reader] += 1;
            waited_by[lifted_source] += 1;
        }
    }
    let mut readers: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut sources: Vec<Vec<usize>> = vec![Vec::new(); count];
    for &(source, reader) in &edges {
        readers[source].push(reader);
        sources[reader].push(source);
    }
    // Peeling nodes with no source left and then nodes with no reader left leaves the nodes on or between cycles.
    let mut gone = vec![false; count];
    let mut ready: Vec<usize> = (0..count).filter(|&position| waiting[position] == 0).collect();
    while let Some(position) = ready.pop() {
        gone[position] = true;
        for &reader in &readers[position] {
            waiting[reader] -= 1;
            if waiting[reader] == 0 {
                ready.push(reader);
            }
        }
    }
    let mut ready: Vec<usize> = (0..count)
        .filter(|&position| !gone[position] && waited_by[position] == 0)
        .collect();
    let mut peeled = gone.clone();
    while let Some(position) = ready.pop() {
        peeled[position] = true;
        for &source in &sources[position] {
            waited_by[source] -= 1;
            if waited_by[source] == 0 && !peeled[source] {
                ready.push(source);
            }
        }
    }
    let on_cycle: Vec<usize> = (0..count).filter(|&position| !peeled[position]).collect();
    if let Some(&first) = on_cycle.first() {
        let mut names: Vec<String> = on_cycle
            .iter()
            .take(8)
            .map(|&position| view.nodes[position].id.to_string())
            .collect();
        if on_cycle.len() > 8 {
            names.push("...".to_owned());
        }
        view.error(
            checker,
            first,
            format!("these nodes depend on each other: {}", names.join(", ")),
            view.table_place(first),
        );
    }
}

/// Returns the two nodes a link joins at the level of their innermost common region, or `None` for a link inside one
/// lifted node, such as a body read of the region's inner output.
fn lift(view: &BodyView<'_>, source: usize, reader: usize) -> Option<(usize, usize)> {
    let (mut source, mut reader) = (source, reader);
    while view.depths[source] > view.depths[reader] {
        source = view.parents[source]?;
    }
    while view.depths[reader] > view.depths[source] {
        reader = view.parents[reader]?;
    }
    while source != reader && view.parents[source] != view.parents[reader] {
        source = view.parents[source]?;
        reader = view.parents[reader]?;
    }
    (source != reader).then_some((source, reader))
}

/// Walk of a body that types each node.
struct BodyWalk<'c, 'f, 'v, 'a> {
    checker: &'c mut Checker<'f>,
    view: &'v BodyView<'a>,
    ports: BodyPorts,
}

impl BodyWalk<'_, '_, '_, '_> {
    /// Types the node at `position`, and reports its contexts, configuration and inputs.
    fn check_node(&mut self, position: usize) {
        let Some(def) = self.view.defs[position] else { return };
        self.check_context(position, def);
        let config_fits = config::check_node(self.checker, self.view, position, def);
        let inputs = def.resolved_inputs(&self.view.nodes[position], self.view.document);
        let mut var = self.seed_var(position, def);
        let rate = self.check_inputs(position, def, &inputs, &mut var, config_fits);
        self.report_special_kinds(position, def, &var);
        self.record_outputs(position, def, &var, rate);
    }

    /// Reports a kind that its pass's stage, a region body, its domain or its backend does not allow (V8, V9, V19).
    fn check_context(&mut self, position: usize, def: &NodeDef) {
        let view = self.view;
        let in_region = view.nodes[position].region.is_some();
        if in_region && !def.contexts.region {
            view.error(
                self.checker,
                position,
                format!("{} does not belong in a region body", view.name(position)),
                view.region_place(position),
            );
        }
        let Some(pass) = view.pass() else { return };
        if !stage_allowed(def.contexts, pass) {
            let reason = if def.has_effect(Effect::ReadsOtherLane) {
                " since it reads another element"
            } else {
                ""
            };
            view.error(
                self.checker,
                position,
                format!(
                    "{} does not belong in the {} pass '{}'{reason}",
                    view.name(position),
                    pass.stage.as_str(),
                    pass.id
                ),
                view.op_place(position),
            );
        }
        if let Some(domain) = def.domain
            && pass.domain.is_some_and(|known| known != domain)
        {
            view.error(
                self.checker,
                position,
                format!(
                    "{} runs over {}, and the pass '{}' runs over another domain",
                    view.name(position),
                    domain.as_str(),
                    pass.id
                ),
                view.op_place(position),
            );
        }
        // Init passes run on the host, reruns included. Every other pass of a graph runs on the device.
        let (needed, backend) = if pass.stage == PassStage::Init {
            (def.backends.host, "the host")
        } else {
            (def.backends.wgsl, "the GPU")
        };
        if !needed {
            view.error(
                self.checker,
                position,
                format!(
                    "{} cannot run on {backend}, where the {} pass '{}' runs",
                    view.name(position),
                    pass.stage.as_str(),
                    pass.id
                ),
                view.op_place(position),
            );
        }
    }

    /// Returns the type variable as configuration fixes it: the lane of a stencil read, the values of a pick.
    fn seed_var(&mut self, position: usize, def: &NodeDef) -> Option<ValueType> {
        let node = &self.view.nodes[position];
        if def.has_effect(Effect::ReadsOtherLane) && !def.type_var.is_empty() {
            let lane_type = self.lane_type(position)?;
            if !def.type_var.iter().any(|class| class.holds(&lane_type)) {
                self.view.error(
                    self.checker,
                    position,
                    format!(
                        "{} reads the lane '{}' of type {lane_type}, which it cannot compare",
                        self.view.name(position),
                        node.config
                            .get("lane")
                            .and_then(ConfigValue::as_str)
                            .unwrap_or_default(),
                    ),
                    self.view.config_place(position, "lane"),
                );
            }
            return Some(lane_type);
        }
        if def.config.iter().any(|key| key.value == ConfigType::Literals) {
            let values = node.config.get("values")?.as_array()?;
            return config::literal_of(values.first()?).map(|literal| literal.value_type());
        }
        None
    }

    /// Returns the type of the lane `config.lane` of the node at `position`.
    fn lane_type(&self, position: usize) -> Option<ValueType> {
        let id = self.view.nodes[position].config.get("lane")?.as_str()?;
        let lanes = &self.view.document.lanes;
        lanes
            .iter()
            .find(|lane| lane.id == id)
            .map(|lane| lane.value_type.clone())
    }

    /// Returns the category type `config.category` of the node at `position` names.
    fn config_category(&self, position: usize) -> Option<ValueType> {
        let id = self.view.nodes[position].config.get("category")?.as_str()?;
        self.view
            .document
            .categories
            .iter()
            .any(|category| category.id == id)
            .then(|| ValueType::Category(id.to_owned()))
    }

    /// Returns the accumulator of the enclosing region that `config.acc` of the node at `position` names.
    fn put_accumulator(&self, position: usize) -> Option<Accumulator> {
        let view = self.view;
        let name = view.nodes[position].config.get("acc")?.as_str()?;
        let region = view.parents[position]?;
        view.nodes[region]
            .accumulators()
            .ok()?
            .into_iter()
            .find(|accumulator| accumulator.name == name)
    }

    /// Returns the concrete type of `socket` at the node, where it does not depend on the type variable.
    fn socket_type(&self, position: usize, socket: SocketType) -> Option<ValueType> {
        match socket {
            SocketType::Fixed(class) => class.value_type(),
            SocketType::Lane => self.lane_type(position),
            SocketType::ConfigCategory => self.config_category(position),
            SocketType::Accumulator => self.put_accumulator(position).map(|accumulator| accumulator.value_type),
            SocketType::Field => self.field(position).map(|field| field.value_type.clone()),
            SocketType::FieldLayer => self.field(position).map(|field| ValueType::Layer(field.id.clone())),
            SocketType::LaneElement => {
                let id = self.view.nodes[position].config.get("lane")?.as_str()?;
                let lane = self.view.document.lanes.iter().find(|lane| lane.id == id)?;
                Some(match lane.domain {
                    crate::document::Domain::Cells => ValueType::Cell,
                    crate::document::Domain::Agents => ValueType::Agent,
                })
            }
            SocketType::InterfacePort => self.interface_port(position),
            SocketType::Var | SocketType::Vector | SocketType::Scalar | SocketType::Wide => None,
        }
    }

    /// Returns the field `config.field` of the node at `position` names.
    fn field(&self, position: usize) -> Option<&crate::document::Field> {
        let id = self.view.nodes[position].config.get("field")?.as_str()?;
        self.view.document.fields.iter().find(|field| field.id == id)
    }

    /// Returns the type of the function port `config.port` of a `fn.input` or `fn.output` at `position` names.
    fn interface_port(&self, position: usize) -> Option<ValueType> {
        let super::view::Owner::Function(function) = self.view.owner else {
            return None;
        };
        let node = &self.view.nodes[position];
        let name = node.config.get("port")?.as_str()?;
        let ports = if node.op == crate::node::OP_FUNCTION_OUTPUT {
            &function.outputs
        } else {
            &function.inputs
        };
        ports
            .iter()
            .find(|port| port.port == name)
            .map(|port| port.value_type.clone())
    }

    /// Checks every input, and returns the join of their rates (V4, V5, V6, V7).
    fn check_inputs(
        &mut self,
        position: usize,
        def: &NodeDef,
        inputs: &[ResolvedInput],
        var: &mut Option<ValueType>,
        config_fits: bool,
    ) -> Rate {
        let view = self.view;
        let node = &view.nodes[position];
        let mut joined = Rate::Constant;
        if config_fits {
            for port in node.inputs.keys() {
                if !inputs.iter().any(|input| input.name == *port) {
                    view.error(
                        self.checker,
                        position,
                        format!("{} has no input `{port}`", view.name(position)),
                        view.input_place(position, port),
                    );
                }
            }
        }
        for input in inputs {
            let Some(value) = node.inputs.get(&input.name) else {
                if self.input_is_required(position, def, input) {
                    view.error(
                        self.checker,
                        position,
                        format!("{} needs the input `{}`", view.name(position), input.name),
                        view.table_place(position),
                    );
                }
                continue;
            };
            let read = self.read_value(position, &input.name, value);
            joined = joined.join(read.rate);
            self.check_rate(position, input, &read);
            let Some(read_type) = read.value_type else { continue };
            let is_var = input.value_type == Some(SocketType::Var);
            let expected = input
                .declared
                .clone()
                .or_else(|| self.socket_type(position, input.value_type?))
                .or_else(|| if is_var { var.clone() } else { None });
            match expected {
                Some(expected) if expected != read_type => view.error(
                    self.checker,
                    position,
                    format!(
                        "{}: the input `{}` is {read_type}, and the node needs {expected}",
                        view.name(position),
                        input.name
                    ),
                    view.input_place(position, &input.name),
                ),
                None if is_var => {
                    if def.type_var.iter().any(|class| class.holds(&read_type)) {
                        *var = Some(read_type);
                    } else {
                        view.error(
                            self.checker,
                            position,
                            format!(
                                "{}: the input `{}` is {read_type}, a type the node does not take",
                                view.name(position),
                                input.name
                            ),
                            view.input_place(position, &input.name),
                        );
                    }
                }
                Some(_) | None => {}
            }
        }
        joined
    }

    /// Returns whether an unwritten `input` is an error, with the rules that depend on the node's other keys.
    fn input_is_required(&self, position: usize, def: &NodeDef, input: &ResolvedInput) -> bool {
        let node = &self.view.nodes[position];
        match def.config_ports {
            // Every variant needs its case unless `default` is written.
            ConfigPorts::CategoryCases if input.name.starts_with("case.") => !node.inputs.contains_key("default"),
            _ if node.op == OP_REGION_PUT && input.name == "value" => self
                .put_accumulator(position)
                .is_some_and(|accumulator| accumulator.combine != Combine::Count),
            _ if input.default == InputDefault::OwnCell => super::agents::own_cell_missing(self.view),
            _ => input.default == InputDefault::Required,
        }
    }

    /// Reports a value that is too fast for its socket (V7).
    fn check_rate(&mut self, position: usize, input: &ResolvedInput, read: &ReadValue) {
        let view = self.view;
        if read.rate > input.max_rate {
            let wanted = match input.max_rate {
                Rate::Constant | Rate::Parameter => "a constant or a parameter",
                Rate::Tick => "a value the same for every element",
                Rate::Element | Rate::Pair => "a value of the element",
            };
            view.error(
                self.checker,
                position,
                format!(
                    "{}: the input `{}` takes {wanted}, and this one changes faster",
                    view.name(position),
                    input.name
                ),
                view.input_place(position, &input.name),
            );
        }
    }

    /// Returns the value an input reads, and reports a name it holds that does not resolve.
    fn read_value(&mut self, position: usize, port: &str, value: &InputValue) -> ReadValue {
        let view = self.view;
        match value {
            InputValue::Literal(literal) => {
                if let Err(reason) = check_literal(view, literal) {
                    view.error(
                        self.checker,
                        position,
                        format!("{}: the input `{port}` {reason}", view.name(position)),
                        view.input_place(position, port),
                    );
                    return ReadValue::HOLE;
                }
                ReadValue {
                    value_type: Some(literal.value_type()),
                    rate: Rate::Constant,
                }
            }
            InputValue::Param(id) => {
                let Some(param) = view.document.params.iter().find(|param| param.id() == id) else {
                    view.error(
                        self.checker,
                        position,
                        format!(
                            "{}: the input `{port}` binds the parameter '{id}', and the file declares no such parameter",
                            view.name(position)
                        ),
                        view.input_place(position, port),
                    );
                    return ReadValue::HOLE;
                };
                ReadValue {
                    value_type: Some(param_type(param.kind())),
                    rate: Rate::Parameter,
                }
            }
            InputValue::Link(link) => self.read_link(position, port, link),
        }
    }

    /// Returns the output a link names, and reports a link that does not resolve or that leaves a region (V4, V13).
    fn read_link(&mut self, position: usize, port: &str, link: &PortRef) -> ReadValue {
        let view = self.view;
        let problem = |walk: &mut Self, message: String| {
            view.error(
                walk.checker,
                position,
                format!("{}: the input `{port}` {message}", view.name(position)),
                view.input_place(position, port),
            );
            ReadValue::HOLE
        };
        let Some(&source) = view.positions.get(&link.node) else {
            return problem(self, format!("reads {link}, and this body holds no node {}", link.node));
        };
        if source == position {
            return problem(self, "reads its own node".to_owned());
        }
        // A kind this build does not define has a diagnostic of its own, and its ports are unknown.
        let Some(def) = view.defs[source] else {
            return ReadValue::HOLE;
        };
        let inside = view.is_inside(position, source);
        let source_id = view.nodes[source].id;
        let outputs = def.resolved_outputs(&view.nodes[source], view.document);
        let name = if let Some(name) = &link.port {
            name.clone()
        } else if inside {
            return problem(self, format!("reads {link} from its body, and needs to name a port"));
        } else {
            match outputs.as_slice() {
                [only] => only.name.clone(),
                [] => return problem(self, format!("reads {link}, which has no output")),
                _ => {
                    return problem(
                        self,
                        format!("reads {link}, which has several outputs and needs to name a port"),
                    );
                }
            }
        };
        let inner = def.resolved_inner_outputs(&view.nodes[source]);
        if inner.iter().any(|output| output.name == name) {
            if !inside {
                return problem(
                    self,
                    format!("reads {source_id}.{name}, which only the body of {source_id} can read"),
                );
            }
            return recorded(self.ports.inner_outputs.get(&source_id), &name);
        }
        if !outputs.iter().any(|output| output.name == name) {
            return problem(
                self,
                format!("reads {source_id}.{name}, and {source_id} has no such port"),
            );
        }
        if inside {
            return problem(
                self,
                format!("reads {source_id}.{name}, the result of the region around it, which is a cycle"),
            );
        }
        if let Some(parent) = view.parents[source]
            && !view.is_inside(position, parent)
        {
            return problem(
                self,
                format!(
                    "reads {link} from inside the region {}, and only an accumulator output leaves a region",
                    view.nodes[parent].id
                ),
            );
        }
        recorded(self.ports.outputs.get(&source_id), &name)
    }

    /// Reports what only one kind's definition says, for the type variable the walk inferred.
    fn report_special_kinds(&mut self, position: usize, def: &NodeDef, var: &Option<ValueType>) {
        let view = self.view;
        let node = &view.nodes[position];
        if def.has_effect(Effect::View)
            && let Some(palette) = node.config.get("palette").and_then(ConfigValue::as_str)
        {
            // A category value takes the category's colours, and a number needs a palette.
            match var {
                Some(ValueType::Category(category)) if category != palette => view.error(
                    self.checker,
                    position,
                    format!(
                        "{}: a value of {category} needs the palette '{category}'",
                        view.name(position)
                    ),
                    view.config_place(position, "palette"),
                ),
                Some(ValueType::U32) if !view.document.palettes.iter().any(|known| known.id == palette) => view.error(
                    self.checker,
                    position,
                    format!(
                        "{}: a number needs a palette, and '{palette}' is a category",
                        view.name(position)
                    ),
                    view.config_place(position, "palette"),
                ),
                _ => {}
            }
        }
        if def.config_ports == ConfigPorts::TransitionRules {
            let rules = node.config.get("rules").and_then(ConfigValue::as_array).unwrap_or(&[]);
            for id in transition_rule_ids(node) {
                let has_target = rules
                    .iter()
                    .filter_map(ConfigValue::as_table)
                    .find(|rule| rule.get("id").and_then(ConfigValue::as_str) == Some(id.as_str()))
                    .is_some_and(|rule| rule.contains_key("to"));
                if !has_target && !node.inputs.contains_key(&format!("rule.{id}.to")) {
                    view.error(
                        self.checker,
                        position,
                        format!(
                            "{}: the rule '{id}' needs a `to` variant or the input `rule.{id}.to`",
                            view.name(position)
                        ),
                        view.config_place(position, "rules"),
                    );
                }
            }
        }
    }

    /// Records the node's outputs, and reports a host type at a rate the pass's backend cannot hold (V27).
    fn record_outputs(&mut self, position: usize, def: &NodeDef, var: &Option<ValueType>, joined: Rate) {
        let view = self.view;
        let node = &view.nodes[position];
        let outputs = def.resolved_outputs(node, view.document);
        let mut record: BTreeMap<String, PortType> = BTreeMap::new();
        for output in &outputs {
            let port = self.port_type(position, output, var, joined);
            self.check_host_type(position, &output.name, &port);
            record.insert(output.name.clone(), port);
        }
        self.ports.outputs.insert(node.id, record);
        if def.is_region {
            let inner = def
                .resolved_inner_outputs(node)
                .iter()
                .map(|output| (output.name.clone(), self.port_type(position, output, var, joined)))
                .collect();
            self.ports.inner_outputs.insert(node.id, inner);
        }
    }

    fn port_type(&self, position: usize, output: &ResolvedOutput, var: &Option<ValueType>, joined: Rate) -> PortType {
        let value_type = output.declared.clone().or_else(|| match output.value_type {
            Some(SocketType::Var) => var.clone(),
            Some(SocketType::Vector) => var.as_ref().and_then(crate::catalogue::vector_of),
            Some(SocketType::Scalar) => var.as_ref().and_then(crate::catalogue::component_of),
            Some(SocketType::Wide) => var.as_ref().and_then(crate::catalogue::wide_of),
            Some(socket) => self.socket_type(position, socket),
            None => None,
        });
        let rate = match output.rate {
            OutputRate::Join => joined,
            // An element value inside a region body changes per iteration too.
            OutputRate::Pinned(Rate::Element) if self.view.parents[position].is_some() => Rate::Pair,
            OutputRate::Pinned(rate) => rate,
        };
        PortType { value_type, rate }
    }

    fn check_host_type(&mut self, position: usize, port: &str, value: &PortType) {
        let view = self.view;
        let Some(pass) = view.pass() else { return };
        if pass.stage != PassStage::Init
            && value.rate >= Rate::Element
            && let Some(value_type) = &value.value_type
            && value_type.is_host_only()
        {
            view.error(
                self.checker,
                position,
                format!(
                    "{}: the output `{port}` is {value_type} per element, which only an init pass holds",
                    view.name(position)
                ),
                view.table_place(position),
            );
        }
    }
}

/// Returns the recorded port `name`, or a hole for a source a cycle put after its reader.
fn recorded(ports: Option<&BTreeMap<String, PortType>>, name: &str) -> ReadValue {
    ports
        .and_then(|ports| ports.get(name))
        .map_or(ReadValue::HOLE, |port| ReadValue {
            value_type: port.value_type.clone(),
            rate: port.rate,
        })
}

/// Returns whether `contexts` allows the stage of `pass`.
fn stage_allowed(contexts: ContextSet, pass: &Pass) -> bool {
    match pass.stage {
        PassStage::Init => contexts.init,
        PassStage::Tick => contexts.tick,
        PassStage::View => contexts.view,
        PassStage::Stats => contexts.stats,
        PassStage::Action => contexts.action,
    }
}

/// Reports a `region.put` outside a region, or with an accumulator its region lacks or another put feeds (V13).
fn check_accumulator_puts(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let mut fed: BTreeMap<(usize, String), usize> = BTreeMap::new();
    for (position, node) in view.nodes.iter().enumerate() {
        if view.defs[position].is_none_or(|def| !def.has_effect(Effect::RegionSink)) {
            continue;
        }
        let Some(region) = view.parents[position] else {
            if node.region.is_none() {
                view.error(
                    checker,
                    position,
                    format!("{} sits outside every region", view.name(position)),
                    view.table_place(position),
                );
            }
            continue;
        };
        let Some(name) = node.config.get("acc").and_then(ConfigValue::as_str) else {
            continue;
        };
        let accumulators = view.nodes[region].accumulators().unwrap_or_default();
        if !accumulators.iter().any(|accumulator| accumulator.name == name) {
            view.error(
                checker,
                position,
                format!(
                    "{} feeds the accumulator '{name}', and the region {} has no such accumulator",
                    view.name(position),
                    view.nodes[region].id
                ),
                view.config_place(position, "acc"),
            );
        } else if let Some(&first) = fed.get(&(region, name.to_owned())) {
            view.error(
                checker,
                position,
                format!(
                    "{} feeds the accumulator '{name}', which {} feeds already, and an accumulator takes one `region.put`",
                    view.name(position),
                    view.nodes[first].id
                ),
                view.config_place(position, "acc"),
            );
        } else {
            fed.insert((region, name.to_owned()), position);
        }
    }
}

/// Warns of each node no lane write, colour, reducer or stream draw reaches, which the compiler prunes.
fn warn_unreached(checker: &mut Checker<'_>, view: &BodyView<'_>) {
    let count = view.nodes.len();
    let mut reached = vec![false; count];
    let mut pending: Vec<usize> = (0..count)
        .filter(|&position| {
            view.defs[position].is_some_and(|def| {
                def.has_effect(Effect::WritesLane)
                    || def.has_effect(Effect::WritesField)
                    || def.has_effect(Effect::Deposits)
                    || def.has_effect(Effect::Counts)
                    || def.has_effect(Effect::View)
                    || def.has_effect(Effect::Reduce)
                    || def.has_effect(Effect::DrawsStream)
            })
        })
        .collect();
    // A node a `[[stat]]` reads is kept, as a reducer is.
    for stat in &view.document.stats {
        if let Some(&position) = view.positions.get(&stat.from.node) {
            pending.push(position);
        }
    }
    let mut puts: Vec<Vec<usize>> = vec![Vec::new(); count];
    for position in 0..count {
        if let Some(region) = view.parents[position]
            && view.defs[position].is_some_and(|def| def.has_effect(Effect::RegionSink))
        {
            puts[region].push(position);
        }
    }
    while let Some(position) = pending.pop() {
        if std::mem::replace(&mut reached[position], true) {
            continue;
        }
        pending.extend(links(view, position).into_iter().map(|(source, _)| source));
        pending.extend(puts[position].iter().copied());
        pending.extend(view.parents[position]);
    }
    for (position, reached) in reached.iter().enumerate() {
        if !reached && view.defs[position].is_some() {
            view.warning(
                checker,
                position,
                format!(
                    "{} reaches nothing the pass keeps, and will be dropped",
                    view.name(position)
                ),
                view.table_place(position),
            );
        }
    }
}
