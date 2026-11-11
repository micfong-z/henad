//! The checker: the corpus checks clean, each rule a grid can meet reports, and every failure reports at once.

use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};

use super::corpus;
use super::host::KEEP_X;
use super::support::node_id;
use crate::GraphFile;
use crate::check::CheckReport;
use crate::limits::MAX_PASS_COST;

/// Header and declarations of a grid with a category lane `c`, a `u32` lane `x` and a few parameters.
const GRID: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_checked"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 8
height = 8

[[category]]
id = "health"
variants = [
  { name = "S", color = [0, 0, 255] },
  { name = "I", color = [255, 0, 0] },
  { name = "R", color = [128, 128, 128] },
]

[[lane]]
id = "c"
domain = "cells"
type = "cat:health"

[[lane]]
id = "x"
domain = "cells"
type = "u32"

[[param]]
id = "rate"
label = "Rate"
kind = "f32"
min = 0.0
max = 1.0
default = 0.5

[[param]]
id = "steps"
label = "Steps"
kind = "u32"
min = 0
max = 100
default = 4

[[param]]
id = "running"
label = "Running"
kind = "bool"
default = true
"#;

/// Returns the report of [`GRID`] followed by `tables`.
fn report(tables: &str) -> CheckReport {
    let text = format!("{GRID}\n{tables}");
    GraphFile::parse(&text)
        .unwrap_or_else(|error| panic!("the file reads: {error}\n{text}"))
        .check()
}

/// Returns the error messages of [`GRID`] followed by `tables`.
fn errors(tables: &str) -> Vec<String> {
    report(tables)
        .errors()
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

/// Returns the warning messages of [`GRID`] followed by `tables`.
fn warnings(tables: &str) -> Vec<String> {
    report(tables)
        .warnings()
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

/// Asserts that one error of `tables` holds `fragment`.
#[track_caller]
fn assert_error(tables: &str, fragment: &str) {
    let found = errors(tables);
    assert!(
        found.iter().any(|message| message.contains(fragment)),
        "no error holds '{fragment}':\n{}",
        found.join("\n")
    );
}

/// Returns a tick pass `step` holding `nodes`.
fn tick(nodes: &str) -> String {
    format!("[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n{nodes}")
}

#[test]
fn the_corpus_checks_without_an_error_or_a_warning() {
    let files = corpus::all();
    assert!(files.len() >= 3);
    for (name, text) in files {
        let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let report = file.check();
        let found: Vec<String> = report.diagnostics.iter().map(ToString::to_string).collect();
        assert!(found.is_empty(), "{name}:\n{}", found.join("\n"));
        assert!(report.duplicate_ids.is_empty());
        let mut ticks = 0;
        for (pass, pass_report) in file.document().passes.iter().zip(&report.passes) {
            if pass.stage == crate::document::PassStage::Tick {
                ticks += 1;
                assert!(
                    pass_report.cost > 0 && pass_report.cost <= u64::from(MAX_PASS_COST),
                    "{name} {}: {}",
                    pass.id,
                    pass_report.cost
                );
            }
        }
        assert!(ticks > 0, "{name} has a tick pass");
    }
}

#[test]
fn a_lane_read_at_neighbours_in_the_pass_that_writes_it_is_dual() {
    for (name, text) in corpus::all() {
        let report = GraphFile::parse(&text).expect("the file reads").check();
        let expected: &[&str] = match name.as_str() {
            "game_of_life.toml" => &["state"],
            "vote.toml" => &["vote"],
            "repeat_trials.toml" | "ants.toml" | "gpu_ants.toml" | "random_walk.toml" => &[],
            "boids.toml" | "gpu_boids.toml" => &["pos", "vel"],
            _ => &["health"],
        };
        assert_eq!(report.dual_lanes, expected, "{name}");
    }
}

#[test]
fn the_checker_types_each_port() {
    let text = crate::starters::starter("sir.toml").expect("the SIR starter").text;
    let report = GraphFile::parse(text).expect("the file reads").check();
    let step = report
        .passes
        .iter()
        .find(|pass| pass.id == "step")
        .expect("a step pass");
    let count = &step.outputs[&node_id(11)]["count"];
    assert_eq!(count.value_type, Some(crate::types::ValueType::U32));
    assert_eq!(count.rate, crate::types::Rate::Element);
    let base = &step.outputs[&node_id(12)]["out"];
    assert_eq!(base.rate, crate::types::Rate::Parameter);
}

#[test]
fn every_failure_is_reported_at_once_with_its_pass_and_node() {
    let found = report(&tick(
        r#"
[[pass.node]]
id = "n1"
op = "math.frobnicate"

[[pass.node]]
id = "n2"
op = "math.add"
in.a = { u32 = 1 }

[[pass.node]]
id = "n3"
op = "math.add"
in.a = { u32 = 1 }
in.b = { f32 = 1.0 }

[[pass.node]]
id = "n4"
op = "lane.set"
config.lane = "nowhere"
in.value = "n2"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "x"
in.value = "n9"
"#,
    ));
    let errors: Vec<&CodeDiagnostic> = found.errors().collect();
    assert!(errors.len() >= 5, "{errors:#?}");
    for fragment in [
        "math.frobnicate",
        "needs the input `b`",
        "is f32",
        "'nowhere'",
        "holds no node n9",
    ] {
        assert!(
            errors.iter().any(|error| error.message.contains(fragment)),
            "{fragment}: {errors:#?}"
        );
    }
    assert!(errors.iter().all(|error| error.pass.as_deref() == Some("step")));
    assert!(errors.iter().all(|error| error.node.is_some() && error.line.is_some()));
    let unknown = errors
        .iter()
        .find(|error| error.message.contains("frobnicate"))
        .expect("the unknown kind");
    assert_eq!(unknown.node.as_deref(), Some("n1"));
}

#[test]
fn types_and_rates_are_checked() {
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "conv.to_f32"
in.a = "n1"

[[pass.node]]
id = "n3"
op = "math.powi"
config.max_exponent = 4
in.base = "n2"
in.exponent = { u32 = 2 }
"#,
        ),
        "takes a constant or a parameter",
    );
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "logic.select"
in.cond = { u32 = 1 }
in.if_true = { u32 = 1 }
in.if_false = { u32 = 2 }
"#,
        ),
        "the input `cond` is u32, and the node needs bool",
    );
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "math.add"
in.a = { bool = true }
in.b = { bool = true }
"#,
        ),
        "a type the node does not take",
    );
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "math.add"
in.a = { cat = "health.Q" }
in.b = { u32 = 1 }
"#,
        ),
        "'health' has no such variant",
    );
    assert_error(
        &tick("[[pass.node]]\nid = \"n1\"\nop = \"rng.threshold\"\nin.p = { param = \"missing\" }\n"),
        "no such parameter",
    );
}

#[test]
fn contexts_and_backends_are_checked() {
    let init_count = r#"
[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "grid.count"
config.lane = "c"
in.value = { cat = "health.I" }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = "n1"
"#;
    assert_error(&format!("{KEEP_X}{init_count}"), "since it reads another element");
    let view_draw = r#"
[[pass]]
id = "paint"
stage = "view"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "rand.below"
config.key = 7
in.threshold = { u32 = 5 }
"#;
    assert_error(
        &format!("{KEEP_X}{view_draw}"),
        "does not belong in the view pass 'paint'",
    );
    let tick_stream = "[[pass.node]]\nid = \"n1\"\nop = \"stream.bits\"\n";
    let found = errors(&tick(tick_stream));
    assert!(
        found.iter().any(|message| message.contains("cannot run on the GPU")),
        "{found:#?}"
    );
    assert!(
        found
            .iter()
            .any(|message| message.contains("does not belong in the tick pass")),
        "{found:#?}"
    );
}

#[test]
fn a_view_or_stats_pass_writes_no_lane() {
    for stage in ["view", "stats"] {
        let pass = format!(
            r#"
[[pass]]
id = "late"
stage = "{stage}"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = "n1"
"#
        );
        assert_error(
            &format!("{KEEP_X}{pass}"),
            &format!("Write lane n2 does not belong in the {stage} pass 'late'"),
        );
    }
}

#[test]
fn draws_need_unique_keys_and_streams_their_draw_order() {
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "rand.below"
config.key = 7
in.threshold = { u32 = 5 }

[[pass.node]]
id = "n2"
op = "rand.chance"
config.key = 7
in.p = { f32 = 0.5 }
"#,
        ),
        "two draws with one key correlate",
    );
    let seed = r#"
[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"
draw_order = ["n2", "n3"]

[[pass.node]]
id = "n1"
op = "stream.bits"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = "n1"
"#;
    let found = errors(&format!("{KEEP_X}{seed}"));
    assert!(
        found
            .iter()
            .any(|message| message.contains("lists n2, which is no stream node")),
        "{found:#?}"
    );
    assert!(found.iter().any(|message| message.contains("lists n3")), "{found:#?}");
    assert!(
        found.iter().any(|message| message.contains("lists no such node")),
        "{found:#?}"
    );
}

#[test]
fn a_stream_draw_reads_no_draw_listed_after_it() {
    let seed = |order: &str| {
        format!(
            r#"{KEEP_X}
[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"
draw_order = [{order}]

[[pass.node]]
id = "n10"
op = "stream.bits"
in.when = "n12"

[[pass.node]]
id = "n11"
op = "stream.bits"

[[pass.node]]
id = "n12"
op = "cmp.gt"
in.a = "n11"
in.b = {{ u32 = 5 }}

[[pass.node]]
id = "n13"
op = "lane.set"
config.lane = "x"
in.value = "n10"
"#
        )
    };
    let found = report(&seed("\"n10\", \"n11\""));
    let draw = found
        .errors()
        .find(|diagnostic| diagnostic.message.contains("reads the draw of n11"))
        .unwrap_or_else(|| panic!("{:#?}", found.diagnostics));
    assert_eq!(draw.node.as_deref(), Some("n10"));
    assert!(draw.line.is_some(), "{draw:?}");
    assert!(errors(&seed("\"n11\", \"n10\"")).is_empty());
}

/// Returns a stats pass `counts` holding `nodes`, with the stat `Big` reading `n53`.
fn counts(nodes: &str) -> String {
    format!(
        "{KEEP_X}\n[[stat]]\nlabel = \"Big\"\nfrom = \"n53\"\ncolor = [1, 2, 3]\n\n[[pass]]\nid = \"counts\"\n\
         stage = \"stats\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\nid = \"n50\"\nop = \"lane.get\"\n\
         config.lane = \"x\"\n\n[[pass.node]]\nid = \"n51\"\nop = \"cmp.gt\"\nin.a = \"n50\"\nin.b = {{ u32 = 3 }}\n\n\
         [[pass.node]]\nid = \"n52\"\nop = \"stat.count_where\"\nin.pred = \"n51\"\n{nodes}"
    )
}

#[test]
fn a_reducer_ends_its_pass() {
    let mixed = counts(
        r#"
[[pass.node]]
id = "n54"
op = "conv.to_u32"
in.a = "n52"

[[pass.node]]
id = "n55"
op = "cmp.gt"
in.a = "n50"
in.b = "n54"

[[pass.node]]
id = "n53"
op = "stat.count_where"
in.pred = "n55"
"#,
    );
    let found = report(&mixed);
    let messages: Vec<&CodeDiagnostic> = found
        .errors()
        .filter(|diagnostic| diagnostic.message.contains("the result of a reducer"))
        .collect();
    assert_eq!(messages.len(), 1, "{:#?}", found.diagnostics);
    assert_eq!(messages[0].node.as_deref(), Some("n55"));
    assert!(messages[0].message.contains("the input `b`"), "{:?}", messages[0]);

    let reduced_again = counts(
        r#"
[[pass.node]]
id = "n54"
op = "conv.to_u32"
in.a = "n52"

[[pass.node]]
id = "n56"
op = "cmp.gt"
in.a = "n54"
in.b = { u32 = 3 }

[[pass.node]]
id = "n53"
op = "stat.count_where"
in.pred = "n56"
"#,
    );
    let found = errors(&reduced_again);
    assert!(
        found
            .iter()
            .any(|message| message.starts_with("Count where n53: the input `pred` reads the result of a reducer")),
        "{found:#?}"
    );

    let after_the_elements = counts(
        r#"
[[pass.node]]
id = "n53"
op = "math.add"
in.a = "n52"
in.b = { u64 = "0x1" }
"#,
    );
    assert!(
        errors(&after_the_elements).is_empty(),
        "{:#?}",
        errors(&after_the_elements)
    );
}

#[test]
fn lanes_are_written_once_and_a_grid_has_one_tick_pass() {
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "lane.set"
config.lane = "x"
in.value = { u32 = 1 }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = { u32 = 2 }
"#,
        ),
        "a pass writes a lane once per element",
    );
    assert_error(
        &format!(
            "{KEEP_X}\n{}",
            KEEP_X.replace("\"step\"", "\"again\"").replace("n90", "n80")
        ),
        "exactly one tick pass",
    );
    assert_error("", "exactly one tick pass, and this file has 0");
}

#[test]
fn a_dual_lane_cannot_skip_its_writer() {
    let pass = r#"
[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"
enabled = { param = "running" }

[[pass.node]]
id = "n1"
op = "grid.count"
config.lane = "c"
in.value = { cat = "health.I" }

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "c"
in.value = { cat = "health.S" }

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n1"
"#;
    assert_error(pass, "`enabled` cannot bind the parameter 'running'");
    assert_error(
        &pass.replace("{ param = \"running\" }", "{ param = \"rate\" }"),
        "is no `bool` parameter",
    );
}

#[test]
fn stats_actions_and_reruns_are_checked() {
    let stats = r#"
[[stat]]
label = "Infected"
from = "n2.I"
color = [255, 0, 0]

[[stat]]
label = "Raw"
from = "n1"
color = [255, 0, 0]

[[stat]]
label = "Missing"
from = "n77"
color = [255, 0, 0]

[[pass]]
id = "counts"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "c"

[[pass.node]]
id = "n2"
op = "stat.count_by"
config.category = "health"
in.value = "n1"
"#;
    let found = errors(&format!("{KEEP_X}{stats}"));
    assert!(
        found
            .iter()
            .any(|message| message.contains("'Raw' reads n1, a value per element")),
        "{found:#?}"
    );
    assert!(
        found.iter().any(|message| message.contains("'Missing' reads n77")),
        "{found:#?}"
    );
    assert!(
        !found.iter().any(|message| message.contains("'Infected'")),
        "{found:#?}"
    );
    assert_error(
        &format!("{KEEP_X}{}", stats.replace("domain = \"cells\"\n", "")),
        "the stats pass 'counts' needs a `domain`",
    );

    let actions = r#"
[[action]]
id = "empty"
label = "Empty"

[[action]]
id = "again"
label = "Again"
runs = ["seed", "nowhere"]

[[pass]]
id = "seed"
stage = "init"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n2"
op = "lane.set"
config.lane = "x"
in.value = "n1"
"#;
    let found = errors(&format!("{KEEP_X}{actions}"));
    assert!(
        found
            .iter()
            .any(|message| message.contains("'empty' has no pass and reruns none")),
        "{found:#?}"
    );
    assert!(
        found
            .iter()
            .any(|message| message.contains("reruns the pass 'nowhere'")),
        "{found:#?}"
    );
    assert!(
        found
            .iter()
            .any(|message| message.contains("which an action reruns on the host")),
        "{found:#?}"
    );
}

#[test]
fn transitions_need_a_phase_and_a_target_and_switches_every_case() {
    let transition = |rules: &str, inputs: &str| {
        tick(&format!(
            "\n[[pass.node]]\nid = \"n1\"\nop = \"cat.transition\"\nconfig.category = \"health\"\nconfig.rules = [{rules}]\nin.state = {{ cat = \"health.S\" }}\n{inputs}\n"
        ))
    };
    assert_error(
        &transition(
            r#"{ id = "infect", from = "S", to = "I" }"#,
            "in.rule.infect.when = { bool = true }",
        ),
        "a `phase` that is no number",
    );
    assert_error(
        &transition(
            r#"{ id = "infect", from = "S", to = "I", phase = -1 }"#,
            "in.rule.infect.when = { bool = true }",
        ),
        "a `phase` that is no number",
    );
    assert_error(
        &transition(
            r#"{ id = "infect", from = "S", phase = 0 }"#,
            "in.rule.infect.when = { bool = true }",
        ),
        "needs a `to` variant",
    );
    assert_error(
        &transition(
            r#"{ id = "infect", from = "Q", to = "I", phase = 0 }"#,
            "in.rule.infect.when = { bool = true }",
        ),
        "the `from` variant 'Q'",
    );
    assert_error(
        &transition(r#"{ id = "infect", from = "S", to = "I", phase = 0 }"#, ""),
        "needs the input `rule.infect.when`",
    );
    let switch = tick(
        r#"
[[pass.node]]
id = "n1"
op = "cat.switch"
config.category = "health"
in.select = { cat = "health.S" }
in.case.S = { u32 = 1 }
in.case.I = { u32 = 2 }
"#,
    );
    assert_error(&switch, "needs the input `case.R`");
    assert!(errors(&format!("{switch}in.default = {{ u32 = 0 }}\n")).is_empty());
}

#[test]
fn a_view_colours_a_category_by_its_own_palette() {
    let view = r#"
[[pass]]
id = "paint"
stage = "view"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "c"

[[pass.node]]
id = "n2"
op = "view.color"
config.palette = "life"
in.index = "n1"
"#;
    assert_error(&format!("{KEEP_X}{view}"), "no such palette or category");
    let found = errors(&format!("{KEEP_X}{}", view.replace("\"life\"", "\"health\"")));
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn duplicate_node_ids_open_the_repair_state() {
    let text = format!(
        "{GRID}\n{KEEP_X}\n[[pass]]\nid = \"counts\"\nstage = \"stats\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\nid = \"n900\"\nop = \"lane.get\"\nconfig.lane = \"x\"\n"
    );
    let report = GraphFile::parse(&text).expect("the file reads").check();
    assert_eq!(report.duplicate_ids, [node_id(900)]);
    assert!(
        report
            .errors()
            .any(|error| error.message.contains("two nodes hold the id n900"))
    );
}

/// Returns a tick pass with a Repeat `n1` of bound `max`, `body` inside it, and its sum written to `x`.
fn repeat(max: &str, count: &str, body: &str) -> String {
    tick(&format!(
        r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = {max}
config.acc = [{{ name = "sum", type = "u32", combine = "sum" }}]
in.count = {count}

[[pass.node]]
id = "n2"
op = "region.put"
config.acc = "sum"
in.value = "n1.k"
region = "n1"

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n1.sum"
{body}"#
    ))
}

#[test]
fn a_repeat_costs_one_plus_max_times_its_body_and_one() {
    let found = report(&repeat("10", "{ u32 = 3 }", ""));
    assert!(!found.has_errors(), "{:#?}", found.diagnostics);
    // Repeat 1 + 10 * (put 1 + 1), and the lane write 1.
    assert_eq!(found.passes[0].cost, 22);
    let nested = report(&repeat(
        "10",
        "{ u32 = 3 }",
        r#"
[[pass.node]]
id = "n4"
op = "region.repeat"
config.max = 5
config.acc = []
in.count = "n1.k"
region = "n1"

[[pass.node]]
id = "n5"
op = "rng.hash"
in.x = "n4.k"
region = "n4"
"#,
    ));
    // Inner Repeat 1 + 5 * (hash 2 + 1) = 16, outer 1 + 10 * (16 + put 1 + 1), and the lane write 1.
    assert_eq!(
        nested.passes[0].cost,
        1 + 10 * (16 + 1 + 1) + 1,
        "{:#?}",
        nested.diagnostics
    );
    assert!(
        nested
            .warnings()
            .any(|warning| warning.message.contains("reaches nothing"))
    );
}

#[test]
fn a_pass_past_the_cost_limit_is_refused_naming_its_heaviest_node() {
    let found = report(&repeat("65536", "{ u32 = 3 }", ""));
    let error = found
        .errors()
        .find(|error| error.message.contains("costs"))
        .expect("the cost error");
    assert_eq!(error.pass.as_deref(), Some("step"));
    assert_eq!(error.node.as_deref(), Some("n1"));
    assert!(
        error.notes.iter().any(|note| note.contains("Repeat n1 costs 131073")),
        "{error}"
    );
}

#[test]
fn nested_repeats_saturate_their_cost_and_nest_three_deep() {
    let mut body = String::new();
    for depth in 0..5u32 {
        let id = 10 + depth;
        let region = if depth == 0 {
            "n1".to_owned()
        } else {
            format!("n{}", id - 1)
        };
        body.push_str(&format!(
            "\n[[pass.node]]\nid = \"n{id}\"\nop = \"region.repeat\"\nconfig.max = 65536\nconfig.acc = []\nin.count = {{ u32 = 1 }}\nregion = \"{region}\"\n"
        ));
    }
    let found = report(&repeat("65536", "{ u32 = 3 }", &body));
    assert_eq!(found.passes[0].cost, u64::MAX);
    let deep: Vec<&CodeDiagnostic> = found.errors().filter(|error| error.message.contains("nests")).collect();
    assert_eq!(deep.len(), 3, "{deep:#?}");
}

#[test]
fn regions_keep_their_links_inside() {
    let leak = repeat(
        "4",
        "{ u32 = 3 }",
        r#"
[[pass.node]]
id = "n4"
op = "math.add"
in.a = "n1.k"
in.b = { u32 = 1 }
region = "n1"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "c"
in.value = "n4"
"#,
    );
    let found = errors(&leak);
    assert!(
        found
            .iter()
            .any(|message| message.contains("only an accumulator output leaves a region")),
        "{found:#?}"
    );
    // The leak reads as a hole, so the type of the write gives no second diagnostic.
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_error(
        &repeat(
            "4",
            "{ u32 = 3 }",
            "\n[[pass.node]]\nid = \"n4\"\nop = \"math.add\"\nin.a = \"n1.k\"\nin.b = { u32 = 1 }\n",
        ),
        "only the body of n1 can read",
    );
    assert_error(&repeat("4", "\"n1.k\"", ""), "reads its own node");
    assert_error(
        &repeat(
            "4",
            "{ u32 = 3 }",
            "\n[[pass.node]]\nid = \"n4\"\nop = \"math.add\"\nin.a = \"n1.sum\"\nin.b = { u32 = 1 }\nregion = \"n1\"\n",
        ),
        "the result of the region around it",
    );
    assert_error(
        &repeat(
            "4",
            "{ u32 = 3 }",
            "\n[[pass.node]]\nid = \"n4\"\nop = \"region.put\"\nconfig.acc = \"sum\"\nin.value = { u32 = 1 }\nregion = \"n1\"\n",
        ),
        "an accumulator takes one `region.put`",
    );
    assert_error(
        &repeat(
            "4",
            "{ u32 = 3 }",
            "\n[[pass.node]]\nid = \"n4\"\nop = \"lane.set\"\nconfig.lane = \"c\"\nin.value = { cat = \"health.S\" }\nregion = \"n1\"\n",
        ),
        "does not belong in a region body",
    );
    assert_error(
        &repeat(
            "4",
            "{ u32 = 3 }",
            "\n[[pass.node]]\nid = \"n4\"\nop = \"region.put\"\nconfig.acc = \"sum\"\nin.value = { u32 = 1 }\n",
        ),
        "sits outside every region",
    );
    assert_error(
        &repeat(
            "4",
            "{ u32 = 3 }",
            "\n[[pass.node]]\nid = \"n4\"\nop = \"math.add\"\nin.a = { u32 = 1 }\nin.b = { u32 = 1 }\nregion = \"n3\"\n",
        ),
        "which is no region",
    );
}

#[test]
fn a_carry_needs_its_start_and_a_cycle_is_reported() {
    let carry = tick(
        r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "v", type = "u32", combine = "carry" }, { name = "k", type = "u32", combine = "sum" }]
in.count = { u32 = 3 }
"#,
    );
    let found = errors(&carry);
    assert!(
        found
            .iter()
            .any(|message| message.contains("an inner output of the region holds")),
        "{found:#?}"
    );
    let carry = carry.replace(", { name = \"k\", type = \"u32\", combine = \"sum\" }", "");
    assert_error(&carry, "needs the input `init.v`");
    let cycle = tick(
        r#"
[[pass.node]]
id = "n1"
op = "math.add"
in.a = "n2"
in.b = { u32 = 1 }

[[pass.node]]
id = "n2"
op = "math.add"
in.a = "n1"
in.b = { u32 = 1 }

[[pass.node]]
id = "n3"
op = "lane.set"
config.lane = "x"
in.value = "n2"
"#,
    );
    assert_error(&cycle, "depend on each other: n1, n2");
}

#[test]
fn a_count_that_can_pass_max_warns() {
    let found = can_reach(&repeat("64", "{ param = \"steps\" }", ""));
    assert_eq!(
        found,
        ["Repeat n1: `count` can reach 100, and `max` is 64, so the loop stops at 64"]
    );
    assert!(can_reach(&repeat("100", "{ param = \"steps\" }", "")).is_empty());
    assert_eq!(can_reach(&repeat("3", "{ u32 = 4 }", "")).len(), 1);
    let clamped = repeat(
        "64",
        "\"n4\"",
        "\n[[pass.node]]\nid = \"n4\"\nop = \"math.min\"\nin.a = { param = \"steps\" }\nin.b = { u32 = 10 }\n",
    );
    assert!(can_reach(&clamped).is_empty(), "a min is bound by its smaller side");
    let one_side = repeat(
        "64",
        "\"n4\"",
        "\n[[pass.node]]\nid = \"n4\"\nop = \"math.min\"\nin.a = { param = \"steps\" }\nin.b = \"n5\"\n\n[[pass.node]]\nid = \"n5\"\nop = \"ctx.index\"\n",
    );
    assert_eq!(
        can_reach(&one_side).len(),
        1,
        "a min with one known side is bound by that side"
    );
    let added = repeat(
        "64",
        "\"n4\"",
        "\n[[pass.node]]\nid = \"n4\"\nop = \"math.add\"\nin.a = { u32 = 60 }\nin.b = { u32 = 5 }\n",
    );
    assert_eq!(can_reach(&added).len(), 1);
    let unknown = repeat("64", "\"n4\"", "\n[[pass.node]]\nid = \"n4\"\nop = \"ctx.index\"\n");
    assert!(can_reach(&unknown).is_empty(), "an element index has no bound");
    let outer_k = repeat(
        "10",
        "{ u32 = 3 }",
        "\n[[pass.node]]\nid = \"n4\"\nop = \"region.repeat\"\nconfig.max = 5\nconfig.acc = []\nin.count = \"n1.k\"\nregion = \"n1\"\n",
    );
    assert_eq!(
        can_reach(&outer_k),
        ["Repeat n4: `count` can reach 9, and `max` is 5, so the loop stops at 5"]
    );
}

/// Returns the warnings of `tables` that a Repeat's `count` can pass its `max`.
fn can_reach(tables: &str) -> Vec<String> {
    warnings(tables)
        .into_iter()
        .filter(|message| message.contains("can reach"))
        .collect()
}

#[test]
fn a_count_bound_follows_max_and_clamp() {
    let max_of = |a: &str, b: &str| {
        format!(
            "\n[[pass.node]]\nid = \"n4\"\nop = \"math.max\"\nin.a = {a}\nin.b = {b}\n\n[[pass.node]]\nid = \"n5\"\nop = \"ctx.index\"\n"
        )
    };
    assert_eq!(
        can_reach(&repeat(
            "64",
            "\"n4\"",
            &max_of("{ param = \"steps\" }", "{ u32 = 10 }")
        )),
        ["Repeat n1: `count` can reach 100, and `max` is 64, so the loop stops at 64"],
        "a max is bound by its larger side"
    );
    assert_eq!(
        can_reach(&repeat("5", "\"n4\"", &max_of("{ u32 = 3 }", "{ u32 = 10 }"))),
        ["Repeat n1: `count` can reach 10, and `max` is 5, so the loop stops at 5"]
    );
    assert!(can_reach(&repeat("10", "\"n4\"", &max_of("{ u32 = 3 }", "{ u32 = 10 }"))).is_empty());
    assert!(
        can_reach(&repeat("5", "\"n4\"", &max_of("{ u32 = 10 }", "\"n5\""))).is_empty(),
        "a max with an unknown side has no bound"
    );
    let clamp_of = |x: &str, lo: &str, hi: &str| {
        format!(
            "\n[[pass.node]]\nid = \"n4\"\nop = \"math.clamp\"\nin.x = {x}\nin.lo = {lo}\nin.hi = {hi}\n\n[[pass.node]]\nid = \"n5\"\nop = \"ctx.index\"\n"
        )
    };
    assert_eq!(
        can_reach(&repeat(
            "64",
            "\"n4\"",
            &clamp_of("\"n5\"", "{ u32 = 1 }", "{ param = \"steps\" }")
        )),
        ["Repeat n1: `count` can reach 100, and `max` is 64, so the loop stops at 64"],
        "a clamp is bound by its `hi`, whatever its `x`"
    );
    assert!(
        can_reach(&repeat(
            "64",
            "\"n4\"",
            &clamp_of("{ param = \"steps\" }", "{ u32 = 0 }", "{ u32 = 10 }")
        ))
        .is_empty(),
        "a clamp below `max` bounds a parameter that passes it"
    );
    assert_eq!(
        can_reach(&repeat(
            "8",
            "\"n4\"",
            &clamp_of("\"n5\"", "{ u32 = 20 }", "{ u32 = 9 }")
        )),
        ["Repeat n1: `count` can reach 9, and `max` is 8, so the loop stops at 8"],
        "`hi` wins when `lo` passes it"
    );
}

#[test]
fn host_types_stay_off_the_gpu_per_element() {
    assert_error(
        &tick(
            r#"
[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "wide", type = "u64", combine = "sum" }]
in.count = { u32 = 3 }
"#,
        ),
        "is u64 per element, which only an init pass holds",
    );
}

#[test]
fn a_warning_has_its_severity() {
    let found = report(&tick("\n[[pass.node]]\nid = \"n1\"\nop = \"ctx.index\"\n"));
    let warning = found.warnings().next().expect("the warning");
    assert_eq!(warning.severity, DiagnosticSeverity::Warning);
    assert!(
        warning
            .to_string()
            .contains("warning: Element index n1 reaches nothing")
    );
}

#[test]
fn function_calls_saturate_cost_and_a_call_cycle_is_reported() {
    let mut functions = String::new();
    for index in 0..12u32 {
        let next = if index == 11 { 0 } else { index + 1 };
        functions.push_str(&format!(
            "\n[[function]]\nid = \"f{index}\"\nkind = \"pure\"\n\n[[function.node]]\nid = \"n{}\"\nop = \"fn.call\"\nconfig.function = \"f{next}\"\nconfig.key = {}\n\n[[function.node]]\nid = \"n{}\"\nop = \"region.repeat\"\nconfig.max = 65536\nconfig.acc = []\nin.count = {{ u32 = 1 }}\n",
            100 + 2 * index,
            1000 + index,
            101 + 2 * index,
        ));
    }
    let call = "\n[[pass.node]]\nid = \"n1\"\nop = \"fn.call\"\nconfig.function = \"f0\"\nconfig.key = 5\n";
    let found = report(&format!("{functions}\n{}", tick(call)));
    assert!(found.errors().any(|error| error.message.contains("calls itself")));
    assert_eq!(found.passes[0].cost, u64::MAX);
    // A chain without the cycle is deep and its cost saturates through the calls.
    let chain = functions.replace("config.function = \"f0\"", "config.function = \"none\"");
    let found = report(&format!("{chain}\n{}", tick(call)));
    assert!(
        found
            .errors()
            .any(|error| error.message.contains("deeper than 8 functions")),
        "{:#?}",
        found.diagnostics
    );
    assert!(found.passes[0].cost > u64::from(MAX_PASS_COST));
}

#[test]
fn the_checker_places_a_diagnostic_as_the_file_does() {
    let text = format!(
        "{GRID}\r\n# é\r\n{}",
        tick("\n[[pass.node]]\nid = \"n1\"\nop = \"math.frobnicate\"\n")
    );
    let file = GraphFile::parse(&text).expect("the file reads");
    let placer = file.placer();
    for span in [
        0..0,
        3..9,
        400..410,
        text.len() - 10..text.len(),
        text.len()..text.len() + 4,
    ] {
        let expected = file.place(CodeDiagnostic::error("x"), span.clone());
        assert_eq!(placer.place(CodeDiagnostic::error("x"), span), expected);
    }
    let found = file.check();
    let error = found.errors().next().expect("the unknown kind");
    let kind = file.places().passes[0].nodes[0].op.clone();
    assert_eq!(
        *error,
        file.place(error.clone(), kind).with_pass("step").with_node("n1")
    );
}
