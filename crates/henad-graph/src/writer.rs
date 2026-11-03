//! Canonical writer of a [`GraphDocument`].
//!
//! The text is a function of the document alone: UTF-8, `\n` line ends, no comments, one blank line between tables, and
//! `key = value` with single spaces. Tables follow a fixed order, declarations keep their order, passes run in stack
//! order and nodes in natural id order.

use std::collections::BTreeMap;

use henad_core::authoring::primitives::space::Boundary;

use crate::document::{Category, Function, GraphDocument, Lane, Pass, PassEnabled, World};
use crate::file::{FORMAT, FORMAT_VERSION, KIND_GRAPH};
use crate::model_text::{toml_string, write_param};
use crate::node::{Node, NodeBody};
use crate::value::{Color, ConfigValue, InputValue, Literal};

/// Keys of an inline table of a node's configuration, in the order they are written. A key not listed follows, in
/// lexical order.
const INLINE_KEY_ORDER: [&str; 8] = ["id", "name", "label", "type", "combine", "from", "to", "phase"];

/// Returns the canonical text of `document`.
pub(crate) fn semantic_text(document: &GraphDocument) -> String {
    let mut text = String::new();
    write_header(&mut text, document);
    write_world(&mut text, &document.world);
    for category in &document.categories {
        write_category(&mut text, category);
    }
    for palette in &document.palettes {
        text.push_str("\n[[palette]]\n");
        key(&mut text, "id", &toml_string(&palette.id));
        let colors: Vec<String> = palette.colors.iter().map(color).collect();
        key(&mut text, "colors", &format!("[{}]", colors.join(", ")));
    }
    for lane in &document.lanes {
        write_lane(&mut text, lane);
    }
    for field in &document.fields {
        text.push_str("\n[[field]]\n");
        key(&mut text, "id", &toml_string(&field.id));
        key(&mut text, "type", &toml_string(&field.value_type.to_string()));
        key(&mut text, "layers", &strings(&field.layers));
        key(&mut text, "static", &field.is_static.to_string());
        if let Some(combine) = field.combine {
            key(&mut text, "combine", &toml_string(combine.as_str()));
        }
        if let Some(finish) = &field.finish {
            key(&mut text, "finish", &toml_string(finish));
        }
        if let Some(initial) = &field.initial {
            key(&mut text, "initial", &literal(initial));
        }
    }
    for counter in &document.counters {
        text.push_str("\n[[counter]]\n");
        key(&mut text, "id", &toml_string(&counter.id));
    }
    for param in &document.params {
        text.push_str("\n[[param]]\n");
        write_param(&mut text, param);
    }
    for stat in &document.stats {
        text.push_str("\n[[stat]]\n");
        key(&mut text, "label", &toml_string(&stat.label));
        key(&mut text, "from", &toml_string(&stat.from.to_string()));
        key(&mut text, "color", &color(&stat.color));
    }
    for action in &document.actions {
        text.push_str("\n[[action]]\n");
        key(&mut text, "id", &toml_string(&action.id));
        key(&mut text, "label", &toml_string(&action.label));
        key(&mut text, "runs", &strings(&action.runs));
    }
    for function in &document.functions {
        write_function(&mut text, function);
    }
    let mut passes: Vec<&Pass> = document.passes.iter().collect();
    passes.sort_by_key(|pass| pass.stage);
    for pass in passes {
        write_pass(&mut text, pass);
    }
    text
}

fn write_header(text: &mut String, document: &GraphDocument) {
    key(text, "format", &toml_string(FORMAT));
    key(text, "format_version", &FORMAT_VERSION.to_string());
    key(text, "kind", &toml_string(KIND_GRAPH));
    key(text, "id", &toml_string(&document.id));
    key(text, "name", &toml_string(&document.name));
    key(text, "description", &toml_string(&document.description));
    text.push_str("\n[graph]\n");
    key(text, "topology", &toml_string(document.world.topology().as_str()));
    key(text, "library", &document.library.to_string());
    text.push_str("\n[[target]]\n");
    key(text, "backend", "\"gpu\"");
    key(text, "id", &toml_string(&document.target.id));
    key(text, "name", &toml_string(&document.target.name));
    key(text, "description", &toml_string(&document.target.description));
}

fn write_world(text: &mut String, world: &World) {
    match world {
        World::Grid(grid) => {
            text.push_str("\n[grid]\n");
            key(text, "width", &grid.width.to_string());
            key(text, "height", &grid.height.to_string());
            let boundary = match grid.boundary {
                Boundary::Torus => "torus",
                Boundary::Bounded => "bounded",
            };
            key(text, "boundary", &toml_string(boundary));
        }
        World::Agents(agents) => {
            text.push_str("\n[agents]\n");
            key(text, "count", &agents.count.to_string());
            key(text, "max_count", &agents.max_count.to_string());
            key(text, "width", &format!("{:?}", agents.width));
            key(text, "height", &format!("{:?}", agents.height));
        }
    }
}

fn write_category(text: &mut String, category: &Category) {
    text.push_str("\n[[category]]\n");
    key(text, "id", &toml_string(&category.id));
    let variants: Vec<String> = category
        .variants
        .iter()
        .map(|variant| {
            format!(
                "{{ name = {}, label = {}, color = {} }}",
                toml_string(&variant.name),
                toml_string(&variant.label),
                color(&variant.color)
            )
        })
        .collect();
    key(text, "variants", &multiline_array(&variants));
}

fn write_lane(text: &mut String, lane: &Lane) {
    text.push_str("\n[[lane]]\n");
    key(text, "id", &toml_string(&lane.id));
    key(text, "domain", &toml_string(lane.domain.as_str()));
    key(text, "type", &toml_string(&lane.value_type.to_string()));
    if let Some(storage) = lane.storage {
        key(text, "storage", &toml_string(storage.as_str()));
    }
    if let Some(initial) = &lane.initial {
        key(text, "initial", &literal(initial));
    }
    if let Some(role) = lane.role {
        key(text, "role", &toml_string(role.as_str()));
    }
}

fn write_function(text: &mut String, function: &Function) {
    text.push_str("\n[[function]]\n");
    key(text, "id", &toml_string(&function.id));
    key(text, "kind", "\"pure\"");
    for (table, ports) in [("input", &function.inputs), ("output", &function.outputs)] {
        for port in ports {
            text.push_str(&format!("\n[[function.{table}]]\n"));
            key(text, "port", &toml_string(&port.port));
            key(text, "type", &toml_string(&port.value_type.to_string()));
        }
    }
    write_body(text, "function", &function.body);
}

fn write_pass(text: &mut String, pass: &Pass) {
    text.push_str("\n[[pass]]\n");
    key(text, "id", &toml_string(&pass.id));
    key(text, "stage", &toml_string(pass.stage.as_str()));
    if let Some(action) = &pass.action {
        key(text, "action", &toml_string(action));
    }
    key(text, "kind", "\"map\"");
    if let Some(domain) = pass.domain {
        key(text, "domain", &toml_string(domain.as_str()));
    }
    let enabled = match &pass.enabled {
        PassEnabled::Literal(value) => value.to_string(),
        PassEnabled::Param(id) => format!("{{ param = {} }}", toml_string(id)),
    };
    key(text, "enabled", &enabled);
    if !pass.draw_order.is_empty() {
        let ids: Vec<String> = pass.draw_order.iter().map(ToString::to_string).collect();
        key(text, "draw_order", &strings(&ids));
    }
    write_body(text, "pass", &pass.body);
}

fn write_body(text: &mut String, parent: &str, body: &NodeBody) {
    let mut nodes: Vec<&Node> = body.nodes.iter().collect();
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        text.push_str(&format!("\n[[{parent}.node]]\n"));
        write_node(text, node);
    }
}

fn write_node(text: &mut String, node: &Node) {
    key(text, "id", &toml_string(&node.id.to_string()));
    key(text, "op", &toml_string(&node.op));
    for (name, value) in &node.config {
        key(text, &format!("config.{name}"), &config_value(value));
    }
    for (port, value) in &node.inputs {
        key(text, &format!("in.{port}"), &input_value(value));
    }
    if let Some(region) = node.region {
        key(text, "region", &toml_string(&region.to_string()));
    }
}

/// Appends the line `name = value`.
fn key(text: &mut String, name: &str, value: &str) {
    text.push_str(name);
    text.push_str(" = ");
    text.push_str(value);
    text.push('\n');
}

fn color(color: &Color) -> String {
    let [red, green, blue, alpha] = color;
    format!("[{red}, {green}, {blue}, {alpha}]")
}

fn strings(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| toml_string(item)).collect();
    format!("[{}]", items.join(", "))
}

/// Returns an array with one element per line and a trailing comma, or `[]` with no element.
fn multiline_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_owned();
    }
    let mut text = String::from("[\n");
    for item in items {
        text.push_str("  ");
        text.push_str(item);
        text.push_str(",\n");
    }
    text.push(']');
    text
}

fn input_value(value: &InputValue) -> String {
    match value {
        InputValue::Link(reference) => toml_string(&reference.to_string()),
        InputValue::Literal(constant) => literal(constant),
        InputValue::Param(id) => format!("{{ param = {} }}", toml_string(id)),
    }
}

/// Returns the one-key table of `value`.
fn literal(value: &Literal) -> String {
    match value {
        Literal::Bool(flag) => format!("{{ bool = {flag} }}"),
        Literal::U32(number) => format!("{{ u32 = {number} }}"),
        Literal::I32(number) => format!("{{ i32 = {number} }}"),
        Literal::F32(number) => format!("{{ f32 = {number:?} }}"),
        Literal::U64(number) => format!("{{ u64 = \"{number:#x}\" }}"),
        Literal::F64(number) => format!("{{ f64 = {number:?} }}"),
        Literal::Vec2U([x, y]) => format!("{{ vec2u = [{x}, {y}] }}"),
        Literal::Vec2I([x, y]) => format!("{{ vec2i = [{x}, {y}] }}"),
        Literal::Vec2F([x, y]) => format!("{{ vec2f = [{x:?}, {y:?}] }}"),
        Literal::Category(name) => format!("{{ cat = {} }}", toml_string(&name.to_string())),
        Literal::Layer(name) => format!("{{ layer = {} }}", toml_string(&name.to_string())),
        Literal::ChoiceOption(name) => format!("{{ option = {} }}", toml_string(&name.to_string())),
    }
}

fn config_value(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Bool(flag) => flag.to_string(),
        ConfigValue::Integer(number) => number.to_string(),
        ConfigValue::Float(number) => format!("{number:?}"),
        ConfigValue::String(text) => toml_string(text),
        ConfigValue::Array(items) => {
            let written: Vec<String> = items.iter().map(config_value).collect();
            if items.iter().any(|item| matches!(item, ConfigValue::Table(_))) {
                multiline_array(&written)
            } else {
                format!("[{}]", written.join(", "))
            }
        }
        ConfigValue::Table(table) => inline_table(table),
    }
}

fn inline_table(table: &BTreeMap<String, ConfigValue>) -> String {
    let rank = |name: &str| {
        INLINE_KEY_ORDER
            .iter()
            .position(|listed| *listed == name)
            .unwrap_or(INLINE_KEY_ORDER.len())
    };
    let mut entries: Vec<(&String, &ConfigValue)> = table.iter().collect();
    // A stable sort keeps lexical order among the keys the list leaves out.
    entries.sort_by_key(|(name, _)| rank(name));
    let written: Vec<String> = entries
        .iter()
        .map(|(name, value)| format!("{name} = {}", inline_config_value(value)))
        .collect();
    if written.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", written.join(", "))
    }
}

/// Returns a value inside an inline table, on one line since TOML 1.0 keeps an inline table on one line.
fn inline_config_value(value: &ConfigValue) -> String {
    match value {
        ConfigValue::Array(items) => {
            let written: Vec<String> = items.iter().map(inline_config_value).collect();
            format!("[{}]", written.join(", "))
        }
        _ => config_value(value),
    }
}
