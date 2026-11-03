//! Graph files built from known-answer cases, so a case's shape goes through the reader and the checker.

use crate::catalogue::{CaseForm, CaseValue, KnownAnswer, NodeCase, NodeDef, PassCase};

/// Declarations every case file holds: the categories and the palette the cases name.
const CASE_WORLD: &str = r#"
[[category]]
id = "health"
variants = [
  { name = "S", label = "S", color = [0, 0, 255] },
  { name = "I", label = "I", color = [255, 0, 0] },
  { name = "R", label = "R", color = [128, 128, 128] },
]

[[category]]
id = "life"
variants = [
  { name = "Dead", label = "Dead", color = [0, 0, 0] },
  { name = "Alive", label = "Alive", color = [0, 255, 0] },
]

[[palette]]
id = "ramp"
colors = [[0, 0, 0], [85, 85, 85], [170, 170, 170], [255, 255, 255]]
"#;

/// A tick pass over a lane `out`, for a case file whose case runs in another stage.
const TICK_PASS: &str = r#"
[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n100"
op = "lane.get"
config.lane = "out"

[[pass.node]]
id = "n101"
op = "lane.set"
config.lane = "out"
in.value = "n100"
"#;

/// Returns the header of a case file over a grid of `width` by `height`.
fn header(width: u32, height: u32, bounded: bool) -> String {
    let boundary = if bounded { "bounded" } else { "torus" };
    format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"case\"\n\n[graph]\ntopology = \"grid\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[grid]\nwidth = {width}\nheight = {height}\nboundary = \"{boundary}\"\n{CASE_WORLD}"
    )
}

/// Returns the header of a case file over `agents` agents in a world of `width` by `height` world units.
pub(super) fn agents_header(agents: u32, width: u32, height: u32) -> String {
    format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"case\"\n\n[graph]\ntopology = \"agents\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[agents]\ncount = {agents}\nmax_count = {agents}\nwidth = {width}.0\nheight = {height}.0\n{CASE_WORLD}"
    )
}

/// Returns the literal a case value is written as, with a float the format cannot hold written as 0.
pub(super) fn literal(value: &CaseValue) -> String {
    let float = |number: f32| {
        if number.is_finite() {
            format!("{number:?}")
        } else {
            "0.0".to_owned()
        }
    };
    match value {
        CaseValue::Bool(flag) => format!("{{ bool = {flag} }}"),
        CaseValue::U32(number) => format!("{{ u32 = {number} }}"),
        CaseValue::I32(number) => format!("{{ i32 = {number} }}"),
        CaseValue::F32(number) | CaseValue::Near(number, _) => format!("{{ f32 = {} }}", float(*number)),
        CaseValue::Nan => "{ f32 = 0.0 }".to_owned(),
        CaseValue::U64(number) => format!("{{ u64 = \"{number:#x}\" }}"),
        CaseValue::F64(number) => format!("{{ f64 = {number:?} }}"),
        CaseValue::Vec2U([x, y]) => format!("{{ vec2u = [{x}, {y}] }}"),
        CaseValue::Vec2I([x, y]) => format!("{{ vec2i = [{x}, {y}] }}"),
        CaseValue::Vec2F([x, y]) | CaseValue::NearVec2F([x, y], _) => {
            format!("{{ vec2f = [{}, {}] }}", float(*x), float(*y))
        }
        CaseValue::Vec2D([x, _]) => format!("{{ f64 = {x:?} }}"),
        CaseValue::Variant(category, variant) => format!("{{ cat = \"{category}.{variant}\" }}"),
    }
}

/// Returns the stage a node case of `def` runs in: the first stage of tick, init, action, view and stats it allows.
fn case_stage(def: &NodeDef) -> &'static str {
    let contexts = def.contexts;
    [
        (contexts.tick, "tick"),
        (contexts.init, "init"),
        (contexts.action, "action"),
        (contexts.view, "view"),
        (contexts.stats, "stats"),
    ]
    .into_iter()
    .find_map(|(allowed, stage)| allowed.then_some(stage))
    .unwrap_or("tick")
}

/// Returns the file of a node case: the node `n1` with literal inputs, in a pass of the first stage its kind allows.
pub(super) fn node_case_file(def: &NodeDef, case: &NodeCase) -> String {
    let stage = case_stage(def);
    let mut text = header(1, 1, false);
    text.push_str("\n[[lane]]\nid = \"out\"\ndomain = \"cells\"\ntype = \"u32\"\n");
    if stage == "action" {
        text.push_str("\n[[action]]\nid = \"press\"\nlabel = \"Press\"\n");
    }
    text.push_str(&format!(
        "\n[[pass]]\nid = \"case\"\nstage = \"{stage}\"\nkind = \"map\"\ndomain = \"cells\"\n"
    ));
    if stage == "action" {
        text.push_str("action = \"press\"\n");
    }
    if def.has_effect(crate::catalogue::Effect::DrawsStream) {
        text.push_str("draw_order = [\"n1\"]\n");
    }
    text.push_str(&format!(
        "\n[[pass.node]]\nid = \"n1\"\nop = \"{}\"\n{}",
        def.id, case.config
    ));
    for (port, value) in case.inputs {
        text.push_str(&format!("in.{port} = {}\n", literal(value)));
    }
    if stage != "tick" {
        text.push_str(TICK_PASS);
    }
    text
}

/// Returns the file of a pass case.
pub(super) fn pass_case_file(case: &PassCase) -> String {
    let mut text = if case.agents > 0 {
        agents_header(case.agents, case.width, case.height)
    } else {
        header(case.width, case.height, case.bounded)
    };
    text.push('\n');
    text.push_str(case.tables);
    text
}

/// Returns the file of `case` of `def`.
pub(super) fn case_file(def: &NodeDef, case: &KnownAnswer) -> String {
    match &case.form {
        CaseForm::Node(node) => node_case_file(def, node),
        CaseForm::Pass(pass) => pass_case_file(pass),
    }
}
