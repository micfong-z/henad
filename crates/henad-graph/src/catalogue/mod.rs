//! Catalogue of node kinds: one [`NodeDef`] per kind, from which the checker and the library lock read.
//!
//! A *kind* is what a node's `op` names, such as `math.add`. A kind's written semantics and its known-answer cases are
//! its definition, and an implementation on either backend that disagrees with them has a bug. A *context* is a place a
//! kind may appear: a pass's stage, and whether the place is a region body.

mod bits_logic;
mod cases;
mod conv;
mod math;
mod random;
mod region;
mod state;
mod view;

use std::collections::BTreeMap;

use crate::document::{Domain, GraphDocument};
use crate::node::{Combine, Node};
use crate::types::{Rate, ValueType};
use crate::value::{ConfigValue, InputValue, Literal};

pub use cases::{CaseContext, CaseForm, CaseScope, CaseValue, KnownAnswer, NodeCase, PassCase};

/// Every definition of the catalogue, grouped as the palette groups them.
static GROUPS: [&[NodeDef]; 7] = [
    state::KINDS,
    math::KINDS,
    bits_logic::KINDS,
    conv::KINDS,
    random::KINDS,
    region::KINDS,
    view::KINDS,
];

/// Returns every definition of the catalogue.
pub fn definitions() -> impl Iterator<Item = &'static NodeDef> {
    GROUPS.iter().flat_map(|group| group.iter())
}

/// Returns the definition `op` resolves to at node library `library`: the newest with `since <= library`.
pub fn lookup(op: &str, library: u32) -> Option<&'static NodeDef> {
    definitions()
        .filter(|def| def.id == op && def.since <= library)
        .max_by_key(|def| def.since)
}

/// One definition of a node kind.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct NodeDef {
    /// Type id, stable forever, such as `math.add`.
    pub id: &'static str,
    /// Node library version the definition belongs to.
    pub since: u32,
    /// Palette group.
    pub group: &'static str,
    /// Display name.
    pub label: &'static str,
    /// Written semantics, part of the definition.
    pub semantics: &'static str,
    /// Allowed set of the kind's type variable, empty for a kind that has none.
    pub type_var: &'static [TypeClass],
    /// Configuration keys.
    pub config: &'static [ConfigKey],
    /// Fixed input sockets, in the order they are written.
    pub inputs: &'static [InputSocket],
    /// Fixed output sockets.
    pub outputs: &'static [OutputSocket],
    /// Inputs and outputs that configuration makes.
    pub config_ports: ConfigPorts,
    /// Outputs only a region's body reads, `&[]` for a kind that is no region.
    pub inner_outputs: &'static [OutputSocket],
    /// Whether nodes can name the node as their region.
    pub is_region: bool,
    /// Places the kind may appear.
    pub contexts: ContextSet,
    /// Domain the kind needs, `None` for any.
    pub domain: Option<Domain>,
    /// What the kind does besides returning its outputs.
    pub effects: &'static [Effect],
    /// Backends that can run the kind.
    pub backends: Backends,
    /// How closely the backends agree.
    pub parity: Parity,
    /// Work per element, for the cost bound of a pass.
    pub cost: NodeCost,
    /// Known-answer cases.
    pub cases: &'static [KnownAnswer],
}

/// Empty definition that a `static` table fills with `..`, with no semantics and no cases.
pub(crate) const BASE: NodeDef = NodeDef {
    id: "",
    since: 1,
    group: "",
    label: "",
    semantics: "",
    type_var: &[],
    config: &[],
    inputs: &[],
    outputs: &[],
    config_ports: ConfigPorts::None,
    inner_outputs: &[],
    is_region: false,
    contexts: ContextSet::ANY,
    domain: None,
    effects: &[],
    backends: Backends::BOTH,
    parity: Parity::Exact,
    cost: NodeCost::Units(1),
    cases: &[],
};

/// Family of value types a socket's type can be drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TypeClass {
    /// `bool`.
    Bool,
    /// `u32`.
    U32,
    /// `i32`.
    I32,
    /// `f32`.
    F32,
    /// `vec2u`.
    Vec2U,
    /// `vec2i`.
    Vec2I,
    /// `vec2f`.
    Vec2F,
    /// `u64`, on the host only.
    U64,
    /// `f64`, on the host only.
    F64,
    /// `vec2d`, on the host only.
    Vec2D,
    /// Any declared category.
    Category,
}

impl TypeClass {
    /// Returns whether `value_type` belongs to the class.
    pub fn holds(self, value_type: &ValueType) -> bool {
        self == match value_type {
            ValueType::Bool => Self::Bool,
            ValueType::U32 => Self::U32,
            ValueType::I32 => Self::I32,
            ValueType::F32 => Self::F32,
            ValueType::Vec2U => Self::Vec2U,
            ValueType::Vec2I => Self::Vec2I,
            ValueType::Vec2F => Self::Vec2F,
            ValueType::U64 => Self::U64,
            ValueType::F64 => Self::F64,
            ValueType::Vec2D => Self::Vec2D,
            ValueType::Category(_) => Self::Category,
            _ => return false,
        }
    }

    /// Returns the one type of the class, or `None` for [`Self::Category`], which holds many.
    pub fn value_type(self) -> Option<ValueType> {
        Some(match self {
            Self::Bool => ValueType::Bool,
            Self::U32 => ValueType::U32,
            Self::I32 => ValueType::I32,
            Self::F32 => ValueType::F32,
            Self::Vec2U => ValueType::Vec2U,
            Self::Vec2I => ValueType::Vec2I,
            Self::Vec2F => ValueType::Vec2F,
            Self::U64 => ValueType::U64,
            Self::F64 => ValueType::F64,
            Self::Vec2D => ValueType::Vec2D,
            Self::Category => return None,
        })
    }
}

/// Type of a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SocketType {
    /// One fixed type.
    Fixed(TypeClass),
    /// The kind's type variable, one type per node, from [`NodeDef::type_var`].
    Var,
    /// Type of the lane that `config.lane` names.
    Lane,
    /// `cat:<id>` for the category that `config.category` names.
    ConfigCategory,
    /// Type of the accumulator of the enclosing region that `config.acc` names.
    Accumulator,
}

/// Shorthand for a socket of type `bool`.
pub(crate) const BOOL_TYPE: SocketType = SocketType::Fixed(TypeClass::Bool);
/// Shorthand for a socket of type `u32`.
pub(crate) const U32_TYPE: SocketType = SocketType::Fixed(TypeClass::U32);
/// Shorthand for a socket of type `f32`.
pub(crate) const F32_TYPE: SocketType = SocketType::Fixed(TypeClass::F32);

/// What happens to an input the file leaves unwritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputDefault {
    /// The file must write the input.
    Required,
    /// The input is optional and has no written default.
    Optional,
    /// The input takes this `bool` literal, and the canonical text writes it.
    Bool(bool),
}

/// One fixed input socket.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct InputSocket {
    /// Port name.
    pub name: &'static str,
    /// Type of the socket.
    pub value_type: SocketType,
    /// Fastest rate the socket accepts.
    pub max_rate: Rate,
    /// What an unwritten input means.
    pub default: InputDefault,
}

/// Returns a required socket that accepts every rate.
pub(crate) const fn input(name: &'static str, value_type: SocketType) -> InputSocket {
    InputSocket {
        name,
        value_type,
        max_rate: Rate::Pair,
        default: InputDefault::Required,
    }
}

/// Returns a required socket that accepts rates up to `max_rate`.
pub(crate) const fn input_up_to(name: &'static str, value_type: SocketType, max_rate: Rate) -> InputSocket {
    InputSocket {
        name,
        value_type,
        max_rate,
        default: InputDefault::Required,
    }
}

/// The `when` socket: a `bool` of any rate, `true` when unwritten.
pub(crate) const WHEN: InputSocket = InputSocket {
    name: "when",
    value_type: BOOL_TYPE,
    max_rate: Rate::Pair,
    default: InputDefault::Bool(true),
};

/// How the rate of an output follows from the node's inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutputRate {
    /// The join of the rates of the inputs.
    Join,
    /// This rate. An element rate inside a region body reads as the pair rate.
    Pinned(Rate),
}

/// One fixed output socket.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct OutputSocket {
    /// Port name.
    pub name: &'static str,
    /// Type of the socket.
    pub value_type: SocketType,
    /// Rate of the output.
    pub rate: OutputRate,
}

/// Returns an output at the join of the inputs' rates.
pub(crate) const fn output(name: &'static str, value_type: SocketType) -> OutputSocket {
    OutputSocket {
        name,
        value_type,
        rate: OutputRate::Join,
    }
}

/// Returns an output pinned at `rate`.
pub(crate) const fn pinned(name: &'static str, value_type: SocketType, rate: Rate) -> OutputSocket {
    OutputSocket {
        name,
        value_type,
        rate: OutputRate::Pinned(rate),
    }
}

/// Ports a node's configuration makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigPorts {
    /// No port comes from configuration.
    None,
    /// For each accumulator of `config.acc`: the input `init.<acc>`, the output `<acc>` and the inner output
    /// `prev.<acc>`.
    Accumulators,
    /// For each variant of `config.category`: the input `case.<variant>`, of the type variable.
    CategoryCases,
    /// For each rule of `config.rules`: the inputs `rule.<id>.when` and `rule.<id>.to`, and the output `fired.<id>`.
    TransitionRules,
    /// For each variant of `config.category`: the `u64` output `<variant>` at the tick rate.
    VariantCounts,
}

/// Places a kind may appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContextSet {
    /// A map pass in the init stage.
    pub init: bool,
    /// A map pass in the tick stage.
    pub tick: bool,
    /// A map pass in an action.
    pub action: bool,
    /// A view pass.
    pub view: bool,
    /// A stats pass.
    pub stats: bool,
    /// A region body. A node there also needs its pass's stage to allow the kind.
    pub region: bool,
}

impl ContextSet {
    /// Every pass, and region bodies.
    pub const ANY: Self = Self {
        init: true,
        tick: true,
        action: true,
        view: true,
        stats: true,
        region: true,
    };
    /// Every pass, outside region bodies.
    pub const MAP: Self = Self {
        region: false,
        ..Self::ANY
    };
    /// Every pass but view and stats, and region bodies: where a counter-based draw may appear.
    pub const DRAWS: Self = Self {
        view: false,
        stats: false,
        ..Self::ANY
    };
    /// No place, for a definition to add to.
    pub const NONE: Self = Self {
        init: false,
        tick: false,
        action: false,
        view: false,
        stats: false,
        region: false,
    };
}

/// What a kind does besides returning its outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Reads the element's own value of a lane.
    ReadsLane,
    /// Reads another element's value of the lane `config.lane`, which can make the lane dual.
    ReadsOtherLane,
    /// Writes the element's value of the lane `config.lane`.
    WritesLane,
    /// Draws from the counter-based family, keyed by `config.key`.
    DrawsCounter,
    /// Draws from the stage's serial stream.
    DrawsStream,
    /// Reduces a pass's elements to one value.
    Reduce,
    /// Sets the element's colour.
    View,
    /// Feeds an accumulator of the enclosing region.
    RegionSink,
}

/// Backends that can run a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Backends {
    /// The host evaluator, which runs init passes and reruns.
    pub host: bool,
    /// Generated WGSL, which runs every other pass.
    pub wgsl: bool,
}

impl Backends {
    /// Host and WGSL.
    pub const BOTH: Self = Self { host: true, wgsl: true };
    /// Host alone.
    pub const HOST: Self = Self {
        host: true,
        wgsl: false,
    };
}

/// How closely the backends that run a kind agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Parity {
    /// Bit-equal.
    Exact,
    /// Close across backends, and bit-equal on one device.
    Approx,
}

/// Work of a kind per element, in cost units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeCost {
    /// This many units.
    Units(u32),
    /// One unit, and this many more per rule of `config.rules`.
    PerRule(u32),
    /// `1 + max * (body cost + 1)`, for a region with the iteration bound `config.max`.
    Repeat,
}

/// One configuration key of a kind.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ConfigKey {
    /// Key name.
    pub name: &'static str,
    /// Values the key takes.
    pub value: ConfigType,
    /// Value of an unwritten key, which the canonical text writes. `None` when the file must write the key.
    pub default: Option<ConfigDefault>,
}

/// Returns a key the file must write.
pub(crate) const fn config(name: &'static str, value: ConfigType) -> ConfigKey {
    ConfigKey {
        name,
        value,
        default: None,
    }
}

/// Default of a configuration key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigDefault {
    /// A truth value.
    Bool(bool),
    /// One word.
    Word(&'static str),
}

/// Values a configuration key takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigType {
    /// A truth value.
    Bool,
    /// An integer from `min` to `max`.
    Integer {
        /// Smallest value.
        min: i64,
        /// Largest value.
        max: i64,
    },
    /// A `u64` written as a string in decimal or `0x` hexadecimal. The canonical text writes lowercase hexadecimal.
    U64Text,
    /// One of these words.
    Word(&'static [&'static str]),
    /// The id of a declared lane.
    LaneId,
    /// The id of a declared category.
    CategoryId,
    /// The id of a declared palette, or of the category of the node's input.
    PaletteId,
    /// The name of a variant of `config.category`.
    VariantName,
    /// A list of integers from 0 to `max`.
    CountList {
        /// Largest value.
        max: i64,
    },
    /// A list of one-key literal tables of one type.
    Literals,
    /// A list of `{ name, type, combine }` tables.
    Accumulators,
    /// The name of an accumulator of the enclosing region.
    AccumulatorName,
    /// A list of `{ id, from, to, phase }` tables, `to` optional when the input `rule.<id>.to` is written.
    TransitionRules,
}

/// One input of a node as its configuration makes it: fixed or from configuration.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
    /// Port name.
    pub name: String,
    /// Type of the socket. `None` when the configuration that decides it is broken.
    pub value_type: Option<SocketType>,
    /// Type for a socket whose [`SocketType`] needs the node. Set for ports made by configuration.
    pub declared: Option<ValueType>,
    /// Fastest rate the socket accepts.
    pub max_rate: Rate,
    /// What an unwritten input means.
    pub default: InputDefault,
}

/// One output of a node as its configuration makes it.
#[derive(Debug, Clone)]
pub struct ResolvedOutput {
    /// Port name.
    pub name: String,
    /// Type of the socket.
    pub value_type: Option<SocketType>,
    /// Type for a socket whose [`SocketType`] needs the node. Set for ports made by configuration.
    pub declared: Option<ValueType>,
    /// Rate of the output.
    pub rate: OutputRate,
}

impl NodeDef {
    /// Returns whether the kind has `effect`.
    pub fn has_effect(&self, effect: Effect) -> bool {
        self.effects.contains(&effect)
    }

    /// Returns the inputs of `node` in the order the canonical text writes them: the fixed ports, then the ports
    /// configuration makes, in the order of their configuration.
    ///
    /// Broken configuration makes no port, and the checker reports it.
    pub fn resolved_inputs(&self, node: &Node, document: &GraphDocument) -> Vec<ResolvedInput> {
        let mut inputs: Vec<ResolvedInput> = self
            .inputs
            .iter()
            .map(|socket| ResolvedInput {
                name: socket.name.to_owned(),
                value_type: Some(socket.value_type),
                declared: None,
                max_rate: socket.max_rate,
                default: socket.default,
            })
            .collect();
        match self.config_ports {
            ConfigPorts::None | ConfigPorts::VariantCounts => {}
            ConfigPorts::Accumulators => {
                for accumulator in node.accumulators().unwrap_or_default() {
                    inputs.push(ResolvedInput {
                        name: format!("init.{}", accumulator.name),
                        value_type: None,
                        default: if accumulator.combine == Combine::Carry {
                            InputDefault::Required
                        } else {
                            InputDefault::Optional
                        },
                        declared: Some(accumulator.value_type),
                        max_rate: Rate::Pair,
                    });
                }
            }
            ConfigPorts::CategoryCases => {
                for variant in config_category_variants(node, document) {
                    inputs.push(ResolvedInput {
                        name: format!("case.{variant}"),
                        value_type: Some(SocketType::Var),
                        declared: None,
                        max_rate: Rate::Pair,
                        default: InputDefault::Optional,
                    });
                }
            }
            ConfigPorts::TransitionRules => {
                for rule in transition_rule_ids(node) {
                    inputs.push(ResolvedInput {
                        name: format!("rule.{rule}.when"),
                        value_type: Some(BOOL_TYPE),
                        declared: None,
                        max_rate: Rate::Pair,
                        default: InputDefault::Required,
                    });
                    inputs.push(ResolvedInput {
                        name: format!("rule.{rule}.to"),
                        value_type: Some(SocketType::ConfigCategory),
                        declared: None,
                        max_rate: Rate::Pair,
                        default: InputDefault::Optional,
                    });
                }
            }
        }
        inputs
    }

    /// Returns the outputs of `node` that nodes outside a region body can read, fixed ports first.
    pub fn resolved_outputs(&self, node: &Node, document: &GraphDocument) -> Vec<ResolvedOutput> {
        let mut outputs: Vec<ResolvedOutput> = self
            .outputs
            .iter()
            .map(|socket| ResolvedOutput {
                name: socket.name.to_owned(),
                value_type: Some(socket.value_type),
                declared: None,
                rate: socket.rate,
            })
            .collect();
        match self.config_ports {
            ConfigPorts::None | ConfigPorts::CategoryCases => {}
            ConfigPorts::Accumulators => {
                for accumulator in node.accumulators().unwrap_or_default() {
                    outputs.push(ResolvedOutput {
                        name: accumulator.name,
                        value_type: None,
                        declared: Some(accumulator.value_type),
                        rate: OutputRate::Pinned(Rate::Element),
                    });
                }
            }
            ConfigPorts::TransitionRules => {
                for rule in transition_rule_ids(node) {
                    outputs.push(ResolvedOutput {
                        name: format!("fired.{rule}"),
                        value_type: Some(BOOL_TYPE),
                        declared: None,
                        rate: OutputRate::Join,
                    });
                }
            }
            ConfigPorts::VariantCounts => {
                for variant in config_category_variants(node, document) {
                    outputs.push(ResolvedOutput {
                        name: variant,
                        value_type: Some(SocketType::Fixed(TypeClass::U64)),
                        declared: None,
                        rate: OutputRate::Pinned(Rate::Tick),
                    });
                }
            }
        }
        outputs
    }

    /// Returns the outputs of a region node that its body reads: the fixed inner outputs, then `prev.<acc>` per
    /// accumulator.
    pub fn resolved_inner_outputs(&self, node: &Node) -> Vec<ResolvedOutput> {
        let mut outputs: Vec<ResolvedOutput> = self
            .inner_outputs
            .iter()
            .map(|socket| ResolvedOutput {
                name: socket.name.to_owned(),
                value_type: Some(socket.value_type),
                declared: None,
                rate: socket.rate,
            })
            .collect();
        if self.config_ports == ConfigPorts::Accumulators {
            for accumulator in node.accumulators().unwrap_or_default() {
                outputs.push(ResolvedOutput {
                    name: format!("prev.{}", accumulator.name),
                    value_type: None,
                    declared: Some(accumulator.value_type),
                    rate: OutputRate::Pinned(Rate::Pair),
                });
            }
        }
        outputs
    }

    /// Returns the value the canonical text writes for an unwritten input `name`, or `None` for an input without a
    /// literal default.
    pub(crate) fn input_default(&self, name: &str) -> Option<InputValue> {
        self.inputs
            .iter()
            .find(|socket| socket.name == name)
            .and_then(|socket| match socket.default {
                InputDefault::Bool(value) => Some(InputValue::Literal(Literal::Bool(value))),
                InputDefault::Required | InputDefault::Optional => None,
            })
    }
}

/// Returns the variant names of the category that `config.category` of `node` names, in numbering order.
fn config_category_variants(node: &Node, document: &GraphDocument) -> Vec<String> {
    node.config
        .get("category")
        .and_then(ConfigValue::as_str)
        .and_then(|id| document.categories.iter().find(|category| category.id == id))
        .map(|category| category.variants.iter().map(|variant| variant.name.clone()).collect())
        .unwrap_or_default()
}

/// Returns the ids of the rules of `config.rules` of `node`, in list order.
pub(crate) fn transition_rule_ids(node: &Node) -> Vec<String> {
    let rules = node.config.get("rules").and_then(ConfigValue::as_array).unwrap_or(&[]);
    let mut ids = Vec::with_capacity(rules.len());
    for rule in rules {
        if let Some(id) = rule
            .as_table()
            .and_then(|table| table.get("id"))
            .and_then(ConfigValue::as_str)
            && !ids.iter().any(|known| known == id)
        {
            ids.push(id.to_owned());
        }
    }
    ids
}

/// Returns the configuration `node` runs with: its keys, and the default of each key it leaves unwritten.
pub fn effective_config(def: &NodeDef, node: &Node) -> BTreeMap<String, ConfigValue> {
    let mut config = node.config.clone();
    for key in def.config {
        if let Some(default) = key.default {
            config.entry(key.name.to_owned()).or_insert(match default {
                ConfigDefault::Bool(value) => ConfigValue::Bool(value),
                ConfigDefault::Word(word) => ConfigValue::String(word.to_owned()),
            });
        }
    }
    config
}
