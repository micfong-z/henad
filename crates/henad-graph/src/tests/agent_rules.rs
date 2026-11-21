//! The checker's rules of agent models, fields, functions and expressions, and the normalisation behind them.

use crate::GraphFile;
use crate::host::{HostEvaluator, HostWorld, RunContext};
use crate::normal::compose_key;

/// Header of an agents model with a position lane `pos`, a lane `v` of `f32`, a dynamic field `f`, a static field
/// `s`, a counter `c` and a parameter `r`.
const AGENTS: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_checked"

[graph]
topology = "agents"
library = 1

[[target]]
backend = "gpu"

[agents]
count = 8
width = 20.0
height = 10.0

[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[lane]]
id = "v"
domain = "agents"
type = "f32"

[[field]]
id = "f"
type = "f32"
layers = ["a", "b"]
static = false
combine = "max"

[[field]]
id = "s"
type = "f32"
layers = ["value"]
static = true

[[counter]]
id = "c"

[[param]]
id = "r"
label = "R"
kind = "f32"
min = 1.0
max = 10.0
default = 2.0
"#;

/// Returns the messages of the errors of `tables` after the agents header.
fn errors(tables: &str) -> Vec<String> {
    let file = GraphFile::parse(&format!("{AGENTS}\n{tables}")).unwrap_or_else(|error| panic!("{error}"));
    file.check()
        .errors()
        .map(|diagnostic| format!("{} {:?} {:?}", diagnostic.message, diagnostic.node, diagnostic.notes))
        .collect()
}

/// Asserts that one error of `tables` holds `needle`.
fn refuses(tables: &str, needle: &str) {
    let found = errors(tables);
    assert!(
        found.iter().any(|message| message.contains(needle)),
        "no error holds '{needle}':\n{}",
        found.join("\n")
    );
}

/// Asserts that `tables` hold no error.
fn accepts(tables: &str) {
    let found = errors(tables);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// A tick pass over agents named `$id` holding `$nodes`.
macro_rules! tick {
    ($id:literal, $nodes:literal) => {
        concat!(
            "\n[[pass]]\nid = \"",
            $id,
            "\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n",
            $nodes
        )
    };
}

/// A radius fold `$id` around `pos` read by `$read`, of radius 2, with `cell_size` `$cell`, that counts the agents it
/// visits into the lane `v` through `$write`.
macro_rules! fold {
    ($read:literal, $id:literal, $put:literal, $write:literal, $cell:literal) => {
        concat!(
            "\n[[pass.node]]\nid = \"",
            $read,
            "\"\nop = \"lane.get\"\nconfig.lane = \"pos\"\n\n[[pass.node]]\nid = \"",
            $id,
            "\"\nop = \"agent.radius_fold\"\nconfig.acc = [{ name = \"seen\", type = \"f32\", combine = \"sum\" }]\nin.center = \"",
            $read,
            "\"\nin.radius = { f32 = 2.0 }\nin.cell_size = ",
            $cell,
            "\n\n[[pass.node]]\nid = \"",
            $put,
            "\"\nop = \"region.put\"\nconfig.acc = \"seen\"\nin.value = { f32 = 1.0 }\nregion = \"",
            $id,
            "\"\n\n[[pass.node]]\nid = \"",
            $write,
            "\"\nop = \"lane.set\"\nconfig.lane = \"v\"\nin.value = \"",
            $id,
            ".seen\"\n"
        )
    };
}

#[test]
fn a_radius_fold_after_the_pass_that_writes_positions_is_refused() {
    let move_first = tick!(
        "move",
        r#"
[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "pos"
in.value = "n1"
"#
    );
    let look = tick!("look", "");
    let fold = fold!("n10", "n11", "n12", "n13", "{ f32 = 1.0 }");
    refuses(
        &format!("{move_first}{look}{fold}"),
        "cannot read positions after the pass 'move'",
    );
    accepts(&format!("{look}{fold}"));
}

#[test]
fn every_radius_fold_takes_its_cell_size_from_one_source() {
    let first = format!(
        "{}{}",
        tick!("one", ""),
        fold!("n10", "n11", "n12", "n13", "{ param = \"r\" }")
    );
    let same = format!(
        "{}{}",
        tick!("two", ""),
        fold!("n20", "n21", "n22", "n23", "{ param = \"r\" }")
    );
    let other = format!(
        "{}{}",
        tick!("two", ""),
        fold!("n20", "n21", "n22", "n23", "{ f32 = 3.0 }")
    );
    // The two passes write `v`, which no fold reads at another agent, so `v` is plain.
    accepts(&format!("{first}{same}"));
    refuses(
        &format!("{first}{other}"),
        "must take its `cell_size` from the same source",
    );
}

#[test]
fn a_radius_fold_reading_the_positions_its_pass_writes_makes_them_dual() {
    let text = format!(
        "{AGENTS}{}{}\n[[pass.node]]\nid = \"n14\"\nop = \"lane.set\"\nconfig.lane = \"pos\"\nin.value = \"n10\"\n",
        tick!("flock", ""),
        fold!("n10", "n11", "n12", "n13", "{ f32 = 1.0 }")
    );
    let report = GraphFile::parse(&text).expect("the file reads").check();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    assert_eq!(report.dual_lanes, ["pos"]);
}

#[test]
fn a_radius_fold_holds_no_region_and_sits_in_none() {
    let inside_repeat = tick!(
        "flock",
        r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 2
config.acc = [{ name = "n", type = "f32", combine = "sum" }]
in.count = { u32 = 2 }

[[pass.node]]
id = "n2"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n3"
op = "agent.radius_fold"
config.acc = [{ name = "seen", type = "f32", combine = "sum" }]
in.center = "n2"
in.radius = { f32 = 2.0 }
in.cell_size = { f32 = 1.0 }
region = "n1"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "seen"
in.value = { f32 = 1.0 }
region = "n3"

[[pass.node]]
id = "n5"
op = "region.put"
config.acc = "n"
in.value = "n3.seen"
region = "n1"

[[pass.node]]
id = "n6"
op = "lane.set"
config.lane = "v"
in.value = "n1.n"
"#
    );
    refuses(inside_repeat, "a radius fold visits as many agents as it meets");
}

#[test]
fn a_stencil_fold_nests_in_a_repeat_and_holds_one() {
    let nested = tick!(
        "spread",
        r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 3
config.acc = [{ name = "n", type = "f32", combine = "sum" }]
in.count = { u32 = 3 }

[[pass.node]]
id = "n2"
op = "grid.fold"
config.acc = [{ name = "m", type = "f32", combine = "max" }]
in.at = { vec2i = [1, 1] }
region = "n1"

[[pass.node]]
id = "n3"
op = "region.repeat"
config.max = 2
config.acc = [{ name = "steps", type = "u32", combine = "count" }]
in.count = "n2.k"
region = "n2"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "steps"
region = "n3"

[[pass.node]]
id = "n5"
op = "conv.to_f32"
in.a = "n3.steps"
region = "n2"

[[pass.node]]
id = "n6"
op = "region.put"
config.acc = "m"
in.value = "n5"
region = "n2"

[[pass.node]]
id = "n7"
op = "region.put"
config.acc = "n"
in.value = "n2.m"
region = "n1"

[[pass.node]]
id = "n8"
op = "lane.set"
config.lane = "v"
in.value = "n1.n"
"#
    );
    accepts(nested);
    // The stencil of a fold over agents is around `at`, which the file writes, and positions past the field grid's
    // torus wrap. The innermost Repeat counts min(k, 2) for each of the eight positions, and their largest is 2.
    let file = GraphFile::parse(&format!("{AGENTS}{nested}")).expect("the file reads");
    let evaluator = HostEvaluator::new(&file).expect("the file runs");
    let world = HostWorld::Agents {
        count: 2,
        width: 4.0,
        height: 4.0,
    };
    let mut lanes = evaluator.init(world, &[], Some(1)).expect("the init runs");
    evaluator
        .run_pass("spread", &mut lanes, &[], &RunContext::tick(Some(1), 0), &mut 1)
        .expect("the pass runs");
    let v = lanes.column("v").expect("the lane v");
    assert_eq!(v.get(0), Some(crate::host::HostValue::F32(6.0)));
}

#[test]
fn a_stencil_fold_over_agents_needs_its_center() {
    refuses(
        tick!(
            "spread",
            r#"
[[pass.node]]
id = "n2"
op = "grid.fold"
config.acc = [{ name = "m", type = "f32", combine = "max" }]

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "m"
in.value = { f32 = 1.0 }
region = "n2"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "v"
in.value = "n2.m"
"#
        ),
        "needs the input `at`",
    );
}

#[test]
fn another_element_is_read_only_through_a_fold() {
    refuses(
        tick!(
            "look",
            r#"
[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n2"
op = "agent.radius_fold"
config.acc = [{ name = "seen", type = "f32", combine = "sum" }]
in.center = "n1"
in.radius = { f32 = 2.0 }
in.cell_size = { f32 = 1.0 }

[[pass.node]]
id = "n3"
op = "lane.get_at"
config.lane = "v"
in.at = "n2.other"

[[pass.node]]
id = "n4"
op = "region.put"
config.acc = "seen"
in.value = { f32 = 1.0 }
region = "n2"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "v"
in.value = "n2.seen"
"#
        ),
        "cannot read n2.other outside the body of n2",
    );
}

#[test]
fn fields_take_deposits_and_writes_where_they_allow_them() {
    let deposit_into_static = tick!(
        "lay",
        r#"
[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n2"
op = "space.cell_of"
in.p = "n1"

[[pass.node]]
id = "n3"
op = "field.deposit"
config.field = "s"
in.layer = { layer = "s.value" }
in.at = "n2.cell"
in.value = { f32 = 1.0 }
"#
    );
    refuses(deposit_into_static, "cannot deposit into the static field 's'");
    let action = r#"
[[action]]
id = "clear"
label = "Clear"

[[pass]]
id = "wipe"
stage = "action"
action = "clear"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "field.set"
config.field = "s"
in.value = { f32 = 0.0 }

[[pass.node]]
id = "n2"
op = "field.set"
config.field = "f"
in.layer = { layer = "f.b" }
in.value = { f32 = 0.0 }

[[pass.node]]
id = "n3"
op = "field.set"
config.field = "f"
in.value = { f32 = 0.0 }
"#;
    refuses(action, "cannot write the static field 's': only an init pass can");
    let twice = r#"
[[action]]
id = "clear"
label = "Clear"

[[pass]]
id = "wipe"
stage = "action"
action = "clear"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "field.set"
config.field = "f"
in.layer = { layer = "f.a" }
in.value = { f32 = 0.0 }

[[pass.node]]
id = "n2"
op = "field.set"
config.field = "f"
in.value = { f32 = 1.0 }
"#;
    refuses(
        twice,
        "cannot write a layer of the field 'f' that n1 might already write",
    );
}

/// A function `noise` of one draw, called twice from a tick pass whose calls have keys `$first` and `$second`.
macro_rules! two_calls {
    ($first:literal, $second:literal) => {
        concat!(
            r#"
[[function]]
id = "noise"
kind = "pure"

[[function.output]]
port = "bits"
type = "u32"

[[function.node]]
id = "n101"
op = "rand.bits"
config.key = 5

[[function.node]]
id = "n102"
op = "fn.output"
config.port = "bits"
in.value = "n101"

[[lane]]
id = "a"
domain = "agents"
type = "u32"

[[lane]]
id = "b"
domain = "agents"
type = "u32"
"#,
            tick!(
                "draw",
                r#"
[[pass.node]]
id = "n1"
op = "fn.call"
config.function = "noise"
"#
            ),
            "config.key = ",
            $first,
            r#"

[[pass.node]]
id = "n2"
op = "fn.call"
config.function = "noise"
config.key = "#,
            $second,
            r#"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "a"
in.value = "n1"

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "b"
in.value = "n2"
"#
        )
    };
}

#[test]
fn two_calls_of_one_function_draw_apart_through_composed_keys() {
    accepts(two_calls!("11", "12"));
    let file = GraphFile::parse(&format!("{AGENTS}{}", two_calls!("11", "12"))).expect("the file reads");
    let evaluator = HostEvaluator::new(&file).expect("the file runs");
    let keys: Vec<i64> = evaluator.document().passes[0]
        .body
        .nodes
        .iter()
        .filter(|node| node.op == "rand.bits")
        .filter_map(|node| node.config.get("key").and_then(crate::value::ConfigValue::as_integer))
        .collect();
    assert_eq!(keys, [i64::from(compose_key(11, 5)), i64::from(compose_key(12, 5))]);
    let world = HostWorld::Agents {
        count: 4,
        width: 10.0,
        height: 10.0,
    };
    let mut lanes = evaluator.init(world, &[], Some(3)).expect("the init runs");
    evaluator
        .run_pass("draw", &mut lanes, &[], &RunContext::tick(Some(3), 2), &mut 1)
        .expect("the pass runs");
    let (a, b) = (lanes.column("a").expect("a"), lanes.column("b").expect("b"));
    assert_ne!(a.words(), b.words(), "two calls draw apart");
    // A call key that composes to a key another draw of the file holds is refused.
    let colliding = format!(
        "{}\n[[pass.node]]\nid = \"n5\"\nop = \"rand.bits\"\nconfig.key = {}\n\n[[pass.node]]\nid = \"n6\"\nop = \"lane.set\"\nconfig.lane = \"v\"\nin.value = \"n7\"\n\n[[pass.node]]\nid = \"n7\"\nop = \"conv.to_f32\"\nin.a = \"n5\"\n",
        two_calls!("11", "12"),
        compose_key(11, 5)
    );
    refuses(&colliding, "inside a call, the same key as another draw");
}

#[test]
fn a_function_is_pure_and_its_context_reaches_its_calls() {
    let sink = r#"
[[function]]
id = "bad"
kind = "pure"

[[function.input]]
port = "x"
type = "f32"

[[function.node]]
id = "n101"
op = "fn.input"
config.port = "x"

[[function.node]]
id = "n102"
op = "lane.set"
config.lane = "v"
in.value = "n101"
"#;
    refuses(sink, "is a lane write, a side effect the function 'bad' cannot have");
    // A function holding a draw, called from a view pass, is refused at the call, with a note into the body.
    let view = format!(
        "{}\n[[palette]]\nid = \"one\"\ncolors = [[0, 0, 0]]\n\n[[pass]]\nid = \"look\"\nstage = \"view\"\nkind = \"map\"\ndomain = \"agents\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"fn.call\"\nconfig.function = \"noise\"\nconfig.key = 9\n\n[[pass.node]]\nid = \"n2\"\nop = \"view.color\"\nconfig.palette = \"one\"\nin.index = \"n1\"\n",
        &two_calls!("11", "12")[..two_calls!("11", "12").find("\n[[lane]]").expect("the lanes")]
    );
    let found = errors(&view);
    assert!(
        found.iter().any(|message| message
            .contains("Random bits n101 of the function 'noise' does not belong in the view pass")
            && message.contains("Some(\"n1\")")),
        "{}",
        found.join("\n")
    );
    let unset = r#"
[[function]]
id = "half"
kind = "pure"

[[function.output]]
port = "y"
type = "f32"
"#;
    refuses(unset, "has no `fn.output` for its output 'y'");
}

#[test]
fn a_function_input_is_typed_at_its_call() {
    let file = r#"
[[function]]
id = "grow"
kind = "pure"

[[function.input]]
port = "x"
type = "f32"

[[function.output]]
port = "y"
type = "f32"

[[function.node]]
id = "n101"
op = "fn.input"
config.port = "x"

[[function.node]]
id = "n102"
op = "math.midpoint"
in.a = "n101"
in.b = { f32 = 4.0 }

[[function.node]]
id = "n103"
op = "fn.output"
config.port = "y"
in.value = "n102"
"#;
    // A parameter reaches a socket that takes a constant or a parameter, and a lane value does not.
    let by_param = format!(
        "{file}{}",
        tick!(
            "grow",
            r#"
[[pass.node]]
id = "n1"
op = "fn.call"
config.function = "grow"
config.key = 3
in.x = { param = "r" }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "v"
in.value = "n1"
"#
        )
    );
    accepts(&by_param);
    let by_lane = format!(
        "{file}{}",
        tick!(
            "grow",
            r#"
[[pass.node]]
id = "n1"
op = "fn.call"
config.function = "grow"
config.key = 3
in.x = "n3"

[[pass.node]]
id = "n3"
op = "lane.get"
config.lane = "v"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "v"
in.value = "n1"
"#
        )
    );
    refuses(
        &by_lane,
        "Midpoint n102 of the function 'grow': the input `a` takes a constant or a parameter",
    );
}

#[test]
fn an_expression_that_does_not_expand_is_placed_at_its_text() {
    let broken = tick!(
        "math",
        r#"
[[pass.node]]
id = "n1"
op = "math.expr"
config.text = "out = v * (2.0"
config.inputs = [{ name = "v", type = "f32" }]
config.outputs = ["out"]
in.v = { f32 = 1.0 }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "v"
in.value = "n1.out"
"#
    );
    refuses(broken, "Expression n1: the expression needs Close");
    let mixed = tick!(
        "math",
        r#"
[[pass.node]]
id = "n1"
op = "math.expr"
config.text = "out = v + 1u"
config.inputs = [{ name = "v", type = "vec2i" }]
config.outputs = ["out"]
in.v = { vec2i = [1, 2] }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "v"
in.value = "n1.out"
"#
    );
    assert!(!errors(mixed).is_empty());
}

#[test]
fn a_radius_fold_past_its_candidate_cap_faults_with_its_place() {
    let text = format!(
        "{AGENTS}{}{}",
        tick!("flock", ""),
        fold!("n10", "n11", "n12", "n13", "{ f32 = 1.0 }")
    );
    let evaluator = HostEvaluator::new(&GraphFile::parse(&text).expect("the file reads")).expect("the file runs");
    // Every agent starts at the origin, so the walk of the first meets them all in one cell.
    let crowd = crate::limits::MAX_FOLD_CANDIDATES + 1;
    let world = HostWorld::Agents {
        count: crowd,
        width: 20.0,
        height: 10.0,
    };
    let mut lanes = evaluator.init(world, &[], None).expect("the init runs");
    let error = evaluator
        .run_pass("flock", &mut lanes, &[], &RunContext::tick(None, 0), &mut 1)
        .expect_err("the walk passes its cap");
    let message = error.to_string();
    assert!(message.contains("visited more than 65536 agents"), "{message}");
    assert!(message.contains("node n11") && message.contains("'flock'"), "{message}");
    // Three agents at one point each see the two others.
    let world = HostWorld::Agents {
        count: 3,
        width: 20.0,
        height: 10.0,
    };
    let mut small = evaluator.init(world, &[], None).expect("the init runs");
    evaluator
        .run_pass("flock", &mut small, &[], &RunContext::tick(None, 0), &mut 1)
        .expect("a small walk runs");
    assert_eq!(
        small.column("v").and_then(|column| column.get(0)),
        Some(crate::host::HostValue::F32(2.0))
    );
}

/// A stats pass over agents named `tally`, whose `n1` reads the counter `c` and `n2` its total as a `u32`.
const COUNTER_TALLY: &str = r#"
[[pass]]
id = "tally"
stage = "stats"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "stat.counter"
config.counter = "c"

[[pass.node]]
id = "n2"
op = "conv.to_u32"
in.a = "n1"

[[pass.node]]
id = "n3"
op = "lane.get"
config.lane = "v"

[[pass.node]]
id = "n4"
op = "cmp.gt"
in.a = "n3"
in.b = { f32 = 0.0 }
"#;

/// A counter total is ready only after the elements, as a reducer's result is, so a node per element cannot read it
/// and a value computed from it alone runs once after the elements.
#[test]
fn a_counter_total_is_read_after_the_elements_alone() {
    let per_element = format!(
        "{COUNTER_TALLY}\n[[pass.node]]\nid = \"n5\"\nop = \"cmp.gt\"\nin.a = \"n2\"\nin.b = {{ u32 = 0 }}\n\n\
         [[pass.node]]\nid = \"n6\"\nop = \"logic.and\"\nin.a = \"n4\"\nin.b = \"n5\"\n\n[[pass.node]]\nid = \
         \"n7\"\nop = \"stat.count_where\"\nin.pred = \"n6\"\n"
    );
    refuses(&per_element, "the input `b` reads a counter total of this pass");

    let after = format!(
        "\n[[stat]]\nlabel = \"Twice\"\nfrom = \"n6\"\ncolor = [1, 2, 3, 255]\n{COUNTER_TALLY}\n[[pass.node]]\nid = \
         \"n5\"\nop = \"stat.count_where\"\nin.pred = \"n4\"\n\n[[pass.node]]\nid = \"n6\"\nop = \"math.mul\"\n\
         in.a = \"n2\"\nin.b = {{ u32 = 2 }}\n"
    );
    accepts(&after);
    let text = format!("{AGENTS}\n{after}");
    let evaluator = HostEvaluator::new(&GraphFile::parse(&text).expect("the file reads")).expect("the file runs");
    let world = HostWorld::Agents {
        count: 3,
        width: 20.0,
        height: 10.0,
    };
    let mut lanes = evaluator.init(world, &[], None).expect("the init runs");
    assert!(lanes.set_counter("c", 21));
    let outcome = evaluator
        .run_pass("tally", &mut lanes, &[], &RunContext::tick(None, 0), &mut 1)
        .expect("the pass runs");
    let n6 = crate::ids::NodeId::new(6).and_then(|id| outcome.values.get(&id)?.get("out"));
    assert_eq!(n6, Some(&crate::host::HostValue::U32(42)));
}

/// A flat chain of operators is as deep a tree as the same chain written in parentheses, so a long chain is refused
/// with a message, and neither the check nor the drop of its tree runs out of a small stack.
#[test]
fn a_long_flat_expression_is_refused_within_a_small_stack() {
    let refusal = |text: String| {
        let tables = format!(
            "{}\n[[pass.node]]\nid = \"n2\"\nop = \"math.expr\"\nconfig.text = \"{text}\"\n\
             config.inputs = [{{ name = \"a\", type = \"f32\" }}]\nconfig.outputs = [\"o\"]\nin.a = \"n1\"\n\n\
             [[pass.node]]\nid = \"n3\"\nop = \"lane.set\"\nconfig.lane = \"v\"\nin.value = \"n2.o\"\n",
            tick!(
                "long",
                "\n[[pass.node]]\nid = \"n1\"\nop = \"lane.get\"\nconfig.lane = \"v\"\n"
            )
        );
        // A thread of 256 KiB, an eighth of a test thread's, would overflow on a recursion as deep as the chain.
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || errors(&tables))
            .expect("a thread")
            .join()
            .expect("the check returns")
    };
    for chain in [" + a", " * a", " - a", " / a"] {
        let found = refusal(format!("o = a{}", chain.repeat(20_000)));
        assert!(
            found
                .iter()
                .any(|message| message.contains("nests deeper than 64 levels")),
            "{chain}: {found:?}"
        );
    }
    let found = refusal(format!("o = a < a{}", " and a < a".repeat(20_000)));
    assert!(
        found
            .iter()
            .any(|message| message.contains("nests deeper than 64 levels")),
        "{found:?}"
    );
    let found = refusal(format!("o = vec(a, a){}", ".x".repeat(20_000)));
    assert!(
        found
            .iter()
            .any(|message| message.contains("nests deeper than 64 levels")),
        "{found:?}"
    );
    // A chain of 40 operands expands.
    accepts(&format!(
        "{}\n[[pass.node]]\nid = \"n2\"\nop = \"math.expr\"\nconfig.text = \"o = a{}\"\n\
         config.inputs = [{{ name = \"a\", type = \"f32\" }}]\nconfig.outputs = [\"o\"]\nin.a = \"n1\"\n\n\
         [[pass.node]]\nid = \"n3\"\nop = \"lane.set\"\nconfig.lane = \"v\"\nin.value = \"n2.o\"\n",
        tick!(
            "long",
            "\n[[pass.node]]\nid = \"n1\"\nop = \"lane.get\"\nconfig.lane = \"v\"\n"
        ),
        " + a".repeat(39)
    ));
}

/// A radius fold whose walk would cover more than 65536 cells of the index faults on the host before it starts, as on
/// the device, and a narrower walk over the same index runs.
#[test]
fn a_radius_fold_past_its_cell_cap_faults_before_it_walks() {
    let text = |radius: &str| {
        format!(
            "{AGENTS}{}{}",
            tick!("flock", ""),
            fold!("n10", "n11", "n12", "n13", "{ f32 = 0.05 }").replace("in.radius = { f32 = 2.0 }", radius)
        )
    };
    let world = HostWorld::Agents {
        count: 3,
        width: 20.0,
        height: 10.0,
    };
    // Cells 0.05 wide make an index of 400 by 200 cells, and a radius of 20 covers all 80000 of them.
    let evaluator = HostEvaluator::new(&GraphFile::parse(&text("in.radius = { f32 = 20.0 }")).expect("the file reads"))
        .expect("the file runs");
    let mut lanes = evaluator.init(world, &[], None).expect("the init runs");
    let error = evaluator
        .run_pass("flock", &mut lanes, &[], &RunContext::tick(None, 0), &mut 1)
        .expect_err("the walk passes its cell cap");
    let message = error.to_string();
    assert!(message.contains("cannot walk more than 65536 cells"), "{message}");
    assert!(message.contains("node n11") && message.contains("'flock'"), "{message}");
    // A radius of 2 covers 81 by 81 cells.
    let evaluator = HostEvaluator::new(&GraphFile::parse(&text("in.radius = { f32 = 2.0 }")).expect("the file reads"))
        .expect("the file runs");
    let mut lanes = evaluator.init(world, &[], None).expect("the init runs");
    evaluator
        .run_pass("flock", &mut lanes, &[], &RunContext::tick(None, 0), &mut 1)
        .expect("a narrower walk runs");
}
