//! Configuration of a node against its definition: rule V4 for configuration keys, and the accumulator and transition
//! rule lists that make ports.

use std::collections::BTreeSet;

use henad_core::params::ParamKind;

use super::Checker;
use super::view::BodyView;
use crate::catalogue::{ConfigKey, ConfigType, NodeDef, TypeClass};
use crate::ids::is_port_segment;
use crate::limits::{MAX_ACCUMULATORS, MAX_PICK_VALUES, MAX_TRANSITION_RULES};
use crate::node::{Combine, Node};
use crate::reader::read_u64;
use crate::types::ValueType;
use crate::value::{ConfigValue, Literal, MemberName};

/// Inner output names that an accumulator cannot take.
const RESERVED_ACCUMULATOR_NAMES: [&str; 2] = ["k", "total"];

/// Checks the configuration of the node at `position`, and returns whether every key fits.
pub(super) fn check_node(checker: &mut Checker<'_>, view: &BodyView<'_>, position: usize, def: &NodeDef) -> bool {
    let node = &view.nodes[position];
    let mut fits = true;
    for name in node.config.keys() {
        if !def.config.iter().any(|key| key.name == name) {
            view.error(
                checker,
                position,
                format!("{} has no configuration key `{name}`", view.name(position)),
                view.config_place(position, name),
            );
            fits = false;
        }
    }
    for key in def.config {
        match node.config.get(key.name) {
            None if key.default.is_none() => {
                view.error(
                    checker,
                    position,
                    format!("{} needs the configuration key `{}`", view.name(position), key.name),
                    view.table_place(position),
                );
                fits = false;
            }
            None => {}
            Some(value) => {
                if let Err(reason) = check_value(view, node, key, value) {
                    view.error(
                        checker,
                        position,
                        format!("{}: `config.{}` {reason}", view.name(position), key.name),
                        view.config_place(position, key.name),
                    );
                    fits = false;
                }
            }
        }
    }
    fits
}

/// Returns the clause saying how `value` breaks `key`, or `Ok` when it fits.
fn check_value(view: &BodyView<'_>, node: &Node, key: &ConfigKey, value: &ConfigValue) -> Result<(), String> {
    let document = view.document;
    let word = |what: &str| {
        value
            .as_str()
            .ok_or_else(|| format!("is no {what}, which is written as a string"))
    };
    match key.value {
        ConfigType::Bool => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| "is no `true` or `false`".to_owned()),
        ConfigType::Integer { min, max } => match value.as_integer() {
            Some(number) if (min..=max).contains(&number) => Ok(()),
            Some(number) => Err(format!("is {number}, outside {min} to {max}")),
            None => Err(format!("is no whole number from {min} to {max}")),
        },
        ConfigType::U64Text => read_u64(word("number")?)
            .map(|_| ())
            .ok_or_else(|| "is no `u64`, in decimal or `0x` hexadecimal".to_owned()),
        ConfigType::Word(words) => {
            let text = word("word")?;
            if words.contains(&text) {
                Ok(())
            } else {
                Err(format!("is '{text}', and takes one of {}", words.join(", ")))
            }
        }
        ConfigType::LaneId => {
            let text = word("lane id")?;
            if document.lanes.iter().any(|lane| lane.id == text) {
                Ok(())
            } else {
                Err(format!("names the lane '{text}', and the file declares no such lane"))
            }
        }
        ConfigType::CategoryId => {
            let text = word("category id")?;
            if document.categories.iter().any(|category| category.id == text) {
                Ok(())
            } else {
                Err(format!(
                    "names the category '{text}', and the file declares no such category"
                ))
            }
        }
        ConfigType::PaletteId => {
            let text = word("palette id")?;
            let known = document.palettes.iter().any(|palette| palette.id == text)
                || document.categories.iter().any(|category| category.id == text);
            if known {
                Ok(())
            } else {
                Err(format!(
                    "names the palette '{text}', and the file declares no such palette or category"
                ))
            }
        }
        ConfigType::VariantName => {
            let text = word("variant name")?;
            let category = node.config.get("category").and_then(ConfigValue::as_str);
            let found = document
                .categories
                .iter()
                .find(|known| Some(known.id.as_str()) == category);
            match found {
                // A broken `config.category` has its own diagnostic.
                None => Ok(()),
                Some(found) if found.variants.iter().any(|variant| variant.name == text) => Ok(()),
                Some(found) => Err(format!(
                    "names the variant '{text}', and '{}' has no such variant",
                    found.id
                )),
            }
        }
        ConfigType::CountList { max } => check_counts(value, max),
        ConfigType::Literals => check_literals(view, value),
        ConfigType::Accumulators => check_accumulators(node),
        ConfigType::AccumulatorName => word("accumulator name").map(|_| ()),
        ConfigType::TransitionRules => check_rules(view, node, value),
    }
}

/// Returns the clause saying how `value` breaks a list of counts from 0 to `max`, or `Ok` when it fits.
fn check_counts(value: &ConfigValue, max: i64) -> Result<(), String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("is no list of counts from 0 to {max}"))?;
    if items
        .iter()
        .all(|item| item.as_integer().is_some_and(|number| (0..=max).contains(&number)))
    {
        Ok(())
    } else {
        Err(format!("holds a value outside the counts from 0 to {max}"))
    }
}

/// Returns the clause saying how `value` breaks a list of literals of one type, or `Ok` when it fits.
fn check_literals(view: &BodyView<'_>, value: &ConfigValue) -> Result<(), String> {
    let items = value.as_array().ok_or_else(|| "is no list of literals".to_owned())?;
    if items.is_empty() || items.len() > MAX_PICK_VALUES {
        return Err(format!(
            "holds {} values, and holds 1 to {MAX_PICK_VALUES}",
            items.len()
        ));
    }
    let mut first: Option<ValueType> = None;
    for item in items {
        let literal = literal_of(item).ok_or_else(|| "holds a value that is no one-key literal table".to_owned())?;
        check_literal(view, &literal)?;
        let value_type = literal.value_type();
        match &first {
            None => first = Some(value_type),
            Some(known) if *known == value_type => {}
            Some(known) => return Err(format!("mixes the types {known} and {value_type}")),
        }
    }
    Ok(())
}

/// Returns the clause saying how the accumulators of `node` break the region rules, or `Ok` when they fit.
fn check_accumulators(node: &Node) -> Result<(), String> {
    let accumulators = node.accumulators().map_err(|reason| format!("is broken: {reason}"))?;
    if accumulators.len() > MAX_ACCUMULATORS {
        return Err(format!(
            "holds {} accumulators, and a region holds at most {MAX_ACCUMULATORS}",
            accumulators.len()
        ));
    }
    let mut names = BTreeSet::new();
    for accumulator in &accumulators {
        let name = accumulator.name.as_str();
        if !is_port_segment(name) {
            return Err(format!(
                "names the accumulator '{name}', outside the grammar of a port name"
            ));
        }
        if RESERVED_ACCUMULATOR_NAMES.contains(&name) {
            return Err(format!(
                "names an accumulator '{name}', which an inner output of the region holds"
            ));
        }
        if !names.insert(name) {
            return Err(format!("names the accumulator '{name}' twice"));
        }
        if !combine_fits(accumulator.combine, &accumulator.value_type) {
            return Err(format!(
                "gives the accumulator '{name}' the combine `{}`, which does not apply to {}",
                accumulator.combine.as_str(),
                accumulator.value_type
            ));
        }
    }
    Ok(())
}

/// Returns whether `combine` applies to `value_type`.
fn combine_fits(combine: Combine, value_type: &ValueType) -> bool {
    const NUMERIC: [TypeClass; 8] = [
        TypeClass::U32,
        TypeClass::I32,
        TypeClass::F32,
        TypeClass::Vec2U,
        TypeClass::Vec2I,
        TypeClass::Vec2F,
        TypeClass::U64,
        TypeClass::F64,
    ];
    match combine {
        Combine::Sum | Combine::Min | Combine::Max => NUMERIC.iter().any(|class| class.holds(value_type)),
        Combine::Count => *value_type == ValueType::U32,
        Combine::Or | Combine::And => *value_type == ValueType::Bool,
        Combine::Carry => true,
    }
}

/// Returns the clause saying how the rules of `value` break the transition rules, or `Ok` when they fit.
fn check_rules(view: &BodyView<'_>, node: &Node, value: &ConfigValue) -> Result<(), String> {
    let rules = value.as_array().ok_or_else(|| "is no list of rules".to_owned())?;
    if rules.len() > MAX_TRANSITION_RULES {
        return Err(format!(
            "holds {} rules, and a node holds at most {MAX_TRANSITION_RULES}",
            rules.len()
        ));
    }
    let category = node.config.get("category").and_then(ConfigValue::as_str);
    let variants = view
        .document
        .categories
        .iter()
        .find(|known| Some(known.id.as_str()) == category);
    let mut ids = BTreeSet::new();
    for rule in rules {
        let table = rule
            .as_table()
            .ok_or_else(|| "holds a rule that is no `{ id, from, to, phase }` table".to_owned())?;
        for key in table.keys() {
            if !["id", "from", "to", "phase"].contains(&key.as_str()) {
                return Err(format!("holds a rule with the unknown key `{key}`"));
            }
        }
        let id = table
            .get("id")
            .and_then(ConfigValue::as_str)
            .ok_or_else(|| "holds a rule with no string `id`".to_owned())?;
        if !is_port_segment(id) {
            return Err(format!("names the rule '{id}', outside the grammar of a port name"));
        }
        if !ids.insert(id) {
            return Err(format!("names the rule '{id}' twice"));
        }
        // A `phase` is a `u32`, compared as a number.
        // Gaps are allowed, and a rule without one has no place in the order.
        match table.get("phase").and_then(ConfigValue::as_integer) {
            Some(phase) if u32::try_from(phase).is_ok() => {}
            _ => {
                return Err(format!(
                    "gives the rule '{id}' a `phase` that is no number from 0 to {}",
                    u32::MAX
                ));
            }
        }
        let from = table
            .get("from")
            .and_then(ConfigValue::as_str)
            .ok_or_else(|| format!("gives the rule '{id}' no string `from`"))?;
        for (key, variant) in [
            ("from", Some(from)),
            ("to", table.get("to").and_then(ConfigValue::as_str)),
        ] {
            if let (Some(variant), Some(variants)) = (variant, variants)
                && !variants.variants.iter().any(|known| known.name == variant)
            {
                return Err(format!(
                    "gives the rule '{id}' the `{key}` variant '{variant}', and '{}' has no such variant",
                    variants.id
                ));
            }
        }
        if table.contains_key("to") && table.get("to").and_then(ConfigValue::as_str).is_none() {
            return Err(format!("gives the rule '{id}' a `to` that is no string"));
        }
    }
    Ok(())
}

/// Returns the literal a one-key configuration table holds, as an element of `config.values`.
pub(crate) fn literal_of(value: &ConfigValue) -> Option<Literal> {
    let table = value.as_table()?;
    let mut entries = table.iter();
    let (tag, inner) = entries.next()?;
    if entries.next().is_some() {
        return None;
    }
    let member = |text: &str| {
        text.split_once('.').map(|(owner, member)| MemberName {
            owner: owner.to_owned(),
            member: member.to_owned(),
        })
    };
    let float = |value: &ConfigValue| match value {
        ConfigValue::Float(number) if number.is_finite() => Some(*number),
        _ => None,
    };
    Some(match tag.as_str() {
        "bool" => Literal::Bool(inner.as_bool()?),
        "u32" => Literal::U32(u32::try_from(inner.as_integer()?).ok()?),
        "i32" => Literal::I32(i32::try_from(inner.as_integer()?).ok()?),
        "f32" => Literal::F32(float(inner)? as f32),
        "f64" => Literal::F64(float(inner)?),
        "u64" => Literal::U64(read_u64(inner.as_str()?)?),
        "vec2u" => {
            let [x, y] = inner.as_array()? else { return None };
            Literal::Vec2U([
                u32::try_from(x.as_integer()?).ok()?,
                u32::try_from(y.as_integer()?).ok()?,
            ])
        }
        "vec2i" => {
            let [x, y] = inner.as_array()? else { return None };
            Literal::Vec2I([
                i32::try_from(x.as_integer()?).ok()?,
                i32::try_from(y.as_integer()?).ok()?,
            ])
        }
        "vec2f" => {
            let [x, y] = inner.as_array()? else { return None };
            Literal::Vec2F([float(x)? as f32, float(y)? as f32])
        }
        "cat" => Literal::Category(member(inner.as_str()?)?),
        "option" => Literal::ChoiceOption(member(inner.as_str()?)?),
        _ => return None,
    })
}

/// Returns the clause saying what `literal` names that the file does not declare, or `Ok` when every name resolves.
pub(super) fn check_literal(view: &BodyView<'_>, literal: &Literal) -> Result<(), String> {
    let document = view.document;
    match literal {
        Literal::Category(name) => {
            let Some(category) = document.categories.iter().find(|category| category.id == name.owner) else {
                return Err(format!(
                    "names the category '{}', and the file declares no such category",
                    name.owner
                ));
            };
            if category.variants.iter().any(|variant| variant.name == name.member) {
                Ok(())
            } else {
                Err(format!(
                    "names the variant '{}', and '{}' has no such variant",
                    name.member, name.owner
                ))
            }
        }
        Literal::Layer(name) => {
            let Some(field) = document.fields.iter().find(|field| field.id == name.owner) else {
                return Err(format!(
                    "names the field '{}', and the file declares no such field",
                    name.owner
                ));
            };
            if field.layers.contains(&name.member) {
                Ok(())
            } else {
                Err(format!(
                    "names the layer '{}', and '{}' has no such layer",
                    name.member, name.owner
                ))
            }
        }
        Literal::ChoiceOption(name) => {
            let Some(param) = document.params.iter().find(|param| param.id() == name.owner) else {
                return Err(format!(
                    "names the parameter '{}', and the file declares no such parameter",
                    name.owner
                ));
            };
            let ParamKind::Choice { options, .. } = param.kind() else {
                return Err(format!(
                    "names an option of '{}', which is no choice parameter",
                    name.owner
                ));
            };
            if options.iter().any(|option| option == name.member) {
                Ok(())
            } else {
                Err(format!(
                    "names the option '{}', and '{}' has no such option",
                    name.member, name.owner
                ))
            }
        }
        _ => Ok(()),
    }
}

/// Returns the type a parameter binding has.
pub(super) fn param_type(kind: &ParamKind) -> ValueType {
    match kind {
        ParamKind::F32 { .. } => ValueType::F32,
        ParamKind::Bool { .. } => ValueType::Bool,
        _ => ValueType::U32,
    }
}
