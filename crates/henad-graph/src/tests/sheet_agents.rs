//! The lowering of agent sheets: every agent starter checked and compiled, the folds of the looks at agents and their
//! index, the power rule of a chance once for each agent on the host and the device, the refusals of the lines this
//! version does not lower and of a property changed two ways, the names of the folds and the cost in the model's words.

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::FaultSink;
use henad_compute::gpu::{GpuContext, wgpu};
use henad_core::params::ParamValue;

use super::support::test_device;
use crate::document::{PassStage, World};
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::ids::NodeId;
use crate::sheet::{NodeOwner, SheetFile, SheetFileError, sentence, starters};
use crate::value::{ConfigValue, InputValue};
use crate::wgsl::AgentProgram;

/// Text of the test sheet of chances once for each infected person nearby.
const MOBILE: &str = include_str!("sheets/mobile_sir.toml");

/// Returns the agent starters with their file names.
fn agent_sheets() -> Vec<(&'static str, &'static str)> {
    starters()
        .iter()
        .filter(|starter| starter.text.contains("made_of = \"agents\""))
        .map(|starter| (starter.file_name, starter.text))
        .collect()
}

/// Returns the text of the agent starter `file_name`.
fn sheet(file_name: &str) -> &'static str {
    agent_sheets()
        .into_iter()
        .find(|(name, _)| *name == file_name)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no agent starter {file_name}"))
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

/// Returns the entry of `file`, compiled with its fault names.
fn entry_of(file: &SheetFile) -> ModelEntry {
    AgentProgram::compile_with_fault_names(file.lowered(), file.fault_names())
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry()
}

/// Returns the parameters of `entry` at their defaults, with each of `set` in place of its default.
fn params_of(entry: &ModelEntry, set: &[(&str, ParamValue)]) -> Vec<ParamValue> {
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            set.iter()
                .find(|(id, _)| *id == descriptor.id())
                .map_or_else(|| descriptor.kind().default_value(), |(_, value)| value.clone())
        })
        .collect()
}

/// Returns the ids of the nodes of every pass of `file` whose kind is `op`.
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
    assert_eq!(names, ["random_walk.toml", "boids.toml", "bounded_confidence.toml"]);
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
        let written = file
            .lowered()
            .document()
            .passes
            .iter()
            .flat_map(|pass| &pass.body.nodes)
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
    let boids = lowered(sheet("boids.toml"));
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
    let opinions = lowered(sheet("bounded_confidence.toml"));
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
    let boids = lowered(sheet("boids.toml"));
    let folds: Vec<_> = nodes_of_kind(&boids, "agent.radius_fold").collect();
    assert_eq!(folds.len(), 1, "Flockmates and Too close share one fold");
    let names: Vec<&str> = folds[0]
        .config
        .get("acc")
        .and_then(ConfigValue::as_array)
        .expect("accumulators")
        .iter()
        .filter_map(|table| match table {
            ConfigValue::Table(table) => table.get("name").and_then(ConfigValue::as_str),
            _ => None,
        })
        .collect();
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
        sheet("boids.toml"),
        "lines = [\"l1\", \"l2\", \"l3\"",
        "lines = [\"l2\", \"l1\", \"l3\"",
    );
    assert_eq!(lowered(&swapped).code(), boids.code());
    // One look keeps every agent its fold walks.
    let opinions = lowered(sheet("bounded_confidence.toml"));
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
    let boids = lowered(sheet("boids.toml"));
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

/// Number of agents of the mobile test sheet whose Hit holds after each of `ticks` ticks on the host, from `agents`
/// agents starting as `health` with chance `chance` per infected person nearby, summed over the ticks.
fn host_hits(text: &str, agents: u32, chance: f32, ticks: u32) -> u64 {
    let file = lowered(text);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
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

/// A chance of 1 once for each of no infected person never holds, on the host and on the device. With no guard the
/// power `0^0` is not 1 on every device.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn for_each_of_zero_with_chance_1_is_0_on_host_and_device() {
    let healthy = edited(
        MOBILE,
        "value = { state = \"infected\" }",
        "value = { state = \"susceptible\" }",
    );
    assert_eq!(host_hits(&healthy, 500, 1.0, 4), 0, "on the host");
    let Some(device) = test_device() else {
        eprintln!("skipping the device half of for_each_of_zero_with_chance_1_is_0_on_host_and_device: no adapter");
        return;
    };
    let ctx = GpuContext::new(
        device.device,
        device.queue,
        wgpu::TextureFormat::Rgba8Unorm,
        FaultSink::new(),
    );
    let file = lowered(&healthy);
    let entry = entry_of(&file);
    let params = params_of(
        &entry,
        &[
            ("num_agents", ParamValue::U32(500)),
            ("world_width", ParamValue::F32(1.0)),
            ("world_height", ParamValue::F32(1.0)),
            ("infection_chance", ParamValue::F32(1.0)),
        ],
    );
    let Ok(ModelState::Gpu(mut state)) = entry.build(&params, Some(7), Some(&ctx)) else {
        panic!("the mobile sheet builds on the device");
    };
    let henad_core::metadata::Structure::GpuAgents { buffers, .. } = &entry.metadata().structure else {
        panic!("a GPU agent model");
    };
    let hit = buffers
        .iter()
        .position(|buffer| buffer.label == "lane_hit_agents")
        .expect("the hit lane");
    henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 3).expect("three ticks run");
    let words = state.read_buffer(hit).expect("the hit lane reads back");
    assert_eq!(words.len(), 500);
    assert!(words.iter().all(|&word| word == 0), "on the device");
    // With every person infected, a chance of 1 for each of 499 holds for everyone.
    let sick = lowered(MOBILE);
    let entry = entry_of(&sick);
    let Ok(ModelState::Gpu(mut state)) = entry.build(&params, Some(7), Some(&ctx)) else {
        panic!("the mobile sheet builds on the device");
    };
    henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 1).expect("a tick runs");
    let words = state.read_buffer(hit).expect("the hit lane reads back");
    assert!(words.iter().all(|&word| word == 1));
}

/// Returns the boids with the line table `table` added as line `l9` of the card that runs each tick.
fn boids_with(table: &str) -> String {
    let text = edited(sheet("boids.toml"), "next_line = 9", "next_line = 10");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\", \"l9\"]",
    );
    edited(
        &text,
        "# Lines of Randomise headings",
        &format!("{table}\n\n# Lines of Randomise headings"),
    )
}

#[test]
fn each_agent_refusal_gives_one_located_error() {
    let one = |text: &str, message: &str, at: &str| {
        let errors = refusals(text);
        assert_eq!(errors.len(), 1, "{errors:#?}");
        assert_eq!(errors[0].message, message);
        assert!(
            placed_at(&errors[0], text).starts_with(at),
            "{} is placed at {:?}",
            errors[0].message,
            placed_at(&errors[0], text)
        );
    };
    let walls = edited(sheet("random_walk.toml"), "edges = \"wrap\"", "edges = \"walls\"");
    one(&walls, sentence::NOT_YET_AGENT_WALLS, "[world]");
    let pick = boids_with(
        "[card.line.l9]\nkind = \"pick_cell\"\nname = \"Spot\"\nkey = 5\npick = \"random\"\naround = \"cells_8\"\nopen_only = true",
    );
    one(&pick, sentence::NOT_YET_PICK_CELL, "[card.line.l9]");
    let record = edited(
        &boids_with("[card.line.l9]\nkind = \"record\"\nevent = \"turn\""),
        "[[parameter]]",
        "[[event]]\nid = \"turn\"\nname = \"Turn\"\n\n[[parameter]]",
    );
    one(&record, sentence::NOT_YET_RECORD, "[card.line.l9]");
    // A Move to needs a place, and only a Pick a cell gives one yet. Each line takes its own error.
    let move_to = edited(
        &boids_with(
            "[card.line.l9]\nkind = \"pick_cell\"\nname = \"Spot\"\nkey = 5\npick = \"random\"\naround = \"cells_8\"\n\
             open_only = true\n\n[card.line.l10]\nkind = \"move\"\nmode = \"to\"\nvalue = { result = \"l9\" }",
        ),
        "\"l7\", \"l9\"]",
        "\"l7\", \"l9\", \"l10\"]",
    );
    let move_to = edited(&move_to, "next_line = 10", "next_line = 11");
    let errors = refusals(&move_to);
    assert_eq!(errors.len(), 2, "{errors:#?}");
    assert_eq!(errors[0].message, sentence::NOT_YET_PICK_CELL);
    assert_eq!(errors[1].message, sentence::moved_by_already("Line 7"));
    assert!(placed_at(&errors[1], &move_to).starts_with("[card.line.l10]"));
    let set_after_steer = boids_with(
        "[card.line.l9]\nkind = \"set\"\nproperty = \"velocity\"\nvalue = { my = \"velocity\" }\n\n\
         [card.line.l9.condition]\nkind = \"always\"",
    );
    one(
        &set_after_steer,
        &sentence::steered_already("Line 3", "Velocity"),
        "[card.line.l9]",
    );
    let become_in_repeat = edited(
        &edited(
            sheet("bounded_confidence.toml"),
            "[[agents.property]]\nid = \"opinion\"",
            "[[agents.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  { id = \"calm\", name = \
             \"Calm\", color = \"#007AF5\" },\n  { id = \"cross\", name = \"Cross\", color = \"#E43748\" },\n]\n\n\
             [agents.property.starts_as]\nrule = \"value\"\nvalue = { state = \"calm\" }\n\n\
             [[agents.property]]\nid = \"opinion\"",
        ),
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\"]",
    );
    let become_in_repeat = edited(&become_in_repeat, "next_line = 4", "next_line = 6");
    let become_in_repeat = format!(
        "{become_in_repeat}\n\n[card.line.l4]\nkind = \"repeat\"\ntimes = 2\nlines = [\"l5\"]\n\n[card.line.l5]\n\
         kind = \"become\"\nproperty = \"mood\"\nnew_state = \"cross\"\nstart_state = \"calm\"\n\n\
         [card.line.l5.condition]\nkind = \"always\"\n"
    );
    one(&become_in_repeat, sentence::NOT_YET_REPEAT_CHANGE, "[card.line.l5]");
    // The checker refuses a look inside a Repeat and a second kind of agent, each at its table.
    let look_in_repeat = edited(
        &edited(
            sheet("random_walk.toml"),
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
    let errors = refusals(&look_in_repeat);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].message.contains("cannot sit inside a Repeat"),
        "{}",
        errors[0].message
    );
    assert!(placed_at(&errors[0], &look_in_repeat).starts_with("[card.line.l3]"));
    let second = edited(
        sheet("random_walk.toml"),
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
fn a_look_at_the_cells_around_an_agent_waits_for_the_ground() {
    let text = boids_with(
        "[card.line.l9]\nkind = \"count\"\nname = \"Near\"\naround = \"cells_8\"\n\n[card.line.l9.whose]\n\
         kind = \"compare\"\ntest = \"more_than\"\nleft = { my = \"velocity\" }\nright = 1",
    );
    let errors = refusals(&text);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].message, sentence::NOT_YET_GROUND);
    assert!(placed_at(&errors[0], &text).starts_with("[card.line.l9]"));
}

#[test]
fn the_shortfalls_of_a_sheet_name_its_looks_in_the_model_words() {
    let boids = lowered(sheet("boids.toml"));
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
        sheet("random_walk.toml"),
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
    let boids = lowered(sheet("boids.toml"));
    let (node, _) = boids.fault_names().iter().next().expect("a fold name");
    let origin = boids.line_map().origin(node).expect("the fold's origin");
    assert_eq!(origin.owner, NodeOwner::Sheet);
    assert_eq!(origin.path, "card.bird_tick");
    assert!(node < NodeId::new(1000).expect("an id"));
}
