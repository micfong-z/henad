//! The lowering of agent sheets: every agent starter checked and compiled, the folds of the looks at agents and their
//! index, the power rule of a chance once for each agent on the host and the device, the refusals of the lines this
//! version does not lower and of a property changed two ways, the names of the folds and the cost in the model's words.

use henad_compute::gpu::wgpu;
use henad_core::params::ParamValue;

use super::sheet_support::{
    MOBILE, accumulators, agent_program_of, boids_with, edited, entry_of, host_of, lowered, only_refusal, placed_at,
    refusals, sheet_text,
};
use super::support::{built, context, device_for, node_id, params_of};
use crate::document::{PassStage, World};
use crate::host::{HostLanes, HostValue, HostWorld, RunContext};
use crate::sheet::{NodeOwner, SheetFile, sentence, starters};
use crate::value::{ConfigValue, InputValue};
use crate::wgsl::AgentProgram;

/// Returns the agent starters with their file names.
fn agent_sheets() -> Vec<(&'static str, &'static str)> {
    starters()
        .iter()
        .filter(|starter| starter.text.contains("made_of = \"agents\""))
        .map(|starter| (starter.file_name, starter.text))
        .collect()
}

/// Returns the nodes of every pass of `file` whose kind is `op`.
fn nodes_of_kind<'a>(file: &'a SheetFile, op: &'a str) -> impl Iterator<Item = &'a crate::node::Node> + 'a {
    file.lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(move |node| node.op == op)
}

#[test]
fn every_agent_sheet_lowers_checks_and_compiles_on_the_host() {
    let names: Vec<&str> = agent_sheets().iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        ["random_walk.toml", "boids.toml", "bounded_confidence.toml", "ants.toml"]
    );
    for (file_name, text) in agent_sheets().into_iter().chain([("mobile_sir.toml", MOBILE)]) {
        let file = lowered(text);
        assert!(file.warnings().is_empty(), "{file_name}: {:?}", file.warnings());
        let report = file.lowered().check();
        assert!(report.diagnostics.is_empty(), "{file_name}: {:#?}", report.diagnostics);
        assert!(
            matches!(file.lowered().document().world, World::Agents(_)),
            "{file_name}"
        );
        let entry = entry_of(&file);
        assert_eq!(entry.id(), file.document().sheet.id);
        assert_eq!(file.to_toml(), text, "{file_name} is its own canonical text");
        assert_eq!(
            lowered(text).lowered().to_toml(),
            file.lowered().to_toml(),
            "{file_name}: two lowerings write one text"
        );
        // Every node a line owns sits in that line's block of ids.
        for (id, origin) in file.line_map().iter() {
            if let NodeOwner::Line(line) = &origin.owner {
                let number: u32 = line[1..].parse().expect("a line number");
                assert_eq!(id.number() / 1000, number, "{file_name}: {id} of {line}");
            }
        }
        let document = file.lowered().document();
        let written = document.passes.iter().flat_map(|pass| &pass.body.nodes).count()
            + document
                .functions
                .iter()
                .flat_map(|function| &function.body.nodes)
                .count();
        assert_eq!(
            written,
            file.line_map().len(),
            "{file_name}: the line map covers every node"
        );
    }
}

#[test]
fn the_agent_starters_lower_their_worlds_and_drop_an_unread_heading() {
    let boids = lowered(sheet_text("boids.toml"));
    let World::Agents(world) = &boids.lowered().document().world else {
        panic!("boids lower to agents");
    };
    assert_eq!((world.count, world.max_count), (50_000, 1_000_000));
    assert_eq!((world.width, world.height), (1000.0, 1000.0));
    let lanes: Vec<&str> = boids
        .lowered()
        .document()
        .lanes
        .iter()
        .map(|lane| lane.id.as_str())
        .collect();
    assert_eq!(lanes, ["position", "heading", "velocity"]);
    assert!(
        boids.lowered().document().lanes[0].role.is_some(),
        "Position is the position lane"
    );
    // Bounded confidence colours by opinion, and nothing reads its Heading.
    let opinions = lowered(sheet_text("bounded_confidence.toml"));
    let lanes: Vec<&str> = opinions
        .lowered()
        .document()
        .lanes
        .iter()
        .map(|lane| lane.id.as_str())
        .collect();
    assert_eq!(lanes, ["position", "opinion"]);
    let passes: Vec<&str> = opinions
        .lowered()
        .document()
        .passes
        .iter()
        .map(|pass| pass.id.as_str())
        .collect();
    assert!(
        passes.contains(&"start_person") && passes.contains(&"restart_mix_opinions"),
        "{passes:?}"
    );
    assert!(passes.contains(&"tick_person_tick"), "{passes:?}");
}

#[test]
fn the_looks_of_a_card_share_one_fold_at_the_largest_distance() {
    let boids = lowered(sheet_text("boids.toml"));
    let folds: Vec<_> = nodes_of_kind(&boids, "agent.radius_fold").collect();
    assert_eq!(folds.len(), 1, "Flockmates and Too close share one fold");
    let names = accumulators(folds[0]);
    assert_eq!(
        names,
        [
            "l1_count",
            "l1_summary_average_velocity",
            "l1_summary_centre",
            "l2_count",
            "l2_summary_push"
        ]
    );
    assert_eq!(
        boids.fault_names().get(folds[0].id),
        Some("lines 1 and 2, Flockmates and Too close")
    );
    // Each look keeps the birds within its own distance, since the fold walks the larger of the two.
    assert_eq!(nodes_of_kind(&boids, "cmp.le").count(), 2);
    // A reorder of the two looks leaves the code, since the accumulators follow the line ids.
    let swapped = edited(
        sheet_text("boids.toml"),
        "lines = [\"l1\", \"l2\", \"l3\"",
        "lines = [\"l2\", \"l1\", \"l3\"",
    );
    assert_eq!(lowered(&swapped).code(), boids.code());
    // One look keeps every agent its fold walks.
    let opinions = lowered(sheet_text("bounded_confidence.toml"));
    assert_eq!(nodes_of_kind(&opinions, "agent.radius_fold").count(), 1);
    let tick = opinions
        .lowered()
        .document()
        .passes
        .iter()
        .find(|pass| pass.stage == PassStage::Tick)
        .expect("a tick pass");
    assert!(
        tick.body.nodes.iter().all(|node| node.op != "math.max"),
        "one distance takes no largest"
    );
    let fold = nodes_of_kind(&opinions, "agent.radius_fold").next().expect("a fold");
    assert_eq!(
        fold.config.get("exclude_self"),
        Some(&ConfigValue::Bool(false)),
        "including me"
    );
    assert_eq!(opinions.fault_names().get(fold.id), Some("line 1, Like-minded"));
}

#[test]
fn the_radius_index_follows_the_current_largest_distance() {
    let boids = lowered(sheet_text("boids.toml"));
    // One source of `cell_size` for every fold of every pass (V14).
    let sources: Vec<&InputValue> = nodes_of_kind(&boids, "agent.radius_fold")
        .filter_map(|node| node.inputs.get("cell_size"))
        .collect();
    assert!(!sources.is_empty() && sources.iter().all(|source| *source == sources[0]));
    let tick_passes = boids
        .lowered()
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
        .count();
    assert_eq!(tick_passes, 1);
    let program = AgentProgram::compile(boids.lowered()).expect("boids compile");
    let entry = program.entry();
    let spec = program.spec();
    let at = |visual: f32, protected: f32| {
        params_of(
            &entry,
            &[
                ("visual_range", ParamValue::F32(visual)),
                ("protected_range", ParamValue::F32(protected)),
            ],
        )
    };
    let size = spec.hooks.index_cell_size(&at(50.0, 8.0));
    assert!((size - 50.0 / 3.0).abs() < 1e-5, "{size}");
    let size = spec.hooks.index_cell_size(&at(9.0, 30.0));
    assert!(
        (size - 10.0).abs() < 1e-5,
        "the larger distance sizes the index: {size}"
    );
}

/// Number of agents of the mobile sheet `text` whose Hit holds after each of `ticks` ticks on the host, from `agents`
/// agents with chance `chance` per infected person nearby, summed over the ticks.
fn host_hits(text: &str, agents: u32, chance: f32, ticks: u32) -> u64 {
    let file = lowered(text);
    let evaluator = host_of(&file);
    let own = [ParamValue::F32(chance), ParamValue::F32(5.0)];
    let world = HostWorld::Agents {
        count: agents,
        width: 1.0,
        height: 1.0,
    };
    let mut lanes: HostLanes = evaluator.init(world, &own, Some(7)).expect("the init runs");
    let mut hits = 0;
    let mut stream = 0;
    for tick in 0..ticks {
        evaluator
            .run_pass(
                "tick_person_tick",
                &mut lanes,
                &own,
                &RunContext::tick(Some(7), tick),
                &mut stream,
            )
            .expect("a tick runs");
        let hit = lanes.column("hit").expect("the hit lane");
        hits += (0..agents as usize)
            .filter(|&index| hit.get(index) == Some(HostValue::Bool(true)))
            .count() as u64;
    }
    hits
}

/// A chance once for each of 3 infected people nearby holds with `1 - (1 - 0.2)^3 = 0.488`, within five standard
/// errors over 4 agents and 5000 ticks, and a chance of 0 once for each of 1000 never holds. Every agent sees every
/// other in a world 1 cell wide.
#[test]
fn a_chance_once_for_each_agent_follows_the_power_rule_on_the_host() {
    let file = lowered(MOBILE);
    assert_eq!(
        nodes_of_kind(&file, "math.powi").count(),
        0,
        "a count of agents never takes math.powi"
    );
    assert_eq!(nodes_of_kind(&file, "math.pow").count(), 1);
    let samples = 4 * 5000;
    let share = host_hits(MOBILE, 4, 0.2, 5000) as f64 / f64::from(samples);
    let standard_error = (0.488 * 0.512 / f64::from(samples)).sqrt();
    assert!(
        (share - 0.488).abs() <= 5.0 * standard_error,
        "a chance of 0.2 for each of 3 held for {share} of the people"
    );
    assert_eq!(host_hits(MOBILE, 1001, 0.0, 3), 0, "a chance of 0 for each of 1000");
}

/// Returns a sheet of 3 walkers whose card holds the line tables `first` as `l1` and `second` as `l2`, over a Number
/// Level starting at 0.
fn levels_with(first: &str, second: &str) -> String {
    format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\nid = \"levels\"\n\
         name = \"Levels\"\nnext_line = 3\n\n[world]\nmade_of = \"agents\"\nwidth = 10\nheight = 10\nedges = \"wrap\"\n\n\
         [[agents]]\nid = \"walker\"\nname = \"Walker\"\ncount = 3\nmax_count = 3\n\n[[agents.property]]\n\
         id = \"position\"\nname = \"Position\"\ntype = \"position\"\n\n[agents.property.starts_as]\nrule = \"anywhere\"\n\
         key = 11\n\n[[agents.property]]\nid = \"level\"\nname = \"Level\"\ntype = \"number\"\n\n\
         [agents.property.starts_as]\nrule = \"value\"\nvalue = 0\n\n[[card]]\nid = \"walker_tick\"\nevery = \"walker\"\n\
         runs = \"each_tick\"\nlines = [\"l1\", \"l2\"]\n\n[card.line.l1]\n{first}\n\n[card.line.l2]\n{second}\n"
    )
}

/// A Keep between holds a property inside its limits after every Set of the property, above or below it in the card.
#[test]
fn a_keep_between_holds_the_value_after_every_set() {
    let keep = "kind = \"keep_between\"\nproperty = \"level\"\nminimum = 0\nmaximum = 1";
    let set = "kind = \"set\"\nproperty = \"level\"\nvalue = 5\ncondition = { kind = \"always\" }";
    for text in [levels_with(keep, set), levels_with(set, keep)] {
        let file = lowered(&text);
        let evaluator = host_of(&file);
        let world = HostWorld::Agents {
            count: 3,
            width: 10.0,
            height: 10.0,
        };
        let mut lanes = evaluator.init(world, &[], Some(7)).expect("the init runs");
        evaluator
            .run_pass(
                "tick_walker_tick",
                &mut lanes,
                &[],
                &RunContext::tick(Some(7), 0),
                &mut 0,
            )
            .expect("a tick runs");
        let level = lanes.column("level").expect("the level lane");
        for index in 0..3 {
            assert_eq!(level.get(index), Some(HostValue::F32(1.0)), "{text}");
        }
    }
}

/// A chance of 1 once for each of no infected person never holds, on the host and on the device. With no guard the
/// power `0^0` is not 1 on every device.
#[test]
fn for_each_of_zero_with_chance_1_is_0_on_host_and_device() {
    let healthy = edited(
        MOBILE,
        "value = { state = \"infected\" }",
        "value = { state = \"susceptible\" }",
    );
    assert_eq!(host_hits(&healthy, 500, 1.0, 4), 0, "on the host");
    let Some(device) = device_for("the device half of for_each_of_zero_with_chance_1_is_0_on_host_and_device") else {
        return;
    };
    let ctx = context(device);
    let file = lowered(&healthy);
    let program = agent_program_of(&file);
    let hit = program.lane_buffer("hit").expect("the hit lane");
    let entry = program.entry();
    let params = params_of(
        &entry,
        &[
            ("num_agents", ParamValue::U32(500)),
            ("world_width", ParamValue::F32(1.0)),
            ("world_height", ParamValue::F32(1.0)),
            ("infection_chance", ParamValue::F32(1.0)),
        ],
    );
    let mut state = built(&entry, &params, Some(7), &ctx);
    henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 3).expect("three ticks run");
    let words = state.read_buffer(hit.buffer()).expect("the hit lane reads back");
    let words = hit.segment(&words);
    assert_eq!(words.len(), 500);
    assert!(words.iter().all(|&word| word == 0), "on the device");
    // With every person infected, a chance of 1 for each of 499 holds for everyone.
    let sick = lowered(MOBILE);
    let entry = entry_of(&sick);
    let mut state = built(&entry, &params, Some(7), &ctx);
    henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 1).expect("a tick runs");
    let words = state.read_buffer(hit.buffer()).expect("the hit lane reads back");
    assert!(hit.segment(&words).iter().all(|&word| word == 1));
}

#[test]
fn each_agent_refusal_gives_one_located_error() {
    let one = |text: &str, message: &str, at: &str| {
        let error = only_refusal(text);
        assert_eq!(error.message, message);
        assert!(
            placed_at(&error, text).starts_with(at),
            "{} is placed at {:?}",
            error.message,
            placed_at(&error, text)
        );
    };
    let walls = edited(sheet_text("random_walk.toml"), "edges = \"wrap\"", "edges = \"walls\"");
    one(&walls, sentence::NOT_YET_AGENT_WALLS, "[world]");
    // A Move to below a Move by is refused at the Move to, and the pick that names its place lowers.
    let move_to = edited(
        &boids_with(
            "[card.line.l9]\nkind = \"pick_cell\"\nname = \"Spot\"\nkey = 5\npick = \"random\"\naround = \"cells_8\"\n\
             open_only = true\n\n[card.line.l10]\nkind = \"move\"\nmode = \"to\"\nvalue = { result = \"l9\" }",
        ),
        "\"l7\", \"l9\"]",
        "\"l7\", \"l9\", \"l10\"]",
    );
    let move_to = edited(&move_to, "next_line = 10", "next_line = 11");
    one(&move_to, &sentence::moved_by_already("Line 7"), "[card.line.l10]");
    let set_after_steer = boids_with(
        "[card.line.l9]\nkind = \"set\"\nproperty = \"velocity\"\nvalue = { my = \"velocity\" }\n\n\
         [card.line.l9.condition]\nkind = \"always\"",
    );
    one(
        &set_after_steer,
        &sentence::steered_already("Line 3", "Velocity"),
        "[card.line.l9]",
    );
    let record_in_repeat = edited(
        &edited(
            &edited(
                sheet_text("random_walk.toml"),
                "lines = [\"l2\"]",
                "lines = [\"l2\", \"l3\"]",
            ),
            "next_line = 3",
            "next_line = 4",
        ),
        "[[parameter]]",
        "[[event]]\nid = \"turn\"\nname = \"Turn\"\n\n[[parameter]]",
    );
    let record_in_repeat = format!(
        "{record_in_repeat}\n\n[card.line.l3]\nkind = \"record\"\nevent = \"turn\"\n\n[card.line.l3.condition]\n\
         kind = \"always\"\n"
    );
    one(&record_in_repeat, sentence::NOT_YET_REPEAT_CHANGE, "[card.line.l3]");
    // The checker refuses a look inside a Repeat and a second kind of agent, each at its table.
    let look_in_repeat = edited(
        &edited(
            sheet_text("random_walk.toml"),
            "lines = [\"l2\"]",
            "lines = [\"l2\", \"l3\"]",
        ),
        "next_line = 3",
        "next_line = 4",
    );
    let look_in_repeat = format!(
        "{look_in_repeat}\n\n[card.line.l3]\nkind = \"count\"\nname = \"Near\"\naround = \"agents_within\"\n\
         agent_kind = \"walker\"\ndistance = 5\ninclude_me = false\n\n[card.line.l3.whose]\nkind = \"always\"\n"
    );
    let error = only_refusal(&look_in_repeat);
    assert!(
        error.message.contains("cannot be used inside a Repeat"),
        "{}",
        error.message
    );
    assert!(placed_at(&error, &look_in_repeat).starts_with("[card.line.l3]"));
    let second = edited(
        sheet_text("random_walk.toml"),
        "[[parameter]]",
        "[[agents]]\nid = \"runner\"\nname = \"Runner\"\ncount = 1\nmax_count = 1\n\n[[agents.property]]\nid = \
         \"spot\"\nname = \"Spot\"\ntype = \"position\"\n\n[agents.property.starts_as]\nrule = \"anywhere\"\nkey = 9\n\n\
         [[parameter]]",
    );
    let errors = refusals(&second);
    assert!(
        errors.iter().any(|error| error.message == sentence::ONE_AGENT_KIND),
        "{errors:#?}"
    );
}

#[test]
fn the_shortfalls_of_a_sheet_name_its_looks_in_the_model_words() {
    let boids = lowered(sheet_text("boids.toml"));
    let entry = entry_of(&boids);
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: u64::MAX,
        max_buffer_size: u64::MAX,
        ..wgpu::Limits::default()
    };
    let params = params_of(
        &entry,
        &[
            ("num_agents", ParamValue::U32(1_000_000)),
            ("visual_range", ParamValue::F32(200.0)),
        ],
    );
    let raw = entry.shortfalls(&params, &limits);
    assert!(raw.iter().any(|line| line.contains("units of work")), "{raw:?}");
    let mapped = boids.shortfalls(&entry, &params, &limits);
    assert_eq!(
        mapped,
        [sentence::look_costs_too_much(
            "lines 1 and 2, Flockmates and Too close",
            "birds",
            1_000_000
        )]
    );
    for line in &mapped {
        for word in sentence::FORBIDDEN_WORDS {
            assert!(!line.contains(word), "{line} holds {word}");
        }
    }
    let defaults = params_of(&entry, &[]);
    assert!(boids.shortfalls(&entry, &defaults, &limits).is_empty());
}

#[test]
fn a_pass_cost_past_the_limit_names_its_line_in_the_model_words() {
    // A Repeat of at most 65536 Move by lines costs far more than one pass may per walker.
    let text = edited(
        sheet_text("random_walk.toml"),
        "max = 16\ndefault = 4",
        "max = 65536\ndefault = 4",
    );
    let file = lowered(&text);
    let report = file.lowered().check();
    let errors: Vec<_> = report.errors().cloned().collect();
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(errors[0].message.contains("units per element"), "{errors:#?}");
    let placed = file.place_graph_diagnostic(errors[0].clone());
    assert_eq!(
        placed.message,
        sentence::line_costs_too_much("line 1", "walker", &sentence::smaller_maximum("Steps per tick"))
    );
    assert!(placed.notes.is_empty(), "{:?}", placed.notes);
    assert_eq!((placed.pass.as_deref(), placed.node.as_deref()), (None, None));
    assert!(placed_at(&placed, &text).starts_with("[card.line.l1]"));
    for word in sentence::FORBIDDEN_WORDS {
        assert!(!placed.message.contains(word), "{} holds {word}", placed.message);
    }
    // Another finding keeps its words.
    let other = henad_core::diagnostic::CodeDiagnostic::error("the pass 'x' reads a lane it never writes");
    assert_eq!(file.place_graph_diagnostic(other.clone()).message, other.message);
}

#[test]
fn a_node_id_of_the_fold_is_a_node_the_sheet_owns() {
    let boids = lowered(sheet_text("boids.toml"));
    let (node, _) = boids.fault_names().iter().next().expect("a fold name");
    let origin = boids.line_map().origin(node).expect("the fold's origin");
    assert_eq!(origin.owner, NodeOwner::Sheet);
    assert_eq!(origin.path, "card.bird_tick");
    assert!(node < node_id(1000));
}

#[test]
fn a_card_editor_reads_each_lowering_refusal_with_its_card_and_line() {
    let set_after_steer = boids_with(
        "[card.line.l9]\nkind = \"set\"\nproperty = \"velocity\"\nvalue = { my = \"velocity\" }\n\n\
         [card.line.l9.condition]\nkind = \"always\"",
    );
    let move_to = boids_with(
        "[card.line.l9]\nkind = \"move\"\nmode = \"to\"\n\n[card.line.l9.value]\nkind = \"look_at_cell\"\n\
         which = \"here\"",
    );
    for (text, message) in [
        (&set_after_steer, sentence::steered_already("Line 3", "Velocity")),
        (&move_to, sentence::moved_by_already("Line 7")),
    ] {
        let parsed = crate::sheet::ParsedSheet::parse(text).expect("the sheet reads");
        let problems = SheetFile::lowering_problems(&parsed, &parsed.check());
        assert_eq!(problems.len(), 1, "{problems:?}");
        let problem = &problems[0];
        assert_eq!(
            (
                problem.card,
                problem.line.as_deref(),
                problem.diagnostic.message.as_str()
            ),
            (Some(0), Some("l9"), message.as_str())
        );
        assert!(!problem.skips_line);
    }
    let parsed = crate::sheet::ParsedSheet::parse(sheet_text("boids.toml")).expect("boids reads");
    assert!(
        SheetFile::lowering_problems(&parsed, &parsed.check()).is_empty(),
        "a sheet that lowers has no problem"
    );
}
