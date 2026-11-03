//! The host evaluator: inits replay, reruns draw on the action stream, and a Repeat runs as its definition says.

use henad_core::authoring::primitives::rng::random_float;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::ParamValue;

use super::corpus;
use crate::GraphFile;
use crate::document::PassStage;
use crate::draw::{PHASE_INIT, action_phase, draw_bits, key_word};
use crate::host::{HostError, HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};

/// Header of a grid of 8 by 8 with a `u32` lane `x`, an `f32` lane `y`, and the parameters `share` and `seeding`.
pub(super) const GRID: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_host"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 8
height = 8

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

[[param]]
id = "seeding"
label = "Seeding"
kind = "bool"
default = true
"#;

/// A tick pass that keeps `x`, since a grid has exactly one tick pass.
const KEEP_X: &str = r#"
[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n900"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n901"
op = "lane.set"
config.lane = "x"
in.value = "n900"
"#;

/// Returns the evaluator of [`GRID`] followed by `tables` and [`KEEP_X`].
#[track_caller]
fn evaluator(tables: &str) -> HostEvaluator {
    let text = format!("{GRID}\n{tables}\n{KEEP_X}");
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("the file reads: {error}\n{text}"));
    HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{:?}", error.diagnostics()))
}

/// Returns an init pass `seed` holding `nodes`.
fn init(nodes: &str) -> String {
    format!("[[pass]]\nid = \"seed\"\nstage = \"init\"\nkind = \"map\"\ndomain = \"cells\"\n{nodes}")
}

/// The world of [`GRID`].
pub(super) const WORLD: HostWorld = HostWorld::Grid {
    width: 8,
    height: 8,
    boundary: Boundary::Torus,
};

/// Returns every value of lane `id`.
fn values(lanes: &HostLanes, id: &str) -> Vec<HostValue> {
    let column = lanes.column(id).expect("the lane");
    (0..column.len())
        .map(|index| column.get(index).expect("an element"))
        .collect()
}

#[test]
fn every_corpus_init_runs_on_the_host_and_replays() {
    let world = HostWorld::Grid {
        width: 24,
        height: 16,
        boundary: Boundary::Torus,
    };
    for (name, text) in corpus::all() {
        let file = GraphFile::parse(&text).expect("the corpus reads");
        let evaluator = HostEvaluator::new(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let document = evaluator.document();
        let first = evaluator.init(world, &[], Some(7)).expect("the init runs");
        let again = evaluator.init(world, &[], Some(7)).expect("the init runs");
        let other = evaluator.init(world, &[], Some(8)).expect("the init runs");
        let words = |lanes: &HostLanes| -> Vec<Vec<u32>> {
            lanes.columns().iter().map(|column| column.words().to_vec()).collect()
        };
        assert_eq!(words(&first), words(&again), "{name}: one seed gives one init");
        // `repeat_trials.toml` has no init pass, and its cells start at their declared initial value at any seed.
        if document.passes.iter().any(|pass| pass.stage == PassStage::Init) {
            assert_ne!(words(&first), words(&other), "{name}: another seed gives another init");
        }
        for pass in &document.passes {
            let mut lanes = first.clone();
            let context = match pass.stage {
                PassStage::Action => RunContext::action(Some(7), 0, 1, 0),
                _ => RunContext::tick(Some(7), 0),
            };
            let outcome = evaluator
                .run_pass(&pass.id, &mut lanes, &[], &context, &mut 1)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            if pass.stage == PassStage::Stats {
                let total: u64 = outcome
                    .values
                    .values()
                    .flat_map(|outputs| outputs.values())
                    .filter_map(|value| match value {
                        HostValue::U64(count) => Some(*count),
                        _ => None,
                    })
                    .sum();
                assert_eq!(total, 24 * 16, "{name}: the counts cover every cell");
            }
        }
    }
}

#[test]
fn the_sir_init_reads_its_parameters_and_their_defaults() {
    let text = crate::starters::starter("sir.toml").expect("the SIR starter").text;
    let evaluator = HostEvaluator::new(&GraphFile::parse(text).expect("the starter reads")).expect("the starter runs");
    let world = HostWorld::Grid {
        width: 16,
        height: 16,
        boundary: Boundary::Torus,
    };
    let health = |pct: f32| {
        let params = [ParamValue::F32(0.3), ParamValue::F32(0.05), ParamValue::F32(pct)];
        let lanes = evaluator.init(world, &params, Some(3)).expect("the init runs");
        values(&lanes, "health")
    };
    assert!(health(1.0).iter().all(|value| *value == HostValue::Category(1)));
    assert!(health(0.0).iter().all(|value| *value == HostValue::Category(0)));
    let defaults = evaluator.init(world, &[], Some(3)).expect("the init runs");
    let short = evaluator
        .init(world, &[ParamValue::F32(0.3)], Some(3))
        .expect("the init runs");
    assert_eq!(values(&defaults, "health"), values(&short, "health"));
    assert_eq!(values(&defaults, "health"), health(0.01));
}

#[test]
fn an_init_pass_turned_off_by_a_parameter_is_skipped() {
    let evaluator = evaluator(&format!(
        "{}enabled = {{ param = \"seeding\" }}\n\n[[pass.node]]\nid = \"n1\"\nop = \"lane.set\"\nconfig.lane = \"x\"\nin.value = {{ u32 = 9 }}\n",
        init("")
    ));
    let on = evaluator
        .init(WORLD, &[ParamValue::F32(0.5), ParamValue::Bool(true)], None)
        .expect("runs");
    let off = evaluator
        .init(WORLD, &[ParamValue::F32(0.5), ParamValue::Bool(false)], None)
        .expect("runs");
    assert!(values(&on, "x").iter().all(|value| *value == HostValue::U32(9)));
    assert!(values(&off, "x").iter().all(|value| *value == HostValue::U32(0)));
}

#[test]
fn a_file_with_errors_is_refused() {
    let file = GraphFile::parse(&format!(
        "{GRID}\n{}",
        init("\n[[pass.node]]\nid = \"n1\"\nop = \"math.nothing\"\n")
    ))
    .expect("the file reads");
    assert!(matches!(HostEvaluator::new(&file), Err(HostError::Checked(_))));
}

#[test]
fn a_tick_pass_reads_the_start_of_the_pass() {
    let text = crate::starters::starter("game_of_life.toml")
        .expect("the Life starter")
        .text;
    let evaluator = HostEvaluator::new(&GraphFile::parse(text).expect("the starter reads")).expect("the starter runs");
    let world = HostWorld::Grid {
        width: 5,
        height: 5,
        boundary: Boundary::Torus,
    };
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("the lanes");
    let alive = |cells: &[usize]| -> Vec<HostValue> {
        (0..25)
            .map(|cell| HostValue::Category(u32::from(cells.contains(&cell))))
            .collect()
    };
    let column = lanes.column_mut("state").expect("the state lane");
    for (cell, value) in alive(&[11, 12, 13]).into_iter().enumerate() {
        assert!(column.set(cell, value), "a state");
    }
    evaluator
        .run_pass("step", &mut lanes, &[], &RunContext::tick(None, 0), &mut 1)
        .expect("the step runs");
    assert_eq!(
        values(&lanes, "state"),
        alive(&[7, 12, 17]),
        "the blinker turns upright"
    );
    evaluator
        .run_pass("step", &mut lanes, &[], &RunContext::tick(None, 1), &mut 1)
        .expect("the step runs");
    assert_eq!(values(&lanes, "state"), alive(&[11, 12, 13]), "and back");
}

/// A Repeat of five iterations over a `u32` and an `f32` carry, each started from the cell's index.
pub(super) const CARRIED: &str = r#"
[[pass.node]]
id = "n1"
op = "ctx.index"

[[pass.node]]
id = "n2"
op = "conv.to_f32"
in.a = "n1"

[[pass.node]]
id = "n3"
op = "region.repeat"
config.max = 8
config.acc = [
  { name = "word", type = "u32", combine = "carry" },
  { name = "number", type = "f32", combine = "carry" },
]
in.count = { u32 = 5 }
in.init.word = "n1"
in.init.number = "n2"

[[pass.node]]
id = "n4"
op = "bits.xor"
in.a = "n3.prev.word"
in.b = "n3.k"
region = "n3"

[[pass.node]]
id = "n5"
op = "rng.hash"
in.x = "n4"
region = "n3"

[[pass.node]]
id = "n6"
op = "math.mul"
in.a = "n3.prev.number"
in.b = { f32 = 1.1 }
region = "n3"

[[pass.node]]
id = "n7"
op = "conv.to_f32"
in.a = "n3.k"
region = "n3"

[[pass.node]]
id = "n8"
op = "math.add"
in.a = "n6"
in.b = "n7"
region = "n3"

[[pass.node]]
id = "n9"
op = "region.put"
config.acc = "word"
in.value = "n5"
region = "n3"

[[pass.node]]
id = "n10"
op = "region.put"
config.acc = "number"
in.value = "n8"
region = "n3"

[[pass.node]]
id = "n11"
op = "lane.set"
config.lane = "x"
in.value = "n3.word"

[[pass.node]]
id = "n12"
op = "lane.set"
config.lane = "y"
in.value = "n3.number"
"#;

/// Returns the body of [`CARRIED`] written out five times, iteration `k` reading the literal `k`.
pub(super) fn chained() -> String {
    let mut text = String::from(
        "\n[[pass.node]]\nid = \"n1\"\nop = \"ctx.index\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"conv.to_f32\"\nin.a = \"n1\"\n",
    );
    let (mut word, mut number) = ("n1".to_owned(), "n2".to_owned());
    for k in 0..5u32 {
        let base = 100 + k * 10;
        text.push_str(&format!(
            r#"
[[pass.node]]
id = "n{x}"
op = "bits.xor"
in.a = "{word}"
in.b = {{ u32 = {k} }}

[[pass.node]]
id = "n{h}"
op = "rng.hash"
in.x = "n{x}"

[[pass.node]]
id = "n{m}"
op = "math.mul"
in.a = "{number}"
in.b = {{ f32 = 1.1 }}

[[pass.node]]
id = "n{a}"
op = "math.add"
in.a = "n{m}"
in.b = {{ f32 = {k}.0 }}
"#,
            x = base,
            h = base + 1,
            m = base + 2,
            a = base + 3,
        ));
        word = format!("n{}", base + 1);
        number = format!("n{}", base + 3);
    }
    text.push_str(&format!(
        "\n[[pass.node]]\nid = \"n11\"\nop = \"lane.set\"\nconfig.lane = \"x\"\nin.value = \"{word}\"\n\n[[pass.node]]\nid = \"n12\"\nop = \"lane.set\"\nconfig.lane = \"y\"\nin.value = \"{number}\"\n"
    ));
    text
}

#[test]
fn a_repeat_body_equals_its_hand_chained_copies_bit_for_bit() {
    let looped = evaluator(&init(CARRIED)).init(WORLD, &[], None).expect("runs");
    let unrolled = evaluator(&init(&chained())).init(WORLD, &[], None).expect("runs");
    for lane in ["x", "y"] {
        let words = |lanes: &HostLanes| lanes.column(lane).expect("the lane").words().to_vec();
        assert_eq!(words(&looped), words(&unrolled), "lane {lane}");
    }
    assert_ne!(values(&looped, "x")[3], HostValue::U32(3), "the body ran");
}

/// A Repeat of two iterations that keeps the draw of each iteration in a carry of its own.
pub(super) const TWO_DRAWS: &str = r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 2
config.acc = [
  { name = "first", type = "f32", combine = "carry" },
  { name = "second", type = "f32", combine = "carry" },
]
in.count = { u32 = 2 }
in.init.first = { f32 = 0.0 }
in.init.second = { f32 = 0.0 }

[[pass.node]]
id = "n2"
op = "rand.float"
config.key = 4242
in.max = { f32 = 1.0 }
region = "n1"

[[pass.node]]
id = "n3"
op = "cmp.eq"
in.a = "n1.k"
in.b = { u32 = 0 }
region = "n1"

[[pass.node]]
id = "n4"
op = "logic.select"
in.cond = "n3"
in.if_true = "n2"
in.if_false = "n1.prev.first"
region = "n1"

[[pass.node]]
id = "n5"
op = "logic.select"
in.cond = "n3"
in.if_true = "n1.prev.second"
in.if_false = "n2"
region = "n1"

[[pass.node]]
id = "n6"
op = "region.put"
config.acc = "first"
in.value = "n4"
region = "n1"

[[pass.node]]
id = "n7"
op = "region.put"
config.acc = "second"
in.value = "n5"
region = "n1"

[[pass.node]]
id = "n8"
op = "lane.set"
config.lane = "y"
in.value = "n1.first"

[[pass.node]]
id = "n9"
op = "math.sub"
in.a = "n1.second"
in.b = "n1.first"

[[pass.node]]
id = "n10"
op = "cmp.ne"
in.a = "n9"
in.b = { f32 = 0.0 }

[[pass.node]]
id = "n11"
op = "conv.to_u32"
in.a = "n10"

[[pass.node]]
id = "n12"
op = "lane.set"
config.lane = "x"
in.value = "n11"
"#;

#[test]
fn two_iterations_of_one_draw_differ_and_one_seed_replays() {
    let evaluator = evaluator(&init(TWO_DRAWS));
    let first = evaluator.init(WORLD, &[], Some(11)).expect("runs");
    let again = evaluator.init(WORLD, &[], Some(11)).expect("runs");
    assert!(
        values(&first, "x").iter().all(|value| *value == HostValue::U32(1)),
        "the iterations differ"
    );
    assert_eq!(values(&first, "y"), values(&again, "y"), "one seed replays");
    let key = key_word(4242, PHASE_INIT, Some(11));
    for (element, value) in values(&first, "y").into_iter().enumerate() {
        let bits = draw_bits(key, element as u32, 0, Some(0), 0);
        assert_eq!(
            value,
            HostValue::F32(random_float(bits, 1.0)),
            "the draw of cell {element}"
        );
    }
}

/// A Repeat of two iterations around one of three, whose draw on outer iteration 1 and inner iteration 2 lands in `y`.
pub(super) const NESTED_DRAW: &str = r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 3
config.acc = [{ name = "kept", type = "f32", combine = "carry" }]
in.count = { u32 = 2 }
in.init.kept = { f32 = 0.0 }

[[pass.node]]
id = "n2"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "kept", type = "f32", combine = "carry" }]
in.count = { u32 = 3 }
in.init.kept = "n1.prev.kept"
region = "n1"

[[pass.node]]
id = "n3"
op = "rand.float"
config.key = 77
in.max = { f32 = 1.0 }
region = "n2"

[[pass.node]]
id = "n4"
op = "cmp.eq"
in.a = "n1.k"
in.b = { u32 = 1 }
region = "n2"

[[pass.node]]
id = "n5"
op = "cmp.eq"
in.a = "n2.k"
in.b = { u32 = 2 }
region = "n2"

[[pass.node]]
id = "n6"
op = "logic.and"
in.a = "n4"
in.b = "n5"
region = "n2"

[[pass.node]]
id = "n7"
op = "logic.select"
in.cond = "n6"
in.if_true = "n3"
in.if_false = "n2.prev.kept"
region = "n2"

[[pass.node]]
id = "n8"
op = "region.put"
config.acc = "kept"
in.value = "n7"
region = "n2"

[[pass.node]]
id = "n9"
op = "region.put"
config.acc = "kept"
in.value = "n2.kept"
region = "n1"

[[pass.node]]
id = "n10"
op = "lane.set"
config.lane = "y"
in.value = "n1.kept"
"#;

#[test]
fn nested_iterations_combine_in_mixed_radix() {
    let lanes = evaluator(&init(NESTED_DRAW)).init(WORLD, &[], Some(5)).expect("runs");
    let key = key_word(77, PHASE_INIT, Some(5));
    for (element, value) in values(&lanes, "y").into_iter().enumerate() {
        let bits = draw_bits(key, element as u32, 0, Some(4 + 2), 0);
        assert_eq!(
            value,
            HostValue::F32(random_float(bits, 1.0)),
            "the draw of cell {element}"
        );
    }
}

/// An init pass that draws one stream word and one counter-based number per cell, which the action `reseed` reruns.
const RESEEDED: &str = r#"
[[action]]
id = "reseed"
label = "Reseed"
runs = ["seed"]

[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"
draw_order = ["n1"]

[[pass.node]]
id = "n1"
op = "stream.bits"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = "n1"

[[pass.node]]
id = "n3"
op = "rand.float"
config.key = 9
in.max = { f32 = 1.0 }

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "y"
in.value = "n3"
"#;

#[test]
fn an_action_reruns_its_init_passes_on_the_action_stream() {
    let evaluator = evaluator(RESEEDED);
    let seed = Some(21);
    let mut lanes = evaluator.init(WORLD, &[], seed).expect("runs");
    let initial = (values(&lanes, "x"), values(&lanes, "y"));
    let start = henad_core::action::action_seed(seed);
    let mut stream = start;
    let written = evaluator
        .rerun("reseed", 1, &mut lanes, &[], seed, &mut stream)
        .expect("the rerun runs");
    assert_eq!(written, vec![0, 1], "both lanes upload");
    let mut expected = start;
    for _ in 0..64 {
        expected = henad_core::authoring::primitives::rng::xorshift64(expected);
    }
    assert_eq!(stream, expected, "one stream word per cell");
    let first_press = (values(&lanes, "x"), values(&lanes, "y"));
    assert_ne!(first_press.0, initial.0);
    assert_ne!(first_press.1, initial.1);
    let key = key_word(9, action_phase(0), seed);
    assert_eq!(
        first_press.1[5],
        HostValue::F32(random_float(draw_bits(key, 5, 1, None, 0), 1.0)),
        "the press count keys the draw"
    );
    evaluator
        .rerun("reseed", 2, &mut lanes, &[], seed, &mut stream)
        .expect("the rerun runs");
    assert_ne!(values(&lanes, "y"), first_press.1, "another press draws again");
    assert!(matches!(
        evaluator.rerun("nothing", 1, &mut lanes, &[], seed, &mut stream),
        Err(HostError::NoAction(_))
    ));
}

#[test]
fn lanes_of_another_file_fault_and_never_panic() {
    let evaluator = evaluator(&init(
        "\n[[pass.node]]\nid = \"n1\"\nop = \"ctx.index\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"lane.set\"\n\
         config.lane = \"y\"\nin.value = { f32 = 1.0 }\n",
    ));
    let other = GraphFile::parse(&format!("{}\n{KEEP_X}", GRID.replace("id = \"y\"", "id = \"z\"")))
        .expect("the other file reads");
    let short = GraphFile::parse(&format!(
        "{}\n{KEEP_X}",
        GRID.replace("[[lane]]\nid = \"y\"\ndomain = \"cells\"\ntype = \"f32\"\n", "")
    ))
    .expect("the short file reads");
    for document in [other.document(), short.document()] {
        let mut lanes = HostLanes::new(document, WORLD).expect("the lanes");
        let ran = evaluator.run_pass("seed", &mut lanes, &[], &RunContext::init(None), &mut 1);
        assert!(matches!(ran, Err(HostError::Fault { .. })), "{ran:?}");
    }
    let mut lanes = HostLanes::new(evaluator.document(), WORLD).expect("the lanes");
    evaluator
        .run_pass("seed", &mut lanes, &[], &RunContext::init(None), &mut 1)
        .expect("its own lanes run");
}

#[test]
fn lanes_past_the_host_budget_are_refused_before_they_are_allocated() {
    let largest = HostWorld::Grid {
        width: 16384,
        height: 16384,
        boundary: Boundary::Torus,
    };
    // Over 2^28 cells, `x` and `y` take a word each and the two `vec2u` lanes two words each: 6 GiB together.
    let wide = GraphFile::parse(&format!(
        "{GRID}\n[[lane]]\nid = \"v\"\ndomain = \"cells\"\ntype = \"vec2u\"\n\n\
         [[lane]]\nid = \"w\"\ndomain = \"cells\"\ntype = \"vec2u\"\n{KEEP_X}"
    ))
    .expect("the file reads");
    let refused = HostLanes::new(wide.document(), largest);
    assert!(
        matches!(&refused, Err(HostError::Unsupported { reason, .. }) if reason.contains("the host holds at most")),
        "{refused:?}"
    );
    let wide = HostEvaluator::new(&wide).expect("the file checks");
    assert!(matches!(
        wide.init(largest, &[], None),
        Err(HostError::Unsupported { .. })
    ));
}
