//! The file format: canonical round trip, the two-spellings hash test, the editor region and the reader's refusals.

use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::{CodeOrigin, ModelCode};

use super::support::{graph_refusal, line_of, node_id};
use crate::document::{LaneStorage, PassEnabled, PassStage, Topology, World};
use crate::editor::NodeLayout;
use crate::ids::NodeId;
use crate::limits::MAX_FILE_BYTES;
use crate::node::{Combine, NodeBody};
use crate::types::{Rate, ValueType};
use crate::value::{ConfigValue, InputValue, Literal, PortRef};
use crate::{CODE_FORMAT, GraphFile, GraphFileError};

const FULL_GRAPH: &str = include_str!("full_graph.toml");

/// Smallest header of a grid graph.
const MINIMAL: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_minimal"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"
"#;

fn parse(text: &str) -> GraphFile {
    GraphFile::parse(text).expect("the text reads")
}

/// Returns [`MINIMAL`] with `body` after it.
fn minimal_with(body: &str) -> String {
    format!("{MINIMAL}\n{body}")
}

/// Returns a body of one tick pass holding one node, whose keys are `node_keys`.
fn pass_with_node(node_keys: &str) -> String {
    minimal_with(&format!(
        "[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"math.add\"\n{node_keys}\n"
    ))
}

#[test]
fn a_canonical_text_reads_back_to_itself() {
    let canonical = parse(FULL_GRAPH).to_toml();
    let again = parse(&canonical);
    assert_eq!(again.to_toml(), canonical);
    assert_eq!(again.semantic_text(), parse(FULL_GRAPH).semantic_text());
}

#[test]
fn the_canonical_form_of_a_small_file_is_exact() {
    let text = parse(&pass_with_node(
        "in.b = { f32 = 2 }\nin.a = \"n7.sum\"\nconfig.key = 12",
    ))
    .to_toml();
    let expected = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_minimal"
name = "node_minimal"
description = ""

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"
id = "node_minimal"
name = "node_minimal"
description = ""

[grid]
width = 1024
height = 1024
boundary = "torus"

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"
enabled = true

[[pass.node]]
id = "n1"
op = "math.add"
config.key = 12
in.a = "n7.sum"
in.b = { f32 = 2.0 }
"#;
    assert_eq!(text, expected);
}

#[test]
fn the_writer_writes_defaults_and_leaves_out_keys_without_one() {
    let file = parse(FULL_GRAPH);
    let text = file.to_toml();
    // `nudge` reruns nothing, and its `runs` is still written.
    assert!(
        text.contains("id = \"nudge\"\nlabel = \"Nudge\"\nruns = []\n"),
        "{text}"
    );
    assert!(text.contains("runs = [\"seed\"]"), "{text}");
    // A variant's label, a four-byte colour, a lane's storage and its initial value take their defaults.
    assert!(
        text.contains("{ name = \"I\", label = \"I\", color = [228, 55, 72, 200] },"),
        "{text}"
    );
    assert!(
        text.contains("{ name = \"S\", label = \"Susceptible\", color = [0, 122, 245, 255] },"),
        "{text}"
    );
    assert!(
        text.contains("type = \"cat:health\"\nstorage = \"u8\"\ninitial = { cat = \"health.S\" }\n"),
        "{text}"
    );
    assert!(
        text.contains("type = \"u32\"\nstorage = \"u16\"\ninitial = { u32 = 0 }\n"),
        "{text}"
    );
    assert!(text.contains("initial = { vec2f = [0.5, -1.0] }\n"), "{text}");
    // A pass's `enabled` is always written, and `draw_order` only where the pass draws from a stream.
    assert!(text.contains("id = \"seed\"\nstage = \"init\"\nkind = \"map\"\ndomain = \"cells\"\nenabled = true\ndraw_order = [\"n3\", \"n2\"]\n"), "{text}");
    assert_eq!(text.matches("draw_order").count(), 1, "{text}");
    // An optional key with no default has no written form.
    assert!(!text.contains("role"), "{text}");
    assert!(!text.contains("in.at"), "{text}");
    // Writing `format` only when it is `percent` is kept from grid rules.
    assert_eq!(text.matches("format = \"percent\"").count(), 1, "{text}");
}

#[test]
fn a_lane_keeps_its_role_and_a_field_its_keys() {
    let text = format!(
        "{}\n{}",
        MINIMAL.replace("topology = \"grid\"", "topology = \"agents\""),
        r#"[agents]
count = 50
width = 200

[[lane]]
id = "position"
domain = "agents"
type = "vec2f"
role = "position"

[[field]]
id = "pheromone"
type = "f32"
layers = ["to_food", "to_home"]
combine = "max"
finish = "fade"

[[field]]
id = "site"
type = "cat:site"
static = true
"#
    );
    let file = parse(&text);
    assert_eq!(file.document().world.topology(), Topology::Agents);
    let World::Agents(world) = file.document().world else {
        panic!("the file declares agents");
    };
    assert_eq!(
        (world.count, world.max_count, world.width, world.height),
        (50, 10_000_000, 200.0, 1000.0)
    );
    let canonical = file.to_toml();
    assert!(
        canonical.contains("[agents]\ncount = 50\nmax_count = 10000000\nwidth = 200.0\nheight = 1000.0\n"),
        "{canonical}"
    );
    assert!(canonical.contains("role = \"position\""), "{canonical}");
    assert!(canonical.contains(
        "layers = [\"to_food\", \"to_home\"]\nstatic = false\ncombine = \"max\"\nfinish = \"fade\"\ninitial = { f32 = 0.0 }\n"
    ), "{canonical}");
    // An undeclared category leaves the checker a field with no initial value, and the text stays a fixed point.
    assert!(
        canonical.contains("id = \"site\"\ntype = \"cat:site\"\nlayers = [\"value\"]\nstatic = true\n"),
        "{canonical}"
    );
    assert_eq!(parse(&canonical).to_toml(), canonical);
}

/// One graph spelled with every default left out.
const SPELLING_SHORT: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_spelled"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[[category]]
id = "life"
variants = [{ name = "Dead", color = [21, 21, 21] }, { name = "Alive", color = [0, 230, 118, 255] }]

[[lane]]
id = "state"
domain = "cells"
type = "cat:life"

[[param]]
id = "density"
kind = "f32"
min = 0
max = 1
default = 0.25

[[stat]]
label = "Alive"
from = "n21.Alive"
color = [0, 230, 118]

[[action]]
id = "clear"
label = "Clear"

[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"
draw_order = ["n2"]

[[pass.node]]
id = "n1"
op = "rng.threshold"
in.p = { param = "density" }

[[pass.node]]
id = "n2"
op = "stream.bits"
in.when = { bool = true }

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n10"
op = "region.repeat"
config.max = 8
config.acc = [{ name = "hit", type = "bool", combine = "or" }]
in.count = { u32 = 16 }
in.init.hit = { bool = false }

[[pass.node]]
id = "n11"
op = "rng.lane_seed"
region = "n10"
in.seed = { u64 = "16" }
in.scale = { f32 = 1 }

[[pass]]
id = "counts"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n21"
op = "stat.count_by"
config.category = "life"
"#;

/// The same graph with every default written, tables and nodes in another order, inline tables for `config` and `in`,
/// a comment and hexadecimal.
const SPELLING_LONG: &str = r#"# A comment the canonical text drops.
kind = "graph"
format = "henad-model"
format_version = 1
description = ""
id = "node_spelled"
name = "node_spelled"

[[pass]]
id = "counts"
stage = "stats"
kind = "map"
domain = "cells"
enabled = true

[[pass.node]]
op = "stat.count_by"
id = "n21"
config = { category = "life" }

[[pass]]
id = "step"
domain = "cells"
stage = "tick"
kind = "map"
enabled = true
draw_order = []

[[pass.node]]
id = "n11"
op = "rng.lane_seed"
in = { scale = { f32 = 1.0 }, seed = { u64 = "0x10" } }
region = "n10"

[[pass.node]]
id = "n10"
op = "region.repeat"
in = { init = { hit = { bool = false } }, count = { u32 = 16 } }
config.acc = [{ combine = "or", type = "bool", name = "hit" }]
config.max = 8

[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"
enabled = true
draw_order = ["n2"]

[[pass.node]]
id = "n2"
op = "stream.bits"
in.when.bool = true

[[pass.node]]
id = "n1"
op = "rng.threshold"
in.p.param = "density"

[[action]]
id = "clear"
label = "Clear"
runs = []

[[stat]]
label = "Alive"
from = "n21.Alive"
color = [0, 230, 118, 255]

[[param]]
id = "density"
label = "density"
kind = "f32"
min = 0.0
max = 1.0
default = 0.25
format = "plain"

[[lane]]
id = "state"
domain = "cells"
type = "cat:life"
storage = "u8"
initial = { cat = "life.Dead" }

[[category]]
id = "life"
variants = [
    { name = "Dead", label = "Dead", color = [21, 21, 21, 255] },
    { color = [0, 230, 118], label = "Alive", name = "Alive" },
]

[grid]
boundary = "torus"
height = 1024
width = 1024

[[target]]
description = ""
name = "node_spelled"
id = "node_spelled"
backend = "gpu"

[graph]
library = 1
topology = "grid"

[editor]
next_id = 22
node.n1 = { pos = [10.0, 10.0] }
"#;

#[test]
fn two_spellings_of_one_graph_hash_alike() {
    let short = parse(SPELLING_SHORT);
    let long = parse(SPELLING_LONG);
    assert_eq!(short.semantic_text(), long.semantic_text());
    assert_eq!(short.code(), long.code());
    assert_eq!(short.code().hash(), long.code().hash());
}

#[test]
fn respellings_of_the_life_starter_write_one_text() {
    let text = crate::starters::starter("game_of_life.toml")
        .expect("the starter exists")
        .text;
    let base = parse(text);
    assert!(!base.check().has_errors(), "the starter checks");
    for (old, new) in [
        ("config.survive = [2, 3]", "config.survive = [3, 2]"),
        ("config.survive = [2, 3]", "config.survive = [2, 3, 3]"),
        ("config.survive = [2, 3]", "config.survive = [3, 2, 2, 3]"),
        ("in.cell = \"n10\"", "in.cell = \"n10.value\""),
        ("in.cond = \"n3\"", "in.cond = \"n3.out\""),
    ] {
        assert!(text.contains(old), "the test edits a line the starter holds: {old}");
        let respelled = parse(&text.replacen(old, new, 1));
        let errors: Vec<_> = respelled.check().errors().cloned().collect();
        assert!(errors.is_empty(), "`{new}` checks: {errors:?}");
        assert_eq!(
            respelled.semantic_text(),
            base.semantic_text(),
            "`{new}` writes the starter's text"
        );
        assert_eq!(respelled.code().hash(), base.code().hash(), "`{new}` keeps the hash");
    }
}

#[test]
fn a_link_keeps_its_port_where_a_bare_link_would_mean_another() {
    // `n21` counts two variants, and `n3` is the region around `n4`, which reads its inner output `k`.
    let text = minimal_with(
        "[[category]]\nid = \"life\"\nvariants = [{ name = \"Dead\", color = [0, 0, 0] }, { name = \"Alive\", color = [0, 230, 118] }]\n\n\
         [[lane]]\nid = \"state\"\ndomain = \"cells\"\ntype = \"cat:life\"\n\n\
         [[stat]]\nlabel = \"Alive\"\nfrom = \"n21.Alive\"\ncolor = [0, 230, 118]\n\n\
         [[stat]]\nlabel = \"Cells\"\nfrom = \"n22.count\"\ncolor = [0, 0, 0]\n\n\
         [[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n\n\
         [[pass.node]]\nid = \"n3\"\nop = \"region.repeat\"\nconfig.max = 4\n\
         config.acc = [{ name = \"sum_k\", type = \"u32\", combine = \"sum\" }]\nin.count = { u32 = 4 }\n\n\
         [[pass.node]]\nid = \"n4\"\nop = \"region.put\"\nregion = \"n3\"\nconfig.acc = \"sum_k\"\nin.value = \"n3.k\"\n\n\
         [[pass]]\nid = \"counts\"\nstage = \"stats\"\nkind = \"map\"\ndomain = \"cells\"\n\n\
         [[pass.node]]\nid = \"n20\"\nop = \"lane.get\"\nconfig.lane = \"state\"\n\n\
         [[pass.node]]\nid = \"n21\"\nop = \"stat.count_by\"\nconfig.category = \"life\"\nin.value = \"n20\"\n\n\
         [[pass.node]]\nid = \"n22\"\nop = \"stat.count_where\"\nin.pred = { bool = true }\n",
    );
    let file = parse(&text);
    let errors: Vec<_> = file.check().errors().cloned().collect();
    assert!(errors.is_empty(), "the file checks: {errors:?}");
    let written = file.to_toml();
    assert!(written.contains("in.value = \"n3.k\"\n"), "{written}");
    assert!(written.contains("from = \"n21.Alive\"\n"), "{written}");
    assert!(written.contains("in.value = \"n20\"\n"), "{written}");
    assert!(written.contains("from = \"n22\"\n"), "{written}");
}

#[test]
fn line_ends_are_one_more_spelling() {
    let crlf = SPELLING_SHORT.replace('\n', "\r\n");
    assert_eq!(parse(&crlf).code().hash(), parse(SPELLING_SHORT).code().hash());
}

#[test]
fn a_u64_literal_has_one_written_form() {
    let text = parse(SPELLING_SHORT).to_toml();
    assert!(text.contains("in.seed = { u64 = \"0x10\" }\n"), "{text}");
    let high = parse(FULL_GRAPH).to_toml();
    assert!(high.contains("in.seed = { u64 = \"0xdeadbeefcafe1234\" }\n"), "{high}");
    let zero = parse(&pass_with_node("in.a = { u64 = \"0\" }")).to_toml();
    assert!(zero.contains("in.a = { u64 = \"0x0\" }\n"), "{zero}");
}

#[test]
fn every_semantic_change_moves_the_hash() {
    let base = parse(SPELLING_SHORT).code().hash();
    for (old, new) in [
        ("default = 0.25", "default = 0.5"),
        ("in.seed = { u64 = \"16\" }", "in.seed = { u64 = \"17\" }"),
        ("config.max = 8", "config.max = 9"),
        (
            "from = \"n21.Alive\"\ncolor = [0, 230, 118]",
            "from = \"n21.Alive\"\ncolor = [0, 230, 119]",
        ),
        (
            "id = \"clear\"\nlabel = \"Clear\"",
            "id = \"clear\"\nlabel = \"Clear all\"",
        ),
        ("region = \"n10\"", ""),
    ] {
        assert!(
            SPELLING_SHORT.contains(old),
            "the test edits a line the file holds: {old}"
        );
        let edited = SPELLING_SHORT.replace(old, new);
        assert_ne!(parse(&edited).code().hash(), base, "the edit `{old}` moves the hash");
    }
}

#[test]
fn the_code_is_the_text_before_the_editor_region() {
    let file = parse(FULL_GRAPH);
    let code = file.code();
    assert_eq!(code.format(), "henad-graph/1");
    assert_eq!(code.format(), CODE_FORMAT);
    assert!(!code.text().contains("[editor]"));
    assert!(!code.text().contains("[[editor"));
    let canonical = file.to_toml();
    assert!(canonical.starts_with(code.text()));
    assert_eq!(canonical.as_bytes()[code.text().len()], b'\n');
    assert!(canonical[code.text().len()..].starts_with("\n[editor]\n"));
}

#[test]
fn the_source_records_runtime_code() {
    let file = parse(FULL_GRAPH);
    let source = file.source();
    assert_eq!(source.origin(), CodeOrigin::Runtime);
    assert_eq!(source.code(), Some(&file.code()));
    assert_eq!(source.code_hash(), Some(file.code().hash()));
    assert!(source.build().is_none());
}

#[test]
fn a_code_reads_back_into_the_graph_that_recorded_it() {
    let file = parse(FULL_GRAPH);
    let code = file.code();
    let rebuilt = GraphFile::from_code(&code).expect("the code reads");
    assert_eq!(rebuilt.code(), code);
    assert!(rebuilt.editor().is_none());
}

#[test]
fn a_code_of_another_format_is_refused() {
    let code = ModelCode::new("henad-graph/2", MINIMAL);
    let error = GraphFile::from_code(&code).expect_err("the format is newer than the build");
    assert!(matches!(error, GraphFileError::Format(_)));
    assert_eq!(
        error.diagnostic().message,
        "Recorded in format henad-graph/2, but this build reads henad-graph/1"
    );
}

#[test]
fn a_document_writes_into_a_file() {
    let file = parse(FULL_GRAPH);
    let rebuilt = GraphFile::from_document(file.document(), file.editor()).expect("the canonical text reads");
    assert_eq!(rebuilt.to_toml(), file.to_toml());
    assert_eq!(rebuilt.text(), file.to_toml());
}

#[test]
fn moving_a_node_changes_the_text_and_leaves_the_hash() {
    let moved = FULL_GRAPH.replace("pos = [40.4, 60.6]", "pos = [900.0, 900.0]");
    assert_ne!(moved, FULL_GRAPH);
    assert_ne!(parse(&moved).to_toml(), parse(FULL_GRAPH).to_toml());
    assert_eq!(parse(&moved).code().hash(), parse(FULL_GRAPH).code().hash());
}

#[test]
fn a_file_without_an_editor_region_writes_none() {
    let file = parse(MINIMAL);
    assert!(file.editor().is_none());
    assert_eq!(file.to_toml(), file.semantic_text());
    assert_eq!(parse(&format!("{MINIMAL}\n[editor]\n")).code(), file.code());
}

#[test]
fn the_editor_region_is_canonical_and_keeps_unknown_keys() {
    let file = parse(FULL_GRAPH);
    let text = file.to_toml();
    let region = &text[file.semantic_text().len()..];
    let expected = r#"
[editor]
version = 1
next_id = 73
node.n2 = { pos = [1.0, 0.0], collapsed = true, unknown_key = "kept" }
node.n10 = { pos = [40.0, 61.0], label = "This cell" }
view."pass.step" = { pan = [0.25, 0.0], zoom = 1.0 }
function.halve.origin = { name = "Halve", hash = "0x12" }
future_key = { deep = [1, 2.5, "x"] }

[[editor.frame]]
scope = "pass.step"
title = "Read neighbours"
color = "blue"
members = ["n10", "n20"]

[[editor.note]]
scope = "pass.step"
pos = [11.0, 20.0]
text = "A note."
"#;
    assert_eq!(region, expected);
}

#[test]
fn the_editor_accessors_read_what_fits() {
    let file = parse(FULL_GRAPH);
    let editor = file.editor().expect("the file has an editor region");
    assert_eq!(editor.next_id(), Some(73));
    assert_eq!(
        editor.node_layout(node_id(10)),
        Some(NodeLayout {
            pos: [40.4, 60.6],
            collapsed: false,
            label: Some("This cell".to_owned()),
            output_pos: None,
        })
    );
    assert!(editor.node_layout(node_id(99)).is_none());
}

#[test]
fn a_strange_editor_region_stays_readable_and_a_fixed_point() {
    let strange = r#"
[editor]
next_id = "soon"
version = 1.5
node = 3
view = "none"
function = { empty = {}, plain = 2 }
frame = [1, 2]
note = []
"key with space" = 1979-05-27T07:32:00Z
pos = [nan, inf, -inf]
"#;
    let file = parse(&format!("{MINIMAL}{strange}"));
    assert_eq!(file.editor().and_then(|editor| editor.next_id()), None);
    let canonical = file.to_toml();
    assert_eq!(parse(&canonical).to_toml(), canonical);
    assert_eq!(parse(&canonical).code(), parse(MINIMAL).code());
    let odd_nodes = format!(
        "{MINIMAL}\n[editor]\nnode.n2 = 7\nnode.other = {{ pos = [\"a\", 1.4] }}\nnode.n10 = {{ pos = [nan, 1] }}\n"
    );
    let canonical = parse(&odd_nodes).to_toml();
    assert_eq!(parse(&canonical).to_toml(), canonical);
    assert!(canonical.find("node.n2").expect("written") < canonical.find("node.n10").expect("written"));
    assert!(canonical.find("node.n10").expect("written") < canonical.find("node.other").expect("written"));
}

#[test]
fn a_deep_dotted_editor_key_still_has_a_canonical_text_that_reads() {
    let deep = |name: &str, keys: usize, arrays: usize| {
        format!(
            "{} = {}1{}\n",
            vec![name; keys].join("."),
            "[".repeat(arrays),
            "]".repeat(arrays)
        )
    };
    let text = format!("{MINIMAL}\n[editor]\n{}{}", deep("a", 50, 50), deep("b", 32, 32));
    let canonical = parse(&text).to_toml();
    assert_eq!(parse(&canonical).to_toml(), canonical);
    assert!(!canonical.contains("a = "), "{canonical}");
    assert!(canonical.contains("b = "), "{canonical}");
}

#[test]
fn passes_read_in_stack_order_and_nodes_in_natural_order() {
    let file = parse(FULL_GRAPH);
    let stages: Vec<PassStage> = file.document().passes.iter().map(|pass| pass.stage).collect();
    assert_eq!(
        stages,
        [PassStage::Init, PassStage::Tick, PassStage::Stats, PassStage::Action]
    );
    let step = &file.document().passes[1];
    assert_eq!(step.enabled, PassEnabled::Param("enabled_steps".to_owned()));
    let ids: Vec<u32> = step.body.nodes.iter().map(|node| node.id.number()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
}

#[test]
fn nested_regions_read_with_their_accumulators_and_paths() {
    let file = parse(FULL_GRAPH);
    let body: &NodeBody = &file.document().passes[1].body;
    assert_eq!(body.region_path(node_id(22)), [node_id(21), node_id(20)]);
    assert_eq!(body.region_path(node_id(21)), [node_id(20)]);
    assert!(body.region_path(node_id(20)).is_empty());
    let members: Vec<NodeId> = body.region_members(node_id(21)).map(|node| node.id).collect();
    assert_eq!(members, [node_id(22), node_id(23)]);

    let repeat = body.node(node_id(20)).expect("the body holds n20");
    let accumulators = repeat.accumulators().expect("the list reads");
    assert_eq!(accumulators.len(), 2);
    assert_eq!(accumulators[0].name, "hit");
    assert_eq!(accumulators[0].combine, Combine::Or);
    assert_eq!(accumulators[1].value_type, ValueType::Vec2F);
    assert_eq!(accumulators[1].combine, Combine::Carry);
    assert_eq!(
        repeat.inputs.get("init.position"),
        Some(&InputValue::Link(PortRef {
            node: node_id(19),
            port: None,
        }))
    );
    assert_eq!(
        repeat.inputs.get("count"),
        Some(&InputValue::Param("trials".to_owned()))
    );
    assert_eq!(
        body.node(node_id(25)).expect("the body holds n25").inputs.get("value"),
        Some(&InputValue::Link(PortRef {
            node: node_id(20),
            port: Some("prev.position".to_owned()),
        }))
    );
    assert_eq!(repeat.config.get("max"), Some(&ConfigValue::Integer(16)));
}

#[test]
fn a_broken_region_chain_returns_a_finite_path() {
    let text = pass_with_node("region = \"n1\"");
    let file = parse(&text);
    assert!(file.document().passes[0].body.region_path(node_id(1)).is_empty());
    assert!(file.document().passes[0].body.region_path(node_id(5)).is_empty());
    let cycle = minimal_with(
        "[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"a\"\nregion = \"n2\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"b\"\nregion = \"n3\"\n\n[[pass.node]]\nid = \"n3\"\nop = \"c\"\nregion = \"n2\"\n",
    );
    let file = parse(&cycle);
    assert_eq!(
        file.document().passes[0].body.region_path(node_id(1)),
        [node_id(2), node_id(3)]
    );
}

#[test]
fn a_port_made_by_configuration_can_take_the_name_of_a_literal_tag() {
    // Each of these reads as a tagged value when the walk ignores the prefix.
    let file = parse(&pass_with_node(
        "in.init.param = \"n3\"\nin.init.u32 = { u32 = 7 }\nin.case.f32 = { param = \"rate\" }\nin.rule.cat.when = \"n4\"",
    ));
    let node = &file.document().passes[0].body.nodes[0];
    assert_eq!(
        node.inputs.get("init.param"),
        Some(&InputValue::Link(PortRef {
            node: node_id(3),
            port: None,
        }))
    );
    assert_eq!(node.inputs.get("init.u32"), Some(&InputValue::Literal(Literal::U32(7))));
    assert_eq!(node.inputs.get("case.f32"), Some(&InputValue::Param("rate".to_owned())));
    assert!(node.inputs.contains_key("rule.cat.when"));
    let canonical = file.to_toml();
    assert!(canonical.contains("in.init.param = \"n3\"\n"), "{canonical}");
    assert_eq!(parse(&canonical).to_toml(), canonical);
}

#[test]
fn a_function_calls_ports_stay_bare() {
    let file = parse(FULL_GRAPH);
    let call = file.document().passes[1]
        .body
        .node(node_id(28))
        .expect("the body holds n28");
    assert_eq!(call.inputs.get("init"), Some(&InputValue::Literal(Literal::F32(1.0))));
    let canonical = file.to_toml();
    assert!(canonical.contains("in.init = { f32 = 1.0 }\n"), "{canonical}");
}

#[test]
fn inline_tables_keep_the_written_key_order() {
    let text = parse(FULL_GRAPH).to_toml();
    assert!(
        text.contains("  { name = \"position\", type = \"vec2f\", combine = \"carry\" },\n"),
        "{text}"
    );
    assert!(
        text.contains("  { id = \"infect\", from = \"S\", to = \"I\", phase = 0 },\n"),
        "{text}"
    );
    assert!(text.contains("config.flags = [true, false]\n"), "{text}");
    assert!(text.contains("config.ratios = [0.5, 2.0]\n"), "{text}");
    assert!(text.contains("config.salt = \"0x8040e00294134cd9\"\n"), "{text}");
}

#[test]
fn a_choice_option_literal_keeps_a_dotted_option() {
    let text = parse(FULL_GRAPH).to_toml();
    assert!(
        text.contains("in.mode = { option = \"mode.Other.dotted\" }\n"),
        "{text}"
    );
}

#[test]
fn a_lane_defaults_its_storage_by_type() {
    let file = parse(FULL_GRAPH);
    let lanes = &file.document().lanes;
    assert_eq!(lanes[0].storage, Some(LaneStorage::U8));
    assert_eq!(lanes[1].storage, Some(LaneStorage::U16));
    assert_eq!(lanes[2].storage, None);
}

#[test]
fn storage_on_another_lane_type_is_refused_in_place() {
    let text = minimal_with("[[lane]]\nid = \"x\"\ndomain = \"cells\"\ntype = \"f32\"\nstorage = \"u8\"\n");
    let diagnostic = graph_refusal(&text);
    assert!(diagnostic.message.contains("cannot set `storage`"), "{diagnostic}");
    assert!(diagnostic.line.is_some(), "{diagnostic}");
}

#[test]
fn a_duplicate_node_id_reads_and_is_listed() {
    let text = minimal_with(
        "[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"a\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"b\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"c\"\n",
    );
    let file = parse(&text);
    let body = &file.document().passes[0].body;
    assert_eq!(body.duplicate_ids(), [node_id(1)]);
    assert_eq!(body.nodes.len(), 3);
    assert_eq!(body.node(node_id(1)).expect("the body holds n1").op, "a");
    let canonical = file.to_toml();
    assert_eq!(parse(&canonical).to_toml(), canonical);
}

#[test]
fn places_name_the_tables_and_keys_of_the_file() {
    let file = parse(FULL_GRAPH);
    let places = file.places();
    let read = |span: &std::ops::Range<usize>| &FULL_GRAPH[span.clone()];
    assert_eq!(read(&places.id), "\"node_sample\"");
    assert_eq!(places.lanes.len(), 3);
    assert_eq!(places.params.len(), 4);
    assert_eq!(read(&places.lanes[0]), "[[lane]]");
    assert_eq!(read(&places.graph), "[graph]");
    assert!(read(&places.functions[0].nodes[0].op).contains("math.mul"));

    // Passes are in stack order and their places follow.
    assert_eq!(read(&places.passes[0].table), "[[pass]]");
    assert!(FULL_GRAPH[places.passes[0].table.end..].starts_with("\nid = \"seed\""));
    let step = &places.passes[1];
    let repeat = step
        .nodes
        .iter()
        .find(|node| read(&node.id).contains("n20"))
        .expect("the pass holds n20");
    assert_eq!(read(&repeat.op), "\"region.repeat\"");
    assert_eq!(read(&repeat.config["max"]), "16");
    assert_eq!(read(&repeat.inputs["init.position"]), "\"n19\"");
    assert_eq!(read(&repeat.inputs["count"]), "{ param = \"trials\" }");
    let body_node = step
        .nodes
        .iter()
        .find(|node| read(&node.id).contains("n21"))
        .expect("the pass holds n21");
    assert_eq!(read(body_node.region.as_ref().expect("n21 has a region")), "\"n20\"");
    assert!(places.editor.is_some());

    // Each place's node sits at the same index as the document's node.
    for (node, node_places) in file.document().passes[1].body.nodes.iter().zip(&step.nodes) {
        assert_eq!(read(&node_places.id), format!("\"{}\"", node.id));
    }
}

#[test]
fn a_place_maps_back_through_line_ends() {
    let crlf = FULL_GRAPH.replace('\n', "\r\n");
    let file = parse(&crlf);
    let span = file.places().lanes[1].clone();
    let diagnostic = file.place(CodeDiagnostic::error("example"), span);
    let reported = diagnostic.span.clone().expect("the diagnostic is placed");
    assert_eq!(&crlf[reported.clone()], "[[lane]]");
    assert!(crlf[reported.end..].starts_with("\r\nid = \"age\""));
    assert_on_line(&diagnostic, FULL_GRAPH, "[[lane]]\nid = \"age\"");
}

/// Asserts that `diagnostic` sits on the line of `text` that starts `line_needle`.
fn assert_on_line(diagnostic: &CodeDiagnostic, text: &str, line_needle: &str) {
    assert_eq!(diagnostic.line, Some(line_of(text, line_needle)), "{diagnostic}");
}

#[test]
fn another_backend_or_a_second_target_is_refused() {
    let cpu = MINIMAL.replace("backend = \"gpu\"", "backend = \"cpu\"");
    let diagnostic = graph_refusal(&cpu);
    assert!(
        diagnostic.message.contains("graph models compile to WGSL"),
        "{diagnostic}"
    );
    assert_on_line(&diagnostic, &cpu, "backend = \"cpu\"");

    let two = format!("{MINIMAL}\n[[target]]\nbackend = \"gpu\"\nid = \"other\"\n");
    assert!(graph_refusal(&two).message.contains("second `[[target]]`"));

    let none = MINIMAL.replace("[[target]]\nbackend = \"gpu\"\n", "");
    assert!(graph_refusal(&none).message.contains("needs one `[[target]]`"));
}

#[test]
fn network_keys_and_sequence_passes_are_refused() {
    let network = MINIMAL.replace("topology = \"grid\"", "topology = \"network\"");
    let diagnostic = graph_refusal(&network);
    assert!(diagnostic.message.contains("network"), "{diagnostic}");
    assert_on_line(&diagnostic, &network, "topology = ");

    let sequence = pass_with_node("").replace("kind = \"map\"", "kind = \"sequence\"");
    assert!(graph_refusal(&sequence).message.contains("sequence"));

    let after = pass_with_node("after = \"start\"");
    let diagnostic = graph_refusal(&after);
    assert!(diagnostic.message.contains("after"), "{diagnostic}");

    let domain = pass_with_node("").replace("domain = \"cells\"", "domain = \"edges\"");
    assert!(graph_refusal(&domain).message.contains("edges"));

    for type_name in ["node", "edge", "list:node", "opt:node", "opt:edge"] {
        let lane = minimal_with(&format!(
            "[[lane]]\nid = \"x\"\ndomain = \"cells\"\ntype = \"{type_name}\"\n"
        ));
        assert!(
            graph_refusal(&lane).message.contains("unknown value type"),
            "{type_name}"
        );
    }
}

#[test]
fn an_apply_key_and_unknown_keys_are_refused() {
    let apply = minimal_with("[[param]]\nid = \"x\"\nkind = \"bool\"\ndefault = true\napply = \"live\"\n");
    let diagnostic = graph_refusal(&apply);
    assert!(diagnostic.message.contains("apply"), "{diagnostic}");
    assert_on_line(&diagnostic, &apply, "apply = ");

    for (text, key) in [
        (format!("{MINIMAL}\nmystery = 1\n"), "mystery"),
        (MINIMAL.replace("library = 1", "library = 1\nmystery = 1"), "mystery"),
        (pass_with_node("label = \"Node\""), "label"),
        (pass_with_node("pos = [1, 2]"), "pos"),
        (
            minimal_with("[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\n\n[[pass.var]]\nname = \"v\"\n"),
            "var",
        ),
        (minimal_with("[network]\ncount = 1\n"), "network"),
    ] {
        let diagnostic = graph_refusal(&text);
        assert!(
            matches!(GraphFile::parse(&text), Err(GraphFileError::Toml(_))),
            "{diagnostic}"
        );
        assert!(diagnostic.message.contains(key), "{diagnostic}");
    }
}

#[test]
fn the_header_is_checked_before_the_rest() {
    // Each file also holds a key this build does not know, which would be refused after the header.
    let unknown = "\nmystery = 1\n";
    for (old, new, message) in [
        (
            "format = \"henad-model\"",
            "format = \"other\"",
            "the file's format is 'other'",
        ),
        ("format_version = 1", "format_version = 2", "model file version 2"),
        ("kind = \"graph\"", "kind = \"gpu_grid\"", "kind 'gpu_grid'"),
        (
            "library = 1",
            "library = 3",
            "the file uses node library 3, expected at most 1",
        ),
        ("library = 1", "library = 0", "must be at least 1, got 0"),
    ] {
        let text = format!("{}{unknown}", MINIMAL.replace(old, new));
        let diagnostic = graph_refusal(&text);
        assert!(diagnostic.message.contains(message), "{diagnostic}");
        assert_on_line(&diagnostic, &text, new);
    }
}

#[test]
fn a_missing_header_key_is_refused() {
    for key in [
        "format = \"henad-model\"\n",
        "format_version = 1\n",
        "kind = \"graph\"\n",
        "id = \"node_minimal\"\n",
    ] {
        let text = MINIMAL.replace(key, "");
        assert!(matches!(GraphFile::parse(&text), Err(GraphFileError::Toml(_))), "{key}");
    }
}

#[test]
fn a_file_past_the_limit_is_refused_before_parsing() {
    // Text that would fail to parse, so the message proves the size was checked first.
    let text = "[".repeat(MAX_FILE_BYTES + 1);
    let diagnostic = graph_refusal(&text);
    assert!(diagnostic.message.contains("at most"), "{diagnostic}");
    assert!(diagnostic.span.is_none());
}

#[test]
fn a_file_whose_canonical_text_passes_the_limit_is_refused() {
    let nodes: Vec<String> = (1..=30000)
        .map(|number| format!("{{id=\"n{number}\",op=\"ctx.index\"}}"))
        .collect();
    let text = minimal_with(&format!(
        "[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\nnode = [{}]\n",
        nodes.join(",")
    ));
    assert!(text.len() < MAX_FILE_BYTES, "{} bytes", text.len());
    let diagnostic = graph_refusal(&text);
    assert!(diagnostic.message.contains("canonical text"), "{diagnostic}");
}

#[test]
fn deep_nesting_within_the_limit_is_an_error_and_no_crash() {
    let arrays = format!("{MINIMAL}\nmystery = {}", "[".repeat(MAX_FILE_BYTES - 200));
    assert!(GraphFile::parse(&arrays).is_err());
    let tables = format!("{MINIMAL}\nmystery = {}", "{ a = ".repeat((MAX_FILE_BYTES - 200) / 6));
    assert!(GraphFile::parse(&tables).is_err());
    let keys = format!("{MINIMAL}\n{} = 1\n", vec!["a"; (MAX_FILE_BYTES - 200) / 2].join("."));
    assert!(GraphFile::parse(&keys).is_err());
    let config = pass_with_node(&format!("config.a = {}", "[".repeat(MAX_FILE_BYTES - 400)));
    assert!(GraphFile::parse(&config).is_err());
}

#[test]
fn input_forms_are_refused_in_place() {
    for (keys, message) in [
        ("in.a = 3", "an input must be a link"),
        ("in.a = true", "an input must be a link"),
        ("in.a = [1, 2]", "an input must be a link"),
        ("in.a = { f32 = 1.0, u32 = 2 }", "an input must be a link"),
        ("in.a = {}", "an input must be a link"),
        ("in.a = { param = 3 }", "an input must be a link"),
        ("in.a = \"m3\"", "invalid node id"),
        ("in.a = \"n0\"", "invalid node id"),
        ("in.a = \"n03\"", "invalid node id"),
        ("in.a = \"n3.\"", "invalid port reference"),
        ("in.a = \"n3.b c\"", "invalid port reference"),
        ("in.a = \"n99999999999\"", "invalid node id"),
        ("in.a = { mystery = 1 }", "a literal must be a one-key table"),
        ("in.a = { u32 = -1 }", "out of range"),
        ("in.a = { u32 = 4294967296 }", "out of range"),
        ("in.a = { u32 = 1.5 }", "integer of type `u32`"),
        ("in.a = { i32 = 2147483648 }", "out of range"),
        ("in.a = { f32 = nan }", "must be finite"),
        ("in.a = { f32 = inf }", "must be finite"),
        ("in.a = { f32 = 1e300 }", "too large"),
        ("in.a = { f64 = -inf }", "must be finite"),
        ("in.a = { f32 = \"1\" }", "must be a number"),
        ("in.a = { u64 = 16 }", "must be a string"),
        ("in.a = { u64 = \"+16\" }", "invalid `u64`"),
        ("in.a = { u64 = \"0x\" }", "invalid `u64`"),
        ("in.a = { u64 = \"18446744073709551616\" }", "invalid `u64`"),
        ("in.a = { u64 = \"\" }", "invalid `u64`"),
        ("in.a = { vec2f = [1.0] }", "components, expected 2"),
        ("in.a = { vec2f = 1.0 }", "array of 2"),
        ("in.a = { vec2u = [1, -1] }", "out of range"),
        ("in.a = { bool = 1 }", "`true` or `false`"),
        ("in.a = { cat = \"health\" }", "<id>.<name>"),
        ("in.a = { cat = \".I\" }", "<id>.<name>"),
        ("in.a = { layer = 1 }", "<id>.<name>"),
        ("in.a = 1979-05-27T07:32:00Z", "datetime"),
        ("in.init = \"n3\"", "needs a name after it"),
        ("in.rule.a = \"n3\"", "needs a name after it"),
        ("in.\"a b\" = \"n3\"", "invalid node key"),
        ("in = 3", "`in` must be a table"),
        ("config = 3", "`config` must be a table"),
        ("config.a.b = 3", "a scalar or an array"),
        ("config.\"a b\" = 3", "invalid node key"),
        ("config.a = nan", "must be finite"),
        ("config.a = [{ b = inf }]", "must be finite"),
        ("config.a = [{ \"b c\" = 1 }]", "invalid node key"),
        ("region = \"x\"", "invalid node id"),
    ] {
        let text = pass_with_node(keys);
        let diagnostic = graph_refusal(&text);
        assert!(diagnostic.message.contains(message), "`{keys}`: {diagnostic}");
        assert!(diagnostic.line.is_some(), "`{keys}`: {diagnostic}");
        let line = keys.lines().next().unwrap_or_default();
        assert_on_line(&diagnostic, &text, line);
    }
}

#[test]
fn declaration_values_are_refused_in_place() {
    for (body, message) in [
        (
            "[[category]]\nid = \"c\"\nvariants = [{ name = \"a\", color = [1, 2] }]\n",
            "expected 3 or 4",
        ),
        (
            "[[palette]]\nid = \"p\"\ncolors = [[1, 2, 3, 4, 5]]\n",
            "expected 3 or 4",
        ),
        (
            "[[lane]]\nid = \"l\"\ndomain = \"cells\"\ntype = \"mystery\"\n",
            "unknown value type",
        ),
        (
            "[[lane]]\nid = \"l\"\ndomain = \"cells\"\ntype = \"cat:Bad__Id\"\n",
            "not a category id",
        ),
        (
            "[[lane]]\nid = \"l\"\ndomain = \"cells\"\ntype = \"layer:\"\n",
            "not a field id",
        ),
        (
            "[[lane]]\nid = \"l\"\ndomain = \"cells\"\ntype = \"f32\"\ninitial = 0.0\n",
            "a literal must be a one-key table",
        ),
        (
            "[[stat]]\nlabel = \"s\"\nfrom = \"x\"\ncolor = [1, 2, 3]\n",
            "invalid node id",
        ),
        (
            "[[stat]]\nlabel = \"s\"\nfrom = \"n1\"\ncolor = [1, 2]\n",
            "expected 3 or 4",
        ),
        (
            "[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\nenabled = 1\n",
            "`enabled` must be",
        ),
        (
            "[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\nenabled = { param = 1 }\n",
            "`enabled` must be",
        ),
        (
            "[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\nenabled = { other = \"x\" }\n",
            "`enabled` must be",
        ),
        (
            "[[pass]]\nid = \"p\"\nstage = \"tick\"\nkind = \"map\"\ndraw_order = [\"y\"]\n",
            "invalid node id",
        ),
        (
            "[[param]]\nid = \"p\"\nkind = \"f32\"\nmin = 0.0\nmax = nan\ndefault = 0.0\n",
            "not finite",
        ),
        ("[agents]\ncount = 1\n", "does not belong to the grid topology"),
        (
            "[[function]]\nid = \"f\"\nkind = \"pure\"\n\n[[function.input]]\nport = \"a\"\ntype = \"x\"\n",
            "unknown value type",
        ),
    ] {
        let diagnostic = graph_refusal(&minimal_with(body));
        assert!(diagnostic.message.contains(message), "`{body}`: {diagnostic}");
        assert!(diagnostic.line.is_some(), "`{body}`: {diagnostic}");
    }
    let grid_on_agents = format!(
        "{}\n[grid]\nwidth = 1\n",
        MINIMAL.replace("topology = \"grid\"", "topology = \"agents\"")
    );
    assert!(
        graph_refusal(&grid_on_agents)
            .message
            .contains("does not belong to the agents topology")
    );
    let agents_width = format!(
        "{}\n[agents]\nwidth = nan\n",
        MINIMAL.replace("topology = \"grid\"", "topology = \"agents\"")
    );
    assert!(graph_refusal(&agents_width).message.contains("must be finite"));
}

#[test]
fn floats_read_back_bit_for_bit() {
    for value in [
        0.0_f32,
        -0.0,
        1.0,
        0.1,
        6.283_185_5,
        1e20,
        1e-20,
        f32::MIN_POSITIVE,
        1e-45,
        f32::MAX,
        f32::MIN,
    ] {
        let first = pass_with_node(&format!("in.a = {{ f32 = {value:?} }}"));
        let canonical = parse(&first).to_toml();
        let again = parse(&canonical);
        let Some(InputValue::Literal(Literal::F32(read))) = again.document().passes[0].body.nodes[0].inputs.get("a")
        else {
            panic!("the node holds an `f32` literal");
        };
        assert_eq!(read.to_bits(), value.to_bits(), "{value:?}");
    }
    for value in [0.407_f64, -0.0, 1e300, 1e-300, f64::MIN_POSITIVE, f64::MAX] {
        let first = pass_with_node(&format!("in.a = {{ f64 = {value:?} }}"));
        let again = parse(&parse(&first).to_toml());
        let Some(InputValue::Literal(Literal::F64(read))) = again.document().passes[0].body.nodes[0].inputs.get("a")
        else {
            panic!("the node holds an `f64` literal");
        };
        assert_eq!(read.to_bits(), value.to_bits(), "{value:?}");
    }
}

/// The reader narrows an `f32` from the `f64` TOML parses, and the shortest text of an `f32` has to survive that.
#[test]
fn the_shortest_text_of_an_f32_survives_narrowing_from_f64() {
    let mut state = 0x9e37_79b9_u32;
    for _ in 0..200_000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let value = f32::from_bits(state);
        if value.is_finite() {
            let wide: f64 = format!("{value:?}").parse().expect("the text is a float");
            assert_eq!((wide as f32).to_bits(), value.to_bits(), "{value:?}");
        }
    }
}

#[test]
fn every_type_string_round_trips() {
    for text in [
        "bool",
        "u32",
        "i32",
        "f32",
        "vec2u",
        "vec2i",
        "vec2f",
        "cat:health",
        "layer:pheromone",
        "cell",
        "agent",
        "u64",
        "f64",
        "vec2d",
    ] {
        let parsed: ValueType = text.parse().expect("the type reads");
        assert_eq!(parsed.to_string(), text);
    }
    assert!("u64".parse::<ValueType>().expect("the type reads").is_host_only());
    assert!(!"u32".parse::<ValueType>().expect("the type reads").is_host_only());
}

#[test]
fn rates_join_to_the_faster_one() {
    assert_eq!(Rate::Constant.join(Rate::Element), Rate::Element);
    assert_eq!(Rate::Pair.join(Rate::Parameter), Rate::Pair);
    assert!(Rate::Tick.is_uniform());
    assert!(!Rate::Element.is_uniform());
}

#[test]
fn node_ids_order_naturally_and_refuse_other_spellings() {
    assert!(node_id(9) < node_id(10));
    assert_eq!("n10".parse::<NodeId>().expect("the id reads"), node_id(10));
    assert_eq!(node_id(12).to_string(), "n12");
    assert!(NodeId::new(0).is_none());
    for text in ["n", "n0", "n01", "x1", "N1", "n-1", "n+1", "n1.2", ""] {
        assert!(text.parse::<NodeId>().is_err(), "{text}");
    }
}

#[test]
fn the_id_grammars_hold() {
    use crate::ids::{is_id, is_port_segment};
    assert!(is_id("a"));
    assert!(is_id("sub_steps2"));
    assert!(!is_id("A"));
    assert!(!is_id("a__b"));
    assert!(!is_id("2a"));
    assert!(!is_id(""));
    assert!(!is_id(&"a".repeat(65)));
    assert!(is_id(&"a".repeat(64)));
    assert!(is_port_segment("Alive"));
    assert!(is_port_segment("a_b_"));
    assert!(!is_port_segment("a__b"));
    assert!(!is_port_segment("1a"));
    assert!(!is_port_segment("a.b"));
    assert!(!is_port_segment(&"a".repeat(65)));
}

#[test]
fn accumulators_refuse_a_bad_list() {
    for (config, message) in [
        ("config.acc = \"position\"", "not a list"),
        ("config.acc = [1]", "must be a `{ name, type, combine }` table"),
        ("config.acc = [{ name = \"a\", type = \"f32\" }]", "`combine`"),
        (
            "config.acc = [{ name = \"a\", type = \"x\", combine = \"or\" }]",
            "unknown value type",
        ),
        (
            "config.acc = [{ name = \"a\", type = \"f32\", combine = \"x\" }]",
            "unknown combine rule",
        ),
    ] {
        let file = parse(&pass_with_node(config));
        let error = file.document().passes[0].body.nodes[0]
            .accumulators()
            .expect_err("the list is refused");
        assert!(error.contains(message), "`{config}`: {error}");
    }
    let file = parse(&pass_with_node(""));
    assert!(
        file.document().passes[0].body.nodes[0]
            .accumulators()
            .expect("no list declares none")
            .is_empty()
    );
    for rule in [
        Combine::Sum,
        Combine::Count,
        Combine::Min,
        Combine::Max,
        Combine::Or,
        Combine::And,
        Combine::Carry,
    ] {
        assert_eq!(Combine::from_name(rule.as_str()), Ok(rule));
    }
}

#[test]
fn a_graph_file_prints_its_size_and_no_text() {
    let printed = format!("{:?}", parse(FULL_GRAPH));
    assert!(printed.contains("node_sample"), "{printed}");
    assert!(!printed.contains("region.repeat"), "{printed}");
}
