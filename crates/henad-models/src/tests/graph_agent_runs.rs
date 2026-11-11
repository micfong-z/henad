//! Every agents graph of the corpus built and stepped on a device: the testing kit over their entries, the work
//! budgets of the graphs and the agent sheet starters at their largest population in their largest world, the largest
//! population each admits in its declared world, a step of a Repeat on the device against the host evaluator, and the
//! fault of a radius fold past its visit cap.

use henad_compute::entry::ModelEntry;
use henad_compute::fault::FaultKind;
use henad_compute::gpu::stepping::sample_stats;
use henad_compute::gpu::{HOST_WORK_BUDGET, PassKind, SUBMISSION_WORK_BUDGET};
use henad_core::params::{ParamKind, ParamValue};
use henad_explore::model_file::ModelFile;
use henad_graph::GraphFile;
use henad_graph::document::PassStage;
use henad_graph::host::{HostEvaluator, HostWorld, RunContext};

use super::lanes::GraphLanes;
use super::support::{
    agent_corpus, agent_sheets, assert_device_runs, assert_entries_conform, baseline_device, build_gpu, graph_buffer,
    model_entry, roomy_limits, step,
};

/// Every agents entry of the corpus meets the testing kit's checks, its GPU checks included where a device exists.
#[test]
fn every_agent_corpus_entry_conforms() {
    assert_entries_conform(
        agent_corpus()
            .into_iter()
            .map(|(name, file)| (name, model_entry(&file.to_toml()))),
    );
}

/// Returns every agents file of the corpus, then every agent sheet starter, each with its directory and file name and
/// the entry it builds through a [`ModelFile`].
fn budget_corpus() -> Vec<(String, ModelEntry)> {
    let starters: Vec<&str> = henad_graph::starters::agent_starters()
        .iter()
        .map(|starter| starter.file_name)
        .collect();
    let graphs = agent_corpus().into_iter().map(|(name, file)| {
        let entry = model_entry(&file.to_toml());
        let directory = if starters.contains(&name.as_str()) {
            "starters"
        } else {
            "graphs"
        };
        (format!("{directory}/{name}"), entry)
    });
    let sheets = agent_sheets()
        .into_iter()
        .map(|(name, text)| (format!("sheets/{name}"), model_entry(text)));
    graphs.chain(sheets).collect()
}

/// World a budget test runs a file in.
#[derive(Clone, Copy, Debug)]
enum BudgetWorld {
    /// The world the file declares, its width and height parameters at their defaults.
    Declared,
    /// The largest world the file's parameters allow, its width and height parameters at their maxima.
    Largest,
}

/// Returns the declared and the largest population of `entry`: the default and the maximum of its `num_agents`.
fn population_range(name: &str, entry: &ModelEntry) -> (u32, u32) {
    let descriptor = entry
        .param_descriptors()
        .iter()
        .find(|descriptor| descriptor.id() == "num_agents")
        .unwrap_or_else(|| panic!("{name} has no num_agents parameter"));
    match descriptor.kind() {
        ParamKind::U32 { default, max, .. } => (*default, *max),
        kind => panic!("{name}: num_agents is {kind:?}"),
    }
}

/// Returns the parameters of `entry` at their defaults, with `num_agents` at `population` and the world width and
/// height as `world` sets them.
fn budget_params(name: &str, entry: &ModelEntry, population: u32, world: BudgetWorld) -> Vec<ParamValue> {
    let mut sides = 0;
    let params = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| match (descriptor.id(), descriptor.kind(), world) {
            ("num_agents", _, _) => ParamValue::U32(population),
            ("world_width" | "world_height", ParamKind::F32 { max, .. }, BudgetWorld::Largest) => {
                sides += 1;
                ParamValue::F32(*max)
            }
            (_, kind, _) => kind.default_value(),
        })
        .collect();
    if matches!(world, BudgetWorld::Largest) {
        assert_eq!(
            sides, 2,
            "{name} has no world_width and world_height parameters of type f32"
        );
    }
    params
}

/// Agents files whose init on the host passes [`HOST_WORK_BUDGET`] in the largest world their parameters allow, at
/// every population. Each lays a ground of one cell per unit of the world, and its init runs over every cell.
const GROUND_INIT_PAST_THE_HOST_BUDGET: [&str; 3] = ["starters/ants.toml", "graphs/gpu_ants.toml", "sheets/ants.toml"];

/// Every agents file of the corpus and every agent sheet starter at its largest population, in the largest world its
/// parameters allow, stays within the work budget of one submission. A population bound that no world reaches would
/// be a slider whose upper end is always refused.
///
/// Every file but those of [`GROUND_INIT_PAST_THE_HOST_BUDGET`] also stays within the host budget there, and its entry
/// reports no shortfall on a device that holds its buffers. Each of those reports the host budget's refusal of its
/// init alone, the same at its declared population as at its largest.
#[test]
fn every_agent_file_at_its_largest_population_in_its_largest_world_fits_the_work_budgets() {
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: 16,
        ..roomy_limits()
    };
    let mut ground_refused = Vec::new();
    for (name, entry) in budget_corpus() {
        let (declared, largest) = population_range(&name, &entry);
        let params = budget_params(&name, &entry, largest, BudgetWorld::Largest);
        let demand = entry.demand(&params, &limits).expect("a GPU entry has a demand");
        let submission = demand.submission_work();
        assert!(
            submission > 0 && submission <= SUBMISSION_WORK_BUDGET,
            "{name}: {submission} units in one submission"
        );
        let found = entry.shortfalls(&params, &limits);
        if !GROUND_INIT_PAST_THE_HOST_BUDGET.contains(&name.as_str()) {
            for run in &demand.host {
                assert!(run.work <= HOST_WORK_BUDGET, "{name}: {} units on the host", run.work);
            }
            assert!(found.is_empty(), "{name}: {found:?}");
            continue;
        }
        let [line] = found.as_slice() else {
            panic!("{name}: {found:?}");
        };
        let budget = format!(
            "units of work on the host, past the budget of {}",
            grouped(HOST_WORK_BUDGET)
        );
        assert!(
            line.contains("_init' would run ") && line.ends_with(&budget),
            "{name}: {line}"
        );
        let at_declared = entry.shortfalls(&budget_params(&name, &entry, declared, BudgetWorld::Largest), &limits);
        assert_eq!(at_declared, found, "{name} at {declared}");
        ground_refused.push(name);
    }
    assert_eq!(ground_refused, GROUND_INIT_PAST_THE_HOST_BUDGET);
}

/// Every agents file of the corpus and every agent sheet starter, in its declared world with every other parameter at
/// its default, is admitted at its declared population, and the largest population it admits equals its pin. A file
/// admitted at its `max_count` pins that. One agent past a smaller pin is refused by the submission budget alone, and
/// the refusal names the heaviest step pass.
///
/// The largest population is found by bisection over [`ModelEntry::shortfalls`], which runs no device. A change of a
/// pass's charge moves a pin.
#[test]
fn every_agent_file_in_its_declared_world_admits_its_pinned_largest_population() {
    const LARGEST_ADMITTED: [(&str, u32); 9] = [
        ("starters/boids.toml", 201_012),
        ("starters/ants.toml", 5_000_000),
        ("starters/random_walk.toml", 1_000_000),
        ("graphs/gpu_ants.toml", 5_000_000),
        ("graphs/gpu_boids.toml", 195_355),
        ("sheets/random_walk.toml", 1_000_000),
        ("sheets/boids.toml", 201_012),
        ("sheets/bounded_confidence.toml", 552_857),
        ("sheets/ants.toml", 5_000_000),
    ];
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: 16,
        ..roomy_limits()
    };
    let corpus = budget_corpus();
    let found: Vec<(&str, u32)> = corpus
        .iter()
        .map(|(name, entry)| {
            let admits =
                |population| entry.shortfalls(&budget_params(name, entry, population, BudgetWorld::Declared), &limits);
            let (declared, largest) = population_range(name, entry);
            let at_declared = admits(declared);
            assert!(at_declared.is_empty(), "{name} at {declared}: {at_declared:?}");
            if admits(largest).is_empty() {
                return (name.as_str(), largest);
            }
            // `low` is admitted and `high` refused throughout.
            let (mut low, mut high) = (declared, largest);
            while high - low > 1 {
                let middle = low + (high - low) / 2;
                if admits(middle).is_empty() {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            (name.as_str(), low)
        })
        .collect();
    assert_eq!(found, LARGEST_ADMITTED);
    for ((name, entry), (_, pinned)) in corpus.iter().zip(LARGEST_ADMITTED) {
        let (_, largest) = population_range(name, entry);
        if pinned == largest {
            continue;
        }
        let params = budget_params(name, entry, pinned + 1, BudgetWorld::Declared);
        let demand = entry.demand(&params, &limits).expect("a GPU entry has a demand");
        let heaviest = demand
            .passes
            .iter()
            .rev()
            .filter(|pass| pass.kind == PassKind::Step)
            .max_by_key(|pass| pass.work)
            .unwrap_or_else(|| panic!("{name} has a step pass"));
        let refused = entry.shortfalls(&params, &limits);
        let [line] = refused.as_slice() else {
            panic!("{name} at {}: {refused:?}", pinned + 1);
        };
        let budget = format!("past the budget of {}", grouped(SUBMISSION_WORK_BUDGET));
        let pass = format!(
            "and pass '{}' is the heaviest at {} per step",
            heaviest.label,
            grouped(heaviest.work)
        );
        assert!(
            line.starts_with("one submission would run ") && line.ends_with(&format!("{budget}, {pass}")),
            "{name} at {}: {line}",
            pinned + 1
        );
    }
}

/// Returns `units` in decimal digits with a comma between each group of three, as [`Demand::shortfalls`] writes them.
///
/// [`Demand::shortfalls`]: henad_compute::gpu::Demand::shortfalls
fn grouped(units: u64) -> String {
    let digits = units.to_string();
    let mut text = String::new();
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (digits.len() - position).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// Largest distance in world units between a float of a lane on the device and on the host, the shorter way round the
/// torus for a position.
const TOLERANCE: f32 = 1e-4;

/// A tick pass of `random_walk.toml`, whose Repeat carries each agent's position through its random steps, leaves the
/// lanes on the device within [`TOLERANCE`] of those the host evaluator leaves, at two seeds and two step counts, over
/// three ticks.
///
/// Note that the floats need not agree bit for bit. A device may round `cos`, `sin` and contracted products otherwise
/// than the host.
#[test]
fn a_repeat_over_agents_steps_on_the_device_as_on_the_host() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let starter = henad_graph::starters::starter("random_walk.toml").expect("the random walk");
    let file = GraphFile::parse(starter.text).expect("the random walk reads");
    let entry = model_entry(starter.text);
    let device_lanes = GraphLanes::of(&file);
    let evaluator = HostEvaluator::new(&file).expect("the random walk compiles on the host");
    let tick = file
        .document()
        .passes
        .iter()
        .find(|pass| pass.stage == PassStage::Tick)
        .expect("a tick pass")
        .id
        .clone();
    for (sub_steps, seed) in [(4, Some(5)), (9, None)] {
        let own = [ParamValue::U32(sub_steps), ParamValue::F32(2.0)];
        let params = [
            ParamValue::U32(777),
            ParamValue::F32(300.0),
            ParamValue::F32(200.0),
            own[0].clone(),
            own[1].clone(),
        ];
        let mut state = build_gpu(&entry, &params, seed, &ctx);
        let world = HostWorld::Agents {
            count: 777,
            width: 300.0,
            height: 200.0,
        };
        let mut lanes = evaluator.init(world, &own, seed).expect("the init runs");
        for ticks in 0..3u32 {
            for column in lanes.columns() {
                let device = device_lanes.read(&*state, column.id());
                assert_eq!(device.len(), column.words().len(), "{}", column.id());
                let sides = [300.0f32, 200.0];
                for (k, (&found, &expected)) in device.iter().zip(column.words()).enumerate() {
                    let distance = (f32::from_bits(found) - f32::from_bits(expected)).abs();
                    let distance = if column.id() == "pos" {
                        distance.min(sides[k % 2] - distance)
                    } else {
                        distance
                    };
                    assert!(
                        distance <= TOLERANCE,
                        "the word {k} of the lane {} is {} on the device and {} on the host at tick {ticks}, with \
                         {sub_steps} steps and seed {seed:?}",
                        column.id(),
                        f32::from_bits(found),
                        f32::from_bits(expected)
                    );
                }
            }
            let mut stream = 0;
            evaluator
                .run_pass(&tick, &mut lanes, &own, &RunContext::tick(seed, ticks), &mut stream)
                .expect("the tick runs on the host");
            step(&ctx, &mut *state, 1);
        }
    }
}

/// Returns the line, counted from 1, of the table of the pass node `node_id` in the graph file `text`.
fn node_line(text: &str, node_id: &str) -> usize {
    let header = text
        .find(&format!("[[pass.node]]\nid = \"{node_id}\""))
        .unwrap_or_else(|| panic!("no table of node {node_id}"));
    1 + text[..header].matches('\n').count()
}

/// A radius fold whose walk meets more than 65536 candidates sets its fault, which ends the run at the next stats read
/// with a message naming its pass, its node and the line of the node's table, and the device runs another model
/// afterwards.
#[test]
fn a_radius_fold_past_its_visit_cap_faults_and_the_device_survives() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    // Every agent sits at the origin, in one cell of the index, and agent 0 alone walks. The world is wide, so the
    // submission budget, which charges every agent a walk over a uniform spread, admits the file.
    let text = "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"node_crowd\"\n\n[graph]\n\
                topology = \"agents\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[agents]\ncount = 70000\n\
                max_count = 70000\nwidth = 1000.0\nheight = 1000.0\n\n[[lane]]\nid = \"pos\"\ndomain = \"agents\"\n\
                type = \"vec2f\"\nrole = \"position\"\n\n[[lane]]\nid = \"seen\"\ndomain = \"agents\"\ntype = \"u32\"\n\n\
                [[pass]]\nid = \"crowd\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n\n[[pass.node]]\n\
                id = \"n1\"\nop = \"lane.get\"\nconfig.lane = \"pos\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"ctx.index\"\n\n\
                [[pass.node]]\nid = \"n3\"\nop = \"cmp.eq\"\nin.a = \"n2\"\nin.b = { u32 = 0 }\n\n[[pass.node]]\n\
                id = \"n4\"\nop = \"agent.radius_fold\"\nconfig.acc = [{ name = \"seen\", type = \"u32\", combine = \
                \"count\" }]\nin.center = \"n1\"\nin.radius = { f32 = 5.0 }\nin.cell_size = { f32 = 10.0 }\n\
                in.when = \"n3\"\n\n[[pass.node]]\nid = \"n5\"\nop = \"region.put\"\nregion = \"n4\"\n\
                config.acc = \"seen\"\n\n[[pass.node]]\nid = \"n6\"\nop = \"lane.set\"\nconfig.lane = \"seen\"\n\
                in.value = \"n4.seen\"\n";
    let entry = model_entry(text);
    let params = [
        ParamValue::U32(70_000),
        ParamValue::F32(1000.0),
        ParamValue::F32(1000.0),
    ];
    // A run that reads no stats, as a benchmark runs, ends with the fault too.
    let mut unsampled = build_gpu(&entry, &params, None, &ctx);
    let fault = henad_compute::gpu::stepping::run_steps(&mut *unsampled, &ctx, 2).expect_err("the fold faults");
    assert!(fault.to_string().contains("node n4 on line"), "{fault}");
    drop(unsampled);
    let mut state = build_gpu(&entry, &params, None, &ctx);
    step(&ctx, &mut *state, 1);
    let fault = sample_stats(&mut *state, &ctx).expect_err("the fold faults");
    let FaultKind::Refused(message) = &fault.kind else {
        panic!("{fault}");
    };
    let line = node_line(text, "n4");
    assert!(
        message.contains(&format!("the pass 'crowd', node n4 on line {line}:")) && message.contains("65536"),
        "{message}"
    );
    drop(state);
    let models = crate::example_models();
    assert_device_runs(&ctx, models.get("gpu_boids").expect("the stock model"));
}

/// A graph of one agent in a world of 4 by 2 cells, whose field `trail` of two layers starts at 1 and whose `finish`
/// cuts it to 0 where the static field `walls` is set, which the init does at cell 5. `$finish_reads` is the field the
/// `finish` reads.
macro_rules! walled_trail {
    ($finish_reads:literal) => {
        concat!(
            "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"node_walled\"\n\n[graph]\n\
             topology = \"agents\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[agents]\ncount = 1\nwidth = 4.0\n\
             height = 2.0\n\n[[lane]]\nid = \"pos\"\ndomain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\n\
             [[field]]\nid = \"trail\"\ntype = \"f32\"\nlayers = [\"a\", \"b\"]\nstatic = false\ncombine = \"max\"\n\
             finish = \"cut\"\ninitial = { f32 = 1.0 }\n\n[[field]]\nid = \"walls\"\ntype = \"f32\"\n\
             layers = [\"value\"]\nstatic = true\ninitial = { f32 = 0.0 }\n\n[[function]]\nid = \"cut\"\n\
             kind = \"pure\"\n\n[[function.input]]\nport = \"value\"\ntype = \"f32\"\n\n[[function.output]]\n\
             port = \"value\"\ntype = \"f32\"\n\n[[function.node]]\nid = \"n1\"\nop = \"fn.input\"\n\
             config.port = \"value\"\n\n[[function.node]]\nid = \"n2\"\nop = \"field.get\"\nconfig.field = \"",
            $finish_reads,
            "\"\n\n[[function.node]]\nid = \"n3\"\nop = \"cmp.gt\"\nin.a = \"n2\"\nin.b = { f32 = 0.5 }\n\n\
             [[function.node]]\nid = \"n4\"\nop = \"logic.select\"\nin.cond = \"n3\"\nin.if_true = { f32 = 0.0 }\n\
             in.if_false = \"n1\"\n\n[[function.node]]\nid = \"n5\"\nop = \"fn.output\"\nconfig.port = \"value\"\n\
             in.value = \"n4\"\n\n[[pass]]\nid = \"build\"\nstage = \"init\"\nkind = \"map\"\ndomain = \"cells\"\n\n\
             [[pass.node]]\nid = \"n10\"\nop = \"ctx.index\"\n\n[[pass.node]]\nid = \"n11\"\nop = \"cmp.eq\"\n\
             in.a = \"n10\"\nin.b = { u32 = 5 }\n\n[[pass.node]]\nid = \"n12\"\nop = \"logic.select\"\n\
             in.cond = \"n11\"\nin.if_true = { f32 = 1.0 }\nin.if_false = { f32 = 0.0 }\n\n[[pass.node]]\n\
             id = \"n13\"\nop = \"field.set\"\nconfig.field = \"walls\"\nin.value = \"n12\"\n\n[[pass]]\n\
             id = \"stay\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n\n[[pass.node]]\nid = \"n20\"\n\
             op = \"lane.get\"\nconfig.lane = \"pos\"\n\n[[pass.node]]\nid = \"n21\"\nop = \"lane.set\"\n\
             config.lane = \"pos\"\nin.value = \"n20\"\n"
        )
    };
}

/// A field's `finish` runs once per cell of each layer, and every layer reads the other fields at its own cell: a wall
/// at cell 5 cuts both layers there and no other cell. A `finish` that reads the field it updates is refused, since
/// the update writes that field in place.
#[test]
fn a_field_update_reads_each_layer_at_its_own_cell() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let entry = model_entry(walled_trail!("walls"));
    let params = [ParamValue::U32(1), ParamValue::F32(4.0), ParamValue::F32(2.0)];
    let mut state = build_gpu(&entry, &params, None, &ctx);
    step(&ctx, &mut *state, 1);
    let trail: Vec<f32> = graph_buffer(&entry, &*state, "field_trail_cells")
        .into_iter()
        .map(f32::from_bits)
        .collect();
    let expected: Vec<f32> = (0..16).map(|slot| if slot % 8 == 5 { 0.0 } else { 1.0 }).collect();
    assert_eq!(trail, expected, "layer a, then layer b");

    let error = ModelFile::parse(walled_trail!("trail"))
        .and_then(|file| file.entry())
        .expect_err("a finish that reads its own field is refused");
    assert!(
        error
            .to_string()
            .contains("reads the field 'trail' in the update that runs its `finish` 'cut'"),
        "{error}"
    );
}

/// A graph whose radius fold of radius 3000 walks an index of cells 1 wide, which covers a world of 2048 by 2048.
const WORLD_WALK: &str = "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"node_sparse\"\n\n\
                          [graph]\ntopology = \"agents\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[agents]\n\
                          count = 100\nmax_count = 100000\nwidth = 2048.0\nheight = 2048.0\n\n[[lane]]\nid = \"pos\"\n\
                          domain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\n[[lane]]\nid = \"seen\"\n\
                          domain = \"agents\"\ntype = \"u32\"\n\n[[pass]]\nid = \"crowd\"\nstage = \"tick\"\n\
                          kind = \"map\"\ndomain = \"agents\"\n\n[[pass.node]]\nid = \"n1\"\nop = \"lane.get\"\n\
                          config.lane = \"pos\"\n\n[[pass.node]]\nid = \"n4\"\nop = \"agent.radius_fold\"\n\
                          config.acc = [{ name = \"seen\", type = \"u32\", combine = \"count\" }]\n\
                          in.center = \"n1\"\nin.radius = { f32 = 3000.0 }\nin.cell_size = { f32 = 1.0 }\n\n\
                          [[pass.node]]\nid = \"n5\"\nop = \"region.put\"\nregion = \"n4\"\nconfig.acc = \"seen\"\n\n\
                          [[pass.node]]\nid = \"n6\"\nop = \"lane.set\"\nconfig.lane = \"seen\"\n\
                          in.value = \"n4.seen\"\n";

/// A radius fold whose walk covers the world is charged for the cells and agents it walks, and one whose walk passes
/// the cell cap faults before it starts. Over cells 64 wide the submission budget refuses the walk at 60000 agents,
/// naming its pass, and admits it at 100. Over cells 1 wide every walk covers more than 65536 cells, and at 60000 agents
/// the first step faults with its pass, node and line. The device runs another model afterwards.
#[test]
fn a_radius_fold_over_the_whole_world_is_refused_or_faults_with_its_place() {
    let Some(ctx) = baseline_device() else {
        return;
    };
    let world = |agents: u32| {
        [
            ParamValue::U32(agents),
            ParamValue::F32(2048.0),
            ParamValue::F32(2048.0),
        ]
    };
    let limits = ctx.device.limits();
    let coarse = model_entry(&WORLD_WALK.replace("cell_size = { f32 = 1.0 }", "cell_size = { f32 = 64.0 }"));
    let found = coarse.shortfalls(&world(60_000), &limits);
    assert!(
        found.iter().any(|line| line.contains("node_sparse_crowd")),
        "the budget admits a walk of the world at 60000 agents: {found:?}"
    );
    assert!(coarse.shortfalls(&world(100), &limits).is_empty());

    let entry = model_entry(WORLD_WALK);
    assert!(entry.shortfalls(&world(60_000), &limits).is_empty());
    let mut state = build_gpu(&entry, &world(60_000), None, &ctx);
    let fault = henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 1).expect_err("the walk faults");
    let line = node_line(WORLD_WALK, "n4");
    let message = fault.to_string();
    assert!(
        message.contains(&format!("the pass 'crowd', node n4 on line {line}:"))
            && message.contains("walk more than 65536 cells"),
        "{message}"
    );
    drop(state);
    let models = crate::example_models();
    assert_device_runs(&ctx, models.get("gpu_boids").expect("the stock model"));
}
