//! The WGSL compiler: every starter compiles, no step returns early, the names and the range table, and the forms of
//! regions.

use henad_compute::gpu::wgpu;
use henad_core::params::ParamValue;

use super::support::{big_category, node, powi_flood_tables, tally_pass};
use crate::GraphFile;
use crate::starters::starters;
use crate::wgsl::{GeneratedShader, GridProgram, ShaderRole};

/// Writes the text of each of `shaders`, the shaders of the model `id`, to the directory `HENAD_DUMP_GRAPH_WGSL` names,
/// and nothing when it is unset.
fn dump(id: &str, shaders: &[GeneratedShader]) {
    if let Some(directory) = std::env::var_os("HENAD_DUMP_GRAPH_WGSL") {
        for shader in shaders {
            let path = std::path::Path::new(&directory).join(format!("{id}_{}.wgsl", shader.label()));
            std::fs::write(path, shader.text()).expect("a dump");
        }
    }
}

#[test]
fn every_starter_compiles() {
    for starter in starters() {
        let file = GraphFile::parse(starter.text).expect("the starter reads");
        let program =
            GridProgram::compile(&file).unwrap_or_else(|problems| panic!("{}: {problems:#?}", starter.file_name));
        dump(program.id(), program.shaders());
    }
}

/// Every agent starter and every agents fixture compiles, and `HENAD_DUMP_GRAPH_WGSL` writes each shader's text.
#[test]
fn every_agents_graph_compiles() {
    for (name, file) in super::corpus::agents() {
        let program =
            crate::wgsl::AgentProgram::compile(&file).unwrap_or_else(|problems| panic!("{name}: {problems:#?}"));
        dump(program.id(), program.shaders());
    }
}

/// No shader of a starter or a fixture but the display returns early, grid or agents, steps, actions, colour passes
/// and field updates included, so a workgroup barrier at the end of `main` sits in uniform control flow.
#[test]
fn no_step_returns_early() {
    let files = super::corpus::all();
    let mut roles = std::collections::BTreeSet::new();
    for (name, text) in files {
        let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let shaders = if matches!(file.document().world, crate::document::World::Agents(_)) {
            crate::wgsl::AgentProgram::compile(&file)
                .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"))
                .shaders()
                .to_vec()
        } else {
            GridProgram::compile(&file)
                .unwrap_or_else(|problems| panic!("{name}: {problems:#?}"))
                .shaders()
                .to_vec()
        };
        for shader in shaders.iter().filter(|shader| *shader.role() != ShaderRole::Display) {
            roles.insert(match shader.role() {
                ShaderRole::Step => "step",
                ShaderRole::Display => "display",
                ShaderRole::Reduce => "reduce",
                ShaderRole::Action(_) => "action",
                ShaderRole::Color => "color",
                ShaderRole::Merge(_) => "merge",
            });
            let text = shader.text();
            let main = &text[text.find("fn main(").expect("a main")..];
            assert!(!main.contains("return"), "{name} {}: \n{text}", shader.label());
        }
    }
    assert_eq!(
        roles.into_iter().collect::<Vec<_>>(),
        ["action", "color", "merge", "reduce", "step"],
        "every role but the display"
    );
}

/// Every node of a pass names the text it wrote, and a byte of a line of the step names the node that wrote it.
#[test]
fn every_node_owns_the_text_it_wrote() {
    let starter = crate::starters::starter("game_of_life.toml").expect("the Life starter");
    let file = GraphFile::parse(starter.text).expect("the starter reads");
    let program = GridProgram::compile(&file).expect("the starter compiles");
    let step = program
        .shaders()
        .iter()
        .find(|shader| shader.label() == "step")
        .expect("the step shader");
    let pass = file
        .document()
        .passes
        .iter()
        .find(|pass| pass.id == "step")
        .expect("the step pass");
    for node in &pass.body.nodes {
        let range = step
            .ranges()
            .iter()
            .find(|range| range.node == node.id)
            .unwrap_or_else(|| panic!("{} wrote no text", node.id));
        assert_eq!(range.pass, "step");
        assert!(
            step.text()[range.range.clone()].contains(&format!("n{}_", node.id.number())) || node.op == "lane.set",
            "{}: {}",
            node.id,
            &step.text()[range.range.clone()]
        );
    }
    let offset = step.text().find("let n12_next").expect("the rule's line");
    assert_eq!(
        step.node_at(offset).map(|(_, node)| node.to_string()),
        Some("n12".to_owned())
    );
    let count = step
        .text()
        .find("henad_at(1, 1)")
        .expect("the last neighbour of the count");
    assert_eq!(
        step.node_at(count).map(|(_, node)| node.to_string()),
        Some("n11".to_owned())
    );
    assert_eq!(step.node_at(0), None, "the template's header belongs to no node");
    assert!(step.composed().contains("fn main"));
}

/// A grid count is unrolled, one test per neighbour, and a Repeat is a counted loop the compiler writes.
#[test]
fn regions_lower_to_unrolled_tests_and_counted_loops() {
    let starter = crate::starters::starter("game_of_life.toml").expect("the Life starter");
    let program = GridProgram::compile(&GraphFile::parse(starter.text).expect("reads")).expect("compiles");
    let step = program
        .shaders()
        .iter()
        .find(|shader| shader.label() == "step")
        .expect("the step");
    assert_eq!(
        step.text().matches("henad_at(").count(),
        8 + 1,
        "eight neighbours and the helper"
    );
    assert!(!step.text().contains("for ("), "no loop in a pass without a Repeat");

    let repeat = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_trials"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 8
height = 8
boundary = "torus"

[[lane]]
id = "hits"
domain = "cells"
type = "u32"

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "hits", type = "u32", combine = "sum" }]
in.count = { u32 = 3 }

[[pass.node]]
id = "n2"
op = "rand.below"
region = "n1"
config.key = 99
in.threshold = { u32 = 2147483648 }

[[pass.node]]
id = "n3"
op = "conv.to_u32"
region = "n1"
in.a = "n2"

[[pass.node]]
id = "n4"
op = "region.put"
region = "n1"
config.acc = "hits"
in.value = "n3"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "hits"
in.value = "n1.hits"
"#;
    let file = GraphFile::parse(repeat).expect("the Repeat file reads");
    let program = GridProgram::compile(&file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let step = program
        .shaders()
        .iter()
        .find(|shader| shader.label() == "step")
        .expect("the step");
    let text = step.text();
    assert!(text.contains("var n1_hits: u32 = 0u;"), "{text}");
    assert!(text.contains("let n1_total = select(0u, min(3u, 4u), true);"), "{text}");
    assert!(text.contains("let n1_prev__hits = n1_hits;"), "{text}");
    assert!(text.contains("for (var n1_k = 0u;"), "{text}");
    assert!(
        text.contains("draw_core(params.henad_key_n2, henad_index, henad_tick, henad_iteration_n1, 0u)"),
        "{text}"
    );
}

/// Returns a grid file of a `u32` lane `x`, an `f32` lane `y` and an `f32` parameter `share`, with `passes`.
fn grid_file(passes: &str) -> String {
    format!(
        r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_probe"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 8
height = 8
boundary = "torus"

[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[lane]]
id = "y"
domain = "cells"
type = "f32"

[[param]]
id = "share"
label = "Share"
kind = "f32"
min = 0.0
max = 1.0
default = 0.5
{passes}"#
    )
}

/// Returns the nodes of a Repeat of three iterations whose body holds `body` and puts the value of node `n9` into the
/// `u32` carry `out`, and a write of `out` to lane `x`.
pub(super) fn repeat_nodes(body: &str) -> String {
    format!(
        r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 3
config.acc = [{{ name = "out", type = "u32", combine = "carry" }}]
in.count = {{ u32 = 3 }}
in.init.out = {{ u32 = 0 }}
{body}
[[pass.node]]
id = "n8"
op = "region.put"
config.acc = "out"
in.value = "n9"
region = "n1"

[[pass.node]]
id = "n10"
op = "lane.set"
config.lane = "x"
in.value = "n1.out"
"#
    )
}

/// Returns pass `id` of stage `stage` holding the [`repeat_nodes`] of `body`. An action pass belongs to `poke`.
fn repeat_pass(id: &str, stage: &str, body: &str) -> String {
    let action = if stage == "action" { "action = \"poke\"\n" } else { "" };
    let nodes = repeat_nodes(body);
    format!("\n[[pass]]\nid = \"{id}\"\nstage = \"{stage}\"\n{action}kind = \"map\"\ndomain = \"cells\"\n{nodes}")
}

/// Body nodes of [`repeat_nodes`] that add the lane seed to the carry each iteration.
pub(super) const LANE_SEED_BODY: &str = r#"
[[pass.node]]
id = "n2"
op = "rng.lane_seed"
config.salt = "0x8040e00294134cd9"
region = "n1"

[[pass.node]]
id = "n9"
op = "math.add"
in.a = "n1.prev.out"
in.b = "n2"
region = "n1"
"#;

/// Body nodes of [`repeat_nodes`] that add the press word to the carry each iteration.
const PRESS_SEED_BODY: &str = r#"
[[pass.node]]
id = "n2"
op = "ctx.press_seed"
region = "n1"

[[pass.node]]
id = "n9"
op = "math.add"
in.a = "n1.prev.out"
in.b = "n2"
region = "n1"
"#;

/// Body nodes of [`repeat_nodes`] that raise `share * 3` to the power `k` and carry the largest power so far, times
/// 1000 as a `u32`. The base is computed in the body from a parameter alone.
pub(super) const POWI_BODY: &str = r#"
[[pass.node]]
id = "n2"
op = "math.mul"
in.a = { param = "share" }
in.b = { f32 = 3.0 }
region = "n1"

[[pass.node]]
id = "n3"
op = "math.powi"
config.max_exponent = 8
in.base = "n2"
in.exponent = "n1.k"
region = "n1"

[[pass.node]]
id = "n4"
op = "math.mul"
in.a = "n3"
in.b = { f32 = 1000.0 }
region = "n1"

[[pass.node]]
id = "n5"
op = "conv.to_u32"
in.a = "n4"
region = "n1"

[[pass.node]]
id = "n9"
op = "math.max"
in.a = "n1.prev.out"
in.b = "n5"
region = "n1"
"#;

/// A value of a Repeat body that is the same in every iteration and every element compiles. The host's prelude
/// computes it, and the shader reads it from the uniform block: a lane seed and a powi base in a tick pass, and a press
/// word in an action pass.
#[test]
fn uniform_values_of_a_repeat_body_come_from_the_uniform_block() {
    let keep = "\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\n\
                id = \"n20\"\nop = \"lane.get\"\nconfig.lane = \"x\"\n";
    let cases = [
        (
            "lane seed",
            repeat_pass("step", "tick", LANE_SEED_BODY),
            "step",
            "n2_out",
        ),
        (
            "press word",
            format!(
                "{keep}\n[[action]]\nid = \"poke\"\nlabel = \"Poke\"\nruns = []\n{}",
                repeat_pass("poke_pass", "action", PRESS_SEED_BODY)
            ),
            "poke_pass",
            "n2_seed",
        ),
        (
            "powi base",
            repeat_pass("step", "tick", POWI_BODY),
            "step",
            "henad_powers_n3",
        ),
    ];
    for (name, passes, label, member) in cases {
        let text = grid_file(&passes);
        let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}\n{text}"));
        let program = GridProgram::compile(&file).unwrap_or_else(|problems| panic!("{name}: {problems:#?}"));
        let shader = program
            .shaders()
            .iter()
            .find(|shader| shader.label() == label)
            .unwrap_or_else(|| panic!("{name}: the shader '{label}'"));
        assert!(
            shader.text().contains(&format!("params.{member}")),
            "{name}: {}",
            shader.text()
        );
    }
}

/// A tick pass that an action reruns is refused at the file's lines, since a press runs its reruns on the host.
#[test]
fn a_problem_of_the_compiler_is_placed_in_the_file() {
    let starter = crate::starters::starter("game_of_life.toml").expect("the Life starter");
    let text = starter.text.replace(
        "id = \"clear\"\nlabel = \"Clear\"\nruns = []",
        "id = \"clear\"\nlabel = \"Clear\"\nruns = [\"step\"]",
    );
    assert_ne!(text, starter.text, "the edit is in the file");
    let file = GraphFile::parse(&text).expect("the edit reads");
    let problems = GridProgram::compile(&file).expect_err("a rerun of a tick pass compiled");
    let problem = problems
        .iter()
        .find(|problem| problem.message.contains("cannot rerun the tick pass 'step'"))
        .unwrap_or_else(|| panic!("{problems:#?}"));
    let line = text
        .lines()
        .position(|line| line == "id = \"step\"")
        .expect("the step pass")
        + 1;
    // The pass's table opens on the line above its id.
    assert_eq!(
        problem.line,
        Some(u32::try_from(line - 1).expect("a short file")),
        "{problem:#?}"
    );
    assert_eq!(problem.pass.as_deref(), Some("step"));
}

/// Returns a grid file `id` of 64 by 64 cells holding `declarations` and `passes`.
fn flood_file(id: &str, declarations: &str, passes: &str) -> GraphFile {
    let text = format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"{id}\"\n\n\
         [graph]\ntopology = \"grid\"\n\
         library = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[grid]\nwidth = 64\nheight = 64\nboundary = \"torus\"\n\
         {declarations}{passes}"
    );
    GraphFile::parse(&text).unwrap_or_else(|error| panic!("{id} reads: {error}"))
}

/// Returns the shortfalls of `file` on a baseline device at its default size, after it compiles.
fn baseline_shortfalls(file: &GraphFile) -> Vec<String> {
    let program = GridProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let limits = wgpu::Limits::default();
    let params = [ParamValue::U32(64), ParamValue::U32(64)];
    program.spec().demand(&params, &limits).shortfalls(&limits)
}

/// Returns a file whose stats pass counts a category of 256 variants with `counts` nodes, 1 KiB of tallies each.
fn tally_flood(counts: usize) -> GraphFile {
    let declarations = format!(
        "{}\n[[lane]]\nid = \"c\"\ndomain = \"cells\"\ntype = \"cat:big\"\n\
         storage = \"u8\"\ninitial = {{ cat = \"big.V0\" }}\n\n[[stat]]\nlabel = \"S0\"\nfrom = \"n2.V0\"\n\
         color = [1, 1, 1, 255]\n",
        big_category()
    );
    let mut passes = String::from("\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n");
    passes.push_str(&node(9000, "lane.get", "config.lane = \"c\"\n"));
    passes.push_str(&node(9001, "lane.set", "config.lane = \"c\"\nin.value = \"n9000\"\n"));
    passes.push_str(&tally_pass("cells", counts));
    flood_file("node_tally_flood", &declarations, &passes)
}

/// The reduce shader's workgroup memory, four bytes a tally, is refused past the baseline's 16 KiB before a build,
/// with a line naming the reduce pass. Past the device's limit the shader would count wrong.
#[test]
fn tallies_past_the_workgroup_memory_are_refused() {
    assert!(
        baseline_shortfalls(&tally_flood(16)).is_empty(),
        "16 KiB of tallies fit"
    );
    let found = baseline_shortfalls(&tally_flood(17));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("'node_tally_flood_reduce'") && found[0].contains("17408 bytes of workgroup memory"),
        "{}",
        found[0]
    );
}

/// Returns a file whose step pass sums `powers` integer powers of 64 entries' tables each.
fn powi_flood(powers: usize) -> GraphFile {
    let (declarations, passes) = powi_flood_tables("cells", powers);
    flood_file("node_powi_flood", &declarations, &passes)
}

/// A step pass's uniform block, here the tables of its integer powers, is refused past the baseline's 64 KiB before a
/// build, with a line naming the pass. wgpu would otherwise refuse the bind group with no place in the file.
#[test]
fn a_uniform_block_past_the_binding_limit_is_refused() {
    assert!(baseline_shortfalls(&powi_flood(200)).is_empty(), "200 tables fit");
    let found = baseline_shortfalls(&powi_flood(300));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("'node_powi_flood_step'") && found[0].contains("uniform block of"),
        "{}",
        found[0]
    );
}

/// A pass whose nodes expand past the composer's limit on a shader is a problem placed on the pass, and no fault of
/// the compiler. Each grid count writes one test per neighbour.
#[test]
fn a_shader_past_the_size_limit_is_placed_on_its_pass() {
    let declarations = "\n[[lane]]\nid = \"c\"\ndomain = \"cells\"\ntype = \"u32\"\nstorage = \"u32\"\n";
    let mut passes = String::from("\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n");
    let counts = 700;
    for k in 0..counts {
        passes.push_str(&node(
            k + 1,
            "grid.count",
            &format!(
                "config.lane = \"c\"\nconfig.neighborhood = \"moore\"\nconfig.include_center = false\n\
                 in.value = {{ u32 = {k} }}\n"
            ),
        ));
    }
    let mut sum = 1;
    for k in 1..counts {
        let id = counts + k;
        passes.push_str(&node(
            id,
            "math.add",
            &format!("in.a = \"n{sum}\"\nin.b = \"n{}\"\n", k + 1),
        ));
        sum = id;
    }
    passes.push_str(&node(
        2 * counts,
        "lane.set",
        &format!("config.lane = \"c\"\nin.value = \"n{sum}\"\n"),
    ));
    let file = flood_file("node_count_flood", declarations, &passes);
    assert!(!file.check().has_errors(), "the file checks clean");
    let problems = GridProgram::compile(&file).expect_err("a shader past the limit compiled");
    assert_eq!(problems.len(), 1, "{problems:#?}");
    let problem = &problems[0];
    assert!(
        problem.message.contains("bytes of WGSL in the shader 'step'"),
        "{problem:#?}"
    );
    assert!(
        problem.notes.iter().all(|note| !note.contains("compiler fault")),
        "{problem:#?}"
    );
    assert_eq!(problem.pass.as_deref(), Some("step"));
    assert!(problem.line.is_some(), "the problem is placed: {problem:#?}");
}
