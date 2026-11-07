//! Every agents graph of the corpus built and stepped on a device: the testing kit over their entries, the work
//! budgets at their largest population, a step of a Repeat on the device against the host evaluator, and the fault of
//! a radius fold past its visit cap.

use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::fault::FaultKind;
use henad_compute::gpu::stepping::sample_stats;
use henad_compute::gpu::{HOST_WORK_BUDGET, SUBMISSION_WORK_BUDGET};
use henad_core::params::ParamValue;
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};
use henad_graph::GraphFile;
use henad_graph::document::{PassStage, World};
use henad_graph::host::{HostEvaluator, HostWorld, RunContext};

use super::graph_grid_parity::{build_gpu, step};
use super::lanes::GraphLanes;

/// Returns every agents file of the corpus with its name: the agent starters, then the agents fixtures.
fn agent_corpus() -> Vec<(String, GraphFile)> {
    let starters = henad_graph::starters::agent_starters()
        .iter()
        .map(|starter| (starter.file_name.to_owned(), starter.text.to_owned()));
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graphs");
    let mut fixtures: Vec<(String, String)> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
            (name, text)
        })
        .collect();
    fixtures.sort();
    starters
        .chain(fixtures)
        .filter_map(|(name, text)| {
            let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
            matches!(file.document().world, World::Agents(_)).then_some((name, file))
        })
        .collect()
}

/// Returns the entry `file` builds.
fn entry_of(name: &str, file: &GraphFile) -> ModelEntry {
    ModelFile::parse(&file.to_toml())
        .and_then(|model_file| model_file.entry())
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// Every agents entry of the corpus meets the testing kit's checks, its GPU checks included where a device exists.
#[test]
fn every_agent_corpus_entry_conforms() {
    let mut models = ModelSet::new(henad_core::build_info!());
    for (name, file) in agent_corpus() {
        models
            .insert(entry_of(&name, &file))
            .unwrap_or_else(|error| panic!("{name}: the set refused the entry: {error}"));
    }
    let mut settings = CheckSettings::default();
    if let Some(device) = headless_test_device(&TestDeviceRequest::baseline()) {
        settings = settings.gpu(device);
    } else {
        log::warn!("checking the agents corpus without its GPU checks: no adapter");
    }
    assert_set_conforms(&models, &settings);
}

/// Every agents file of the corpus at its largest population, in its declared world, stays within the work budgets
/// of one submission and of the host, and its entry reports no shortfall on a device that holds its buffers.
#[test]
fn every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets() {
    for (name, file) in agent_corpus() {
        let World::Agents(world) = file.document().world else {
            unreachable!("the corpus holds agents files alone");
        };
        let entry = entry_of(&name, &file);
        let mut params: Vec<ParamValue> = entry
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind().default_value())
            .collect();
        params[0] = ParamValue::U32(world.max_count);
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: u64::MAX,
            max_buffer_size: u64::MAX,
            max_storage_buffers_per_shader_stage: 16,
            ..wgpu::Limits::default()
        };
        let demand = entry.demand(&params, &limits).expect("a GPU entry has a demand");
        let submission = demand.submission_work();
        assert!(
            submission > 0 && submission <= SUBMISSION_WORK_BUDGET,
            "{name}: {submission} units in one submission"
        );
        for run in &demand.host {
            assert!(run.work <= HOST_WORK_BUDGET, "{name}: {} units on the host", run.work);
        }
        let found = entry.shortfalls(&params, &limits);
        assert!(found.is_empty(), "{name}: {found:?}");
    }
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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping a_repeat_over_agents_steps_on_the_device_as_on_the_host: no adapter");
        return;
    };
    let starter = henad_graph::starters::starter("random_walk.toml").expect("the random walk");
    let file = GraphFile::parse(starter.text).expect("the random walk reads");
    let entry = entry_of("random_walk.toml", &file);
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

/// A radius fold whose walk meets more than 65536 candidates sets its fault, which ends the run at the next stats read
/// with a message naming its pass, its node and the line of the node's table, and the device runs another model
/// afterwards.
#[test]
fn a_radius_fold_past_its_visit_cap_faults_and_the_device_survives() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping a_radius_fold_past_its_visit_cap_faults_and_the_device_survives: no adapter");
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
    let entry = ModelFile::parse(text)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{error}"));
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
    let header = text.find("[[pass.node]]\nid = \"n4\"").expect("the fold's table");
    let line = 1 + text[..header].matches('\n').count();
    assert!(
        message.contains(&format!("the pass 'crowd', node n4 on line {line}:")) && message.contains("65536"),
        "{message}"
    );
    drop(state);
    let models = crate::example_models();
    let boids = models.get("gpu_boids").expect("the stock model");
    let boids_params = boids
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind().default_value())
        .collect::<Vec<_>>();
    let mut other = build_gpu(boids, &boids_params, None, &ctx);
    step(&ctx, &mut *other, 2);
    sample_stats(&mut *other, &ctx).expect("the device runs another model after the fault");
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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping a_field_update_reads_each_layer_at_its_own_cell: no adapter");
        return;
    };
    let entry = ModelFile::parse(walled_trail!("walls"))
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{error}"));
    let params = [ParamValue::U32(1), ParamValue::F32(4.0), ParamValue::F32(2.0)];
    let mut state = build_gpu(&entry, &params, None, &ctx);
    step(&ctx, &mut *state, 1);
    let trail: Vec<f32> = super::graph_ants_parity::graph_buffer(&entry, &*state, "field_trail_cells")
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
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping a_radius_fold_over_the_whole_world_is_refused_or_faults_with_its_place: no adapter");
        return;
    };
    let entry_of_text = |text: &str| {
        ModelFile::parse(text)
            .and_then(|file| file.entry())
            .unwrap_or_else(|error| panic!("{error}"))
    };
    let world = |agents: u32| {
        [
            ParamValue::U32(agents),
            ParamValue::F32(2048.0),
            ParamValue::F32(2048.0),
        ]
    };
    let limits = ctx.device.limits();
    let coarse = entry_of_text(&WORLD_WALK.replace("cell_size = { f32 = 1.0 }", "cell_size = { f32 = 64.0 }"));
    let found = coarse.shortfalls(&world(60_000), &limits);
    assert!(
        found.iter().any(|line| line.contains("node_sparse_crowd")),
        "the budget admits a walk of the world at 60000 agents: {found:?}"
    );
    assert!(coarse.shortfalls(&world(100), &limits).is_empty());

    let entry = entry_of_text(WORLD_WALK);
    assert!(entry.shortfalls(&world(60_000), &limits).is_empty());
    let mut state = build_gpu(&entry, &world(60_000), None, &ctx);
    let fault = henad_compute::gpu::stepping::run_steps(&mut *state, &ctx, 1).expect_err("the walk faults");
    let header = WORLD_WALK.find("[[pass.node]]\nid = \"n4\"").expect("the fold's table");
    let line = 1 + WORLD_WALK[..header].matches('\n').count();
    let message = fault.to_string();
    assert!(
        message.contains(&format!("the pass 'crowd', node n4 on line {line}:"))
            && message.contains("walk more than 65536 cells"),
        "{message}"
    );
    drop(state);
    let models = crate::example_models();
    let boids = models.get("gpu_boids").expect("the stock model");
    let boids_params = boids
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind().default_value())
        .collect::<Vec<_>>();
    let mut other = build_gpu(boids, &boids_params, None, &ctx);
    step(&ctx, &mut *other, 2);
    sample_stats(&mut *other, &ctx).expect("the device runs another model after the fault");
}
