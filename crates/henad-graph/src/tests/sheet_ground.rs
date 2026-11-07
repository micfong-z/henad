//! The ground, picks, places and Repeats of agent sheets, and the folds that looks and picks over one neighbourhood
//! share: the ants starter lowered and run on the host, its one fused fold, the split past a fold's accumulators on a
//! grid card, the second fold of a look that reads an earlier look, the add-up merge, the `or` of a Become inside a
//! Repeat, a button over the ants and the ground, the open test of moves, and the refusals that remain.

use henad_compute::fault::FaultSink;
use henad_compute::gpu::{GpuContext, wgpu};
use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::ParamValue;

use super::support::test_device;
use crate::document::FieldCombine;
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::node::Node;
use crate::sheet::{SheetFile, SheetFileError, sentence, starters};
use crate::value::{ConfigValue, InputValue};
use crate::wgsl::AgentProgram;

/// Returns the text of the starter `file_name`.
fn starter(file_name: &str) -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .map(|starter| starter.text)
        .unwrap_or_else(|| panic!("no starter {file_name}"))
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Lowers `text`, which must lower.
fn lowered(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("the sheet does not lower: {error}"))
}

/// Returns the errors of lowering `text`, which must hold some.
fn refusals(text: &str) -> Vec<henad_core::diagnostic::CodeDiagnostic> {
    match SheetFile::parse(text) {
        Ok(_) => panic!("the sheet lowers:\n{text}"),
        Err(SheetFileError::Invalid(errors)) => errors.as_slice().to_vec(),
        Err(other) => panic!("the sheet does not read: {other}"),
    }
}

/// Returns the bytes of `text` that `diagnostic` points at.
fn placed_at<'a>(diagnostic: &henad_core::diagnostic::CodeDiagnostic, text: &'a str) -> &'a str {
    let span = diagnostic
        .span
        .clone()
        .unwrap_or_else(|| panic!("{diagnostic} has no place"));
    &text[span]
}

/// Returns the nodes of the pass `pass` of `file`.
fn pass_nodes<'a>(file: &'a SheetFile, pass: &str) -> &'a [Node] {
    &file
        .lowered()
        .document()
        .passes
        .iter()
        .find(|known| known.id == pass)
        .unwrap_or_else(|| panic!("no pass {pass}"))
        .body
        .nodes
}

/// Returns the accumulators of the fold `node`.
fn accumulators(node: &Node) -> Vec<String> {
    match node.config.get("acc") {
        Some(ConfigValue::Array(entries)) => entries
            .iter()
            .filter_map(|entry| match entry {
                ConfigValue::Table(table) => match table.get("name") {
                    Some(ConfigValue::String(name)) => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Default values of the ants' parameters, in the sheet's order.
const ANTS_PARAMS: [ParamValue; 5] = [
    ParamValue::F32(0.9),
    ParamValue::F32(1.0),
    ParamValue::F32(0.8),
    ParamValue::F32(0.1),
    ParamValue::F32(0.999),
];

/// Returns the vector of `value`, a `vec2f`.
fn vector(value: Option<HostValue>) -> [f32; 2] {
    match value {
        Some(HostValue::Vec2F(xy)) => xy,
        other => panic!("{other:?} is no vec2f"),
    }
}

/// The ants lower with no warning, check clean, and run on the host: every ant starts at the centre of the nest cell,
/// the painted ground holds the two rocks, the food and the nest, and after 20 ticks every ant stands at the centre of
/// an open cell inside the world.
#[test]
fn the_ants_sheet_lowers_checks_and_runs_on_the_host() {
    let file = lowered(starter("ants.toml"));
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let report = file.lowered().check();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Agents {
        count: 50,
        width: 200.0,
        height: 200.0,
    };
    let mut lanes: HostLanes = evaluator.init(world, &ANTS_PARAMS, Some(3)).expect("the init runs");
    let positions = lanes.column("position").expect("the position lane");
    for ant in 0..50 {
        assert_eq!(
            vector(positions.get(ant)),
            [175.5, 175.5],
            "ant {ant} starts at the nest"
        );
    }
    let ground = lanes.column("ground.ground").expect("the ground layer");
    let state_at = |x: usize, y: usize| ground.get(y * 200 + x);
    let state = |index: u32| Some(HostValue::Category(index));
    assert_eq!(state_at(25, 25), state(2), "the food");
    assert_eq!(state_at(175, 175), state(3), "the nest");
    assert_eq!(state_at(100, 145), state(1), "a rock's centre");
    assert_eq!(state_at(90, 55), state(1), "the other rock's centre");
    assert_eq!(state_at(10, 190), state(0), "open ground");
    let rocks = (0..ground.len()).filter(|&cell| ground.get(cell) == state(1)).count();
    // Each ellipse is 112 by 20 cells, about 1760 cells, and the two do not overlap.
    assert!((3000..4000).contains(&rocks), "{rocks} rock cells");
    let mut stream = 0;
    for tick in 0..20 {
        evaluator
            .run_pass(
                "tick_ant_tick",
                &mut lanes,
                &ANTS_PARAMS,
                &RunContext::tick(Some(3), tick),
                &mut stream,
            )
            .expect("a tick runs");
    }
    let positions = lanes.column("position").expect("the position lane");
    let ground = lanes.column("ground.ground").expect("the ground layer");
    for ant in 0..50 {
        let [x, y] = vector(positions.get(ant));
        assert!(x.fract() == 0.5 && y.fract() == 0.5, "ant {ant} at ({x}, {y})");
        assert!(
            (0.0..200.0).contains(&x) && (0.0..200.0).contains(&y),
            "ant {ant} at ({x}, {y})"
        );
        // The coordinates are cell centres inside the world, so the truncation is the cell.
        let cell = y as usize * 200 + x as usize;
        assert_ne!(ground.get(cell), state(1), "ant {ant} stands on a rock");
    }
}

/// The ants' card lowers to one fold over the 8 cells around: Food way, Home way and the random open neighbour of
/// line 7 share it with eight loop variables, the two Leaves on the trails merge into one deposit, and the Record adds
/// to the Deliveries counter.
#[test]
fn the_ants_card_lowers_to_one_fused_fold_per_neighbourhood() {
    let file = lowered(starter("ants.toml"));
    let tick = pass_nodes(&file, "tick_ant_tick");
    let folds: Vec<&Node> = tick.iter().filter(|node| node.op == "grid.fold").collect();
    assert_eq!(folds.len(), 1);
    assert_eq!(
        accumulators(folds[0]),
        [
            "l1_best",
            "l1_xy",
            "l1_ties",
            "l2_best",
            "l2_xy",
            "l2_ties",
            "l7_p1_xy",
            "l7_p1_seen"
        ]
    );
    assert_eq!(
        folds[0].config.get("boundary"),
        Some(&ConfigValue::String("bounded".to_owned()))
    );
    let deposits: Vec<&Node> = tick.iter().filter(|node| node.op == "field.deposit").collect();
    assert_eq!(deposits.len(), 1, "the two exclusive Leaves share one deposit");
    assert!(
        matches!(deposits[0].inputs.get("layer"), Some(InputValue::Link(_))),
        "the deposit chooses its layer"
    );
    assert_eq!(tick.iter().filter(|node| node.op == "counter.add").count(), 1);
    assert_eq!(
        tick.len(),
        ANTS_TICK_NODES,
        "the ants' tick pass holds {} nodes",
        tick.len()
    );
    let fields = &file.lowered().document().fields;
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[1].layers, ["to_food", "to_home"]);
    assert_eq!(fields[1].combine, Some(FieldCombine::Max));
    assert_eq!(fields[1].finish.as_deref(), Some("fade_to_food"));
}

/// Nodes of the ants' lowered tick pass.
const ANTS_TICK_NODES: usize = 110;

/// Game of Life with Look around lines `l5` and `l6` added to the card that runs each tick, each with `first` and
/// `second` summaries of whether any neighbour is alive.
fn life_with_looks(first: usize, second: usize) -> String {
    let text = edited(starter("game_of_life.toml"), "next_line = 5", "next_line = 7");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\", \"l6\"]",
    );
    let look = |id: &str, count: usize| {
        let summaries: Vec<String> = (1..=count)
            .map(|index| {
                format!(
                    "  {{ id = \"s{index}\", name = \"s{index}\", summary = \"any\", value = {{ kind = \
                     \"compare\", test = \"is\", left = {{ their = \"status\" }}, right = {{ state = \"alive\" }} \
                     }} }},"
                )
            })
            .collect();
        format!(
            "[card.line.{id}]\nkind = \"look_around\"\nname = \"Look {id}\"\naround = \"cells_8\"\nsummaries = \
             [\n{}\n]\n",
            summaries.join("\n")
        )
    };
    edited(
        &text,
        "# Lines of Clear: run once, every cell, when pressed",
        &format!(
            "{}\n{}\n# Lines of Clear: run once, every cell, when pressed",
            look("l5", first),
            look("l6", second)
        ),
    )
}

/// Two looks over the 8 cells around that take 16 accumulators share one fold, and two that take 17 split into two,
/// each in line id order.
#[test]
fn seventeen_accumulators_split_into_two_folds() {
    let folds = |text: &str| -> Vec<Vec<String>> {
        let file = lowered(text);
        pass_nodes(&file, "tick_cell_tick")
            .iter()
            .filter(|node| node.op == "grid.fold")
            .map(accumulators)
            .collect()
    };
    let sixteen = folds(&life_with_looks(8, 6));
    assert_eq!(sixteen.len(), 1);
    assert_eq!(sixteen[0].len(), 16);
    let seventeen = folds(&life_with_looks(8, 7));
    assert_eq!(seventeen.len(), 2);
    assert_eq!(seventeen[0].len(), 9);
    assert_eq!(seventeen[1].len(), 8);
    assert!(seventeen[1].iter().all(|name| name.starts_with("l6_")));
}

/// A look at the 3 by 3 block and a look at the 8 cells around share one fold that includes the cell itself, and the
/// look at the 8 cells around skips offset (0, 0) through a guard. The values sheet runs its three looks over the 8
/// cells around in one fold and its look at the 4 cells around in another.
#[test]
fn a_block_and_the_8_cells_around_share_a_fold_with_a_guard() {
    let text = edited(
        &life_with_looks(1, 1),
        "name = \"Look l6\"\naround = \"cells_8\"",
        "name = \"Look l6\"\naround = \"block_3x3\"",
    );
    let file = lowered(&text);
    let tick = pass_nodes(&file, "tick_cell_tick");
    let folds: Vec<&Node> = tick.iter().filter(|node| node.op == "grid.fold").collect();
    assert_eq!(folds.len(), 1);
    assert_eq!(folds[0].config.get("include_center"), Some(&ConfigValue::Bool(true)));
    let guards = tick
        .iter()
        .filter(|node| node.op == "cmp.ne" && node.region == Some(folds[0].id))
        .filter(|node| {
            matches!(
                node.inputs.get("a"),
                Some(InputValue::Link(link)) if link.port.as_deref() == Some("offset")
            )
        })
        .count();
    assert_eq!(guards, 1, "one guard for the look at the 8 cells around");
    let values = lowered(include_str!("sheets/values.toml"));
    let orders: Vec<Option<&ConfigValue>> = values
        .lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| node.op == "grid.fold")
        .map(|node| node.config.get("order"))
        .collect();
    assert_eq!(orders.len(), 2, "{orders:?}");
}

/// A ground property that adds up its marks lowers to a field that merges with `sum` at a resolution of 2^-16, and the
/// ants run with it on the device.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn a_ground_property_that_adds_up_lowers_to_a_sum_and_runs() {
    let text = edited(
        &edited(starter("ants.toml"), "marks = \"largest\"", "marks = \"add_up\""),
        "marks = \"largest\"",
        "marks = \"add_up\"",
    );
    let file = lowered(&text);
    let fields = &file.lowered().document().fields;
    assert_eq!(fields[1].combine, Some(FieldCombine::Sum { resolution_shift: 16 }));
    assert!(
        file.lowered()
            .to_toml()
            .contains("combine = \"sum\"\nresolution = 1.52587890625e-5")
    );
    let Some(device) = test_device() else {
        eprintln!("skipping the device half of a_ground_property_that_adds_up_lowers_to_a_sum_and_runs: no adapter");
        return;
    };
    let ctx = GpuContext::new(
        device.device,
        device.queue,
        wgpu::TextureFormat::Rgba8Unorm,
        FaultSink::new(),
    );
    let entry = AgentProgram::compile_with_fault_names(file.lowered(), file.fault_names())
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry();
    let params: Vec<ParamValue> = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind().default_value())
        .collect();
    let Ok(henad_compute::entry::ModelState::Gpu(mut state)) = entry.build(&params, Some(1), Some(&ctx)) else {
        panic!("the ants build on the device");
    };
    henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 100).expect("100 ticks run");
}

/// Ants with a Repeat of 2 holding a Become: the Become's condition is an `or` accumulator of the Repeat, and the
/// change reads its total. A Set inside the Repeat whose value reads a line inside it is refused.
#[test]
fn a_become_inside_a_repeat_applies_when_any_time_holds() {
    let text = edited(starter("ants.toml"), "next_line = 17", "next_line = 20");
    let text = edited(&text, "\"l12\", \"l16\"]", "\"l12\", \"l16\", \"l17\"]");
    let repeat = "[card.line.l17]\nkind = \"repeat\"\ntimes = 2\nlines = [\"l18\", \"l19\"]\n\n[card.line.l18]\n\
                  kind = \"chance\"\nname = \"Lost\"\nkey = 41\nchance = 0.01\n\n[card.line.l19]\nkind = \
                  \"become\"\nproperty = \"cargo\"\nnew_state = \"searching\"\nstart_state = \"carrying_food\"\n\
                  condition = { result = \"l18\" }\n";
    let text = format!("{text}\n{repeat}");
    let file = lowered(&text);
    let tick = pass_nodes(&file, "tick_ant_tick");
    let region = tick
        .iter()
        .find(|node| node.op == "region.repeat")
        .expect("the Repeat's region");
    assert_eq!(accumulators(region), ["l19_applies"]);
    let transition = tick
        .iter()
        .find(|node| node.op == "cat.transition")
        .expect("the transition of Cargo");
    assert!(
        transition.inputs.values().any(|input| matches!(
            input,
            InputValue::Link(link) if link.node == region.id
        )),
        "a row of the transition reads the Repeat's total"
    );
    let set = edited(
        &text,
        "[card.line.l19]\nkind = \"become\"\nproperty = \"cargo\"\nnew_state = \"searching\"\nstart_state = \
         \"carrying_food\"\ncondition = { result = \"l18\" }",
        "[card.line.l19]\nkind = \"set\"\nproperty = \"reward_held\"\n\n[card.line.l19.value]\nkind = \"calculate\"\n\
         formula = \"l18\"\n\n[card.line.l19.condition]\nkind = \"always\"",
    );
    let errors = refusals(&set);
    assert!(
        errors
            .iter()
            .any(|error| error.message.ends_with(sentence::SET_IN_REPEAT)),
        "{errors:#?}"
    );
}

/// The refusals the ground leaves: walls around agents that move by an arrow, the cell ahead of agents that move by an
/// arrow, and a start on a state the ground paints larger than one cell. Each gives one located error.
#[test]
fn each_ground_refusal_gives_one_located_error() {
    let one = |text: &str, message: &str, at: &str| {
        let errors = refusals(text);
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert!(errors[0].message.ends_with(message), "{}", errors[0].message);
        assert!(
            placed_at(&errors[0], text).starts_with(at),
            "{} is placed at {:?}",
            errors[0].message,
            placed_at(&errors[0], text)
        );
    };
    let walls = edited(starter("random_walk.toml"), "edges = \"wrap\"", "edges = \"walls\"");
    one(&walls, sentence::NOT_YET_AGENT_WALLS, "[world]");
    let rock_start = edited(
        starter("ants.toml"),
        "rule = \"on_ground\"\nproperty = \"ground\"\nstate = \"nest\"",
        "rule = \"on_ground\"\nproperty = \"ground\"\nstate = \"obstacle\"",
    );
    one(&rock_start, sentence::ON_GROUND_CELLS, "[[agents.property]]");
    let ahead_by_arrow = edited(
        &edited(starter("boids.toml"), "next_line = 9", "next_line = 10"),
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\", \"l9\"]",
    );
    let ahead_by_arrow = edited(
        &ahead_by_arrow,
        "# Lines of Randomise headings",
        "[card.line.l9]\nkind = \"look_at_cell\"\nname = \"Ahead\"\nwhich = \"ahead\"\n\n# Lines of Randomise headings",
    );
    one(&ahead_by_arrow, sentence::AHEAD_NEEDS_CELLS, "[card.line.l9]");
}

/// A Count of the food around an ant joins the fold of the ants' picks, and reads the ground at each cell it visits.
#[test]
fn a_count_of_the_ground_around_joins_the_fold_of_the_picks() {
    let text = edited(starter("ants.toml"), "next_line = 17", "next_line = 18");
    let text = edited(&text, "\"l12\", \"l16\"]", "\"l12\", \"l16\", \"l17\"]");
    let text = format!(
        "{text}\n[card.line.l17]\nkind = \"count\"\nname = \"Food around\"\naround = \"cells_8\"\n\n\
         [card.line.l17.whose]\nkind = \"compare\"\ntest = \"is\"\nleft = {{ their = \"ground\" }}\nright = {{ state = \
         \"food\" }}\n"
    );
    let file = lowered(&text);
    let tick = pass_nodes(&file, "tick_ant_tick");
    let folds: Vec<&Node> = tick.iter().filter(|node| node.op == "grid.fold").collect();
    assert_eq!(folds.len(), 1);
    assert_eq!(accumulators(folds[0]).last().map(String::as_str), Some("l17_count"));
    let reads = tick
        .iter()
        .filter(|node| node.op == "field.get" && node.region == Some(folds[0].id))
        .count();
    assert_eq!(reads, 4, "the open test, the two trails and the count's ground");
}

/// A grid sheet whose Count over the 8 cells around compares their Wealth with the average a Look around at the same
/// cells works out, then sets Wealth to that count.
const LOOK_READS_LOOK: &str = r##"format = "henad-model"
format_version = 1
kind = "sheet"

[sheet]
version = 1
id = "look_reads_look"
name = "Look reads look"
next_line = 4

[world]
made_of = "cells"
width = 8
height = 8
edges = "wrap"

[[cells.property]]
id = "wealth"
name = "Wealth"
type = "number"

[cells.property.starts_as]
rule = "value"
value = 1

[[card]]
id = "cell_tick"
every = "cells"
runs = "each_tick"
lines = ["l1", "l2", "l3"]

[card.line.l1]
kind = "look_around"
name = "Around"
around = "cells_8"
summaries = [
  { id = "mean", name = "mean", summary = "average", value = { their = "wealth" } },
]

[card.line.l2]
kind = "count"
name = "Richer"
around = "cells_8"

[card.line.l2.whose]
kind = "compare"
test = "more_than"
left = { their = "wealth" }
right = { result = "l1", part = "mean" }

[card.line.l3]
kind = "set"
property = "wealth"
value = { result = "l2" }

[card.line.l3.condition]
kind = "always"
"##;

/// A Count that reads the average of a Look around over the same cells opens a second fold, since one fold cannot
/// read its own result. The sheet checks and runs on the host: with every cell at Wealth 1, no neighbour is richer
/// than the average, so every cell's Wealth is 0 after one tick.
#[test]
fn a_look_that_reads_an_earlier_look_opens_its_own_fold() {
    let file = lowered(LOOK_READS_LOOK);
    let report = file.lowered().check();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let tick = pass_nodes(&file, "tick_cell_tick");
    let folds: Vec<Vec<String>> = tick
        .iter()
        .filter(|node| node.op == "grid.fold")
        .map(accumulators)
        .collect();
    assert_eq!(folds, [vec!["l1_count", "l1_summary_mean"], vec!["l2_count"]]);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Grid {
        width: 8,
        height: 8,
        boundary: Boundary::Torus,
    };
    let mut lanes: HostLanes = evaluator.init(world, &[], Some(1)).expect("the init runs");
    evaluator
        .run_pass("tick_cell_tick", &mut lanes, &[], &RunContext::tick(Some(1), 0), &mut 0)
        .expect("a tick runs");
    let wealth = lanes.column("wealth").expect("the wealth lane");
    for cell in 0..64 {
        assert_eq!(wealth.get(cell), Some(HostValue::F32(0.0)), "cell {cell}");
    }
}

/// A pick whose `only` test reads the amount of an earlier pick over the same cells opens a second fold, which the
/// random pick of line 7, reading neither, joins as the latest fold.
#[test]
fn a_pick_that_reads_an_earlier_pick_opens_its_own_fold() {
    let text = edited(
        starter("ants.toml"),
        "open_only = true\npick = \"most\"\nranked_by = \"to_home\"\n",
        "open_only = true\npick = \"most\"\nranked_by = \"to_home\"\n\n[card.line.l2.only]\nkind = \"compare\"\ntest = \
         \"at_least\"\nleft = { their = \"to_home\" }\nright = { result = \"l1\", part = \"amount\" }\n",
    );
    let file = lowered(&text);
    let report = file.lowered().check();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let tick = pass_nodes(&file, "tick_ant_tick");
    let folds: Vec<Vec<String>> = tick
        .iter()
        .filter(|node| node.op == "grid.fold")
        .map(accumulators)
        .collect();
    assert_eq!(folds.len(), 2, "{folds:?}");
    assert_eq!(folds[0], ["l1_best", "l1_xy", "l1_ties"]);
    assert_eq!(folds[1], ["l2_best", "l2_xy", "l2_ties", "l7_p1_xy", "l7_p1_seen"]);
}

/// A button that starts again an ant property and a ground property reruns two passes of its own, one for the ants
/// and one for the ground. Both lower, check, and run on the host: the reruns set Cargo back to Searching and clear
/// To food.
#[test]
fn a_button_that_starts_ants_and_ground_again_reruns_two_passes() {
    let text = edited(
        starter("ants.toml"),
        "[[button.does]]\nrule = \"start_again\"\nthing = \"ant\"\n\n[[button.does]]\nrule = \"set_everywhere\"\n\
         property = \"to_food\"\nvalue = 0\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \"to_home\"\n\
         value = 0\n",
        "[[button.does]]\nrule = \"start_again\"\nproperties = [\"cargo\", \"to_food\"]\n",
    );
    let file = lowered(&text);
    let report = file.lowered().check();
    assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
    let actions = &file.lowered().document().actions;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].runs, ["restart_reset_colony", "restart_reset_colony_ground"]);
    let layer = format!("{}.to_food", file.lowered().document().fields[1].id);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Agents {
        count: 50,
        width: 200.0,
        height: 200.0,
    };
    let mut lanes: HostLanes = evaluator.init(world, &ANTS_PARAMS, Some(3)).expect("the init runs");
    let trail = lanes.column_mut(&layer).expect("the To food layer");
    for cell in 0..trail.len() {
        trail.set(cell, HostValue::F32(0.5));
    }
    let cargo = lanes.column_mut("cargo").expect("the cargo lane");
    for ant in 0..50 {
        cargo.set(ant, HostValue::Category(1));
    }
    let mut stream = 0;
    for pass in &actions[0].runs {
        evaluator
            .run_pass(
                pass,
                &mut lanes,
                &ANTS_PARAMS,
                &RunContext::action(Some(3), 0, 1, 0),
                &mut stream,
            )
            .unwrap_or_else(|error| panic!("{pass} does not run: {error}"));
    }
    let trail = lanes.column(&layer).expect("the To food layer");
    assert!((0..trail.len()).all(|cell| trail.get(cell) == Some(HostValue::F32(0.0))));
    let cargo = lanes.column("cargo").expect("the cargo lane");
    assert!((0..50).all(|ant| cargo.get(ant) == Some(HostValue::Category(0))));
}

/// Ants whose Next cell takes the cell ahead with no test that it is open, from a nest beside a rock, never stand on
/// the rock over 20 ticks on the host: a Move to applies only where its place is open. A Move by over ground that
/// blocks some states is refused at its line.
#[test]
fn a_move_to_a_blocked_cell_leaves_the_agent_where_it_is() {
    let text = edited(
        starter("ants.toml"),
        "shape = \"cell\"\nstate = \"nest\"\ncenter = [0.875, 0.875]",
        "shape = \"cell\"\nstate = \"nest\"\ncenter = [0.55, 0.775]",
    );
    let text = edited(
        &text,
        "[[card.line.l7.row.condition.items]]\nresult = \"l6\"\npart = \"open\"\n\n",
        "",
    );
    let file = lowered(&text);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let world = HostWorld::Agents {
        count: 50,
        width: 200.0,
        height: 200.0,
    };
    let mut lanes: HostLanes = evaluator.init(world, &ANTS_PARAMS, Some(5)).expect("the init runs");
    let ground = lanes.column("ground.ground").expect("the ground layer");
    assert_eq!(ground.get(155 * 200 + 110), Some(HostValue::Category(3)), "the nest");
    let mut stream = 0;
    let mut moved = 0;
    for tick in 0..20 {
        evaluator
            .run_pass(
                "tick_ant_tick",
                &mut lanes,
                &ANTS_PARAMS,
                &RunContext::tick(Some(5), tick),
                &mut stream,
            )
            .expect("a tick runs");
        let positions = lanes.column("position").expect("the position lane");
        let ground = lanes.column("ground.ground").expect("the ground layer");
        for ant in 0..50 {
            let [x, y] = vector(positions.get(ant));
            let cell = y as usize * 200 + x as usize;
            assert_ne!(
                ground.get(cell),
                Some(HostValue::Category(1)),
                "ant {ant} stands on a rock at ({x}, {y}) after tick {tick}"
            );
            if [x, y] != [110.5, 155.5] {
                moved += 1;
            }
        }
    }
    assert!(moved > 0, "some ant leaves the nest");
    let move_by = edited(
        starter("random_walk.toml"),
        "[[parameter]]",
        "[ground]\nblocked = [\n  { property = \"terrain\", state = \"rock\" },\n]\n\n[[ground.property]]\nid = \
         \"terrain\"\nname = \"Terrain\"\ntype = \"states\"\nstates = [\n  { id = \"open\", name = \"Open\", color = \
         \"#0E0E12\" },\n  { id = \"rock\", name = \"Rock\", color = \"#5A5A62\" },\n]\n\n\
         [ground.property.starts_as]\nrule = \"painted\"\nbase = \"open\"\n\n[[parameter]]",
    );
    let errors = refusals(&move_by);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].message.ends_with(sentence::NOT_YET_BLOCKED_MOVE_BY),
        "{}",
        errors[0].message
    );
    assert!(placed_at(&errors[0], &move_by).starts_with("[card.line.l2]"));
}
