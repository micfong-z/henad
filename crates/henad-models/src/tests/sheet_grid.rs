//! Grid sheets on the device: the sheet starters of a grid world and round 1's chance SIR, lowered and run by the GPU
//! engine.
//!
//! The sheets are the grid sheets of `henad_graph::sheet::starters()` and `tests/fixtures/sheets/sir_chance.toml`, a
//! copy of henad-graph's test sheet of round 1's SIR with chances per tick. Each is held to the host evaluator, to
//! known answers and to the hand-lowered references in `tests/fixtures/lowered/round1/`. No sheet start and no app tool
//! paints cells, so a test that needs given tick-0 lanes plants them through the entry's hooks.
//!
//! Game of Life and Vote draw nothing on their cards. From shared tick-0 lanes they equal their hand-lowered files
//! every tick. The forest fire and the chance SIR draw with other keys than their hand-lowered files, and are held to
//! bands taken over the same seeds, with a planted rule error behind each band.
//!
//! The fire counts of the change lines are held to a planted blinker's known answer and, for the forest fire, to the
//! host evaluator's counts every tick.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use henad_compute::entry::{EntryDeclarations, ModelEntry, ModelSet};
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::{GpuContext, GridHooks, GridPassCtx, GridPassId, RerunUploads};
use henad_compute::snapshot::FireCounts;
use henad_core::action::action_seed;
use henad_core::params::ParamValue;
use henad_core::topology::TopologyHint;
use henad_core::view::StatValue;
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};
use henad_graph::GraphFile;
use henad_graph::document::{PassEnabled, PassStage, World};
use henad_graph::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use henad_graph::sheet::SheetFile;
use henad_graph::wgsl::{GridProgram, Probes};

use super::graph_grid_parity::{build_gpu, press, snapshot, stat, step};

/// Returns the text of `tests/fixtures/<path>`.
pub(super) fn fixture_text(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Returns the name and text of every grid sheet: the sheet starters of a grid world, then the chance SIR.
fn grid_sheets() -> Vec<(String, String)> {
    let mut sheets: Vec<(String, String)> = henad_graph::sheet::starters()
        .iter()
        .filter(|starter| matches!(sheet_file(starter.text).lowered().document().world, World::Grid(_)))
        .map(|starter| (starter.file_name.to_owned(), starter.text.to_owned()))
        .collect();
    sheets.push(("sir_chance.toml".to_owned(), fixture_text("sheets/sir_chance.toml")));
    sheets
}

/// Returns the text of the grid sheet `name`.
fn sheet_text(name: &str) -> String {
    grid_sheets()
        .into_iter()
        .find(|(file_name, _)| file_name == name)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no grid sheet {name}"))
}

/// Returns the sheet file of `text`, read, checked and lowered.
fn sheet_file(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

/// Returns the hand-lowered reference `tests/fixtures/lowered/round1/<name>`.
fn hand_lowered(name: &str) -> GraphFile {
    GraphFile::parse(&fixture_text(&format!("lowered/round1/{name}"))).unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// Returns the entry the model file of `text` builds.
pub(super) fn entry_of(text: &str) -> ModelEntry {
    ModelFile::parse(text)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Returns the build parameters of `entry` at `side` by `side` cells: the grid size, then each parameter at its
/// default unless `overrides` names it.
///
/// Panics if an id of `overrides` names no parameter of `entry`.
pub(super) fn device_params(entry: &ModelEntry, side: u32, overrides: &[(&str, f32)]) -> Vec<ParamValue> {
    for (id, _) in overrides {
        assert!(
            entry
                .param_descriptors()
                .iter()
                .any(|descriptor| descriptor.id() == *id),
            "{}: no parameter {id}",
            entry.id()
        );
    }
    let mut params: Vec<ParamValue> = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            overrides.iter().find(|(id, _)| *id == descriptor.id()).map_or_else(
                || descriptor.kind().default_value(),
                |(_, value)| ParamValue::F32(*value),
            )
        })
        .collect();
    params[0] = ParamValue::U32(side);
    params[1] = ParamValue::U32(side);
    params
}

/// Returns the wrapping world of `side` by `side` cells that the host evaluator runs.
fn host_world(side: u32) -> HostWorld {
    HostWorld::Grid {
        width: side,
        height: side,
        boundary: henad_core::authoring::primitives::space::Boundary::Torus,
    }
}

/// Runs every tick pass of `evaluator` once over `lanes`, in document order, as the device runs tick `tick`.
///
/// `params` holds the document's parameters without the grid size.
fn host_tick(evaluator: &HostEvaluator, lanes: &mut HostLanes, params: &[ParamValue], seed: u64, tick: u32) {
    let ticks: Vec<String> = evaluator
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
        .inspect(|pass| assert_eq!(pass.enabled, PassEnabled::Literal(true), "{}", pass.id))
        .map(|pass| pass.id.clone())
        .collect();
    assert!(!ticks.is_empty(), "no tick pass");
    let mut stream = 0;
    for pass in &ticks {
        evaluator
            .run_pass(pass, lanes, params, &RunContext::tick(Some(seed), tick), &mut stream)
            .unwrap_or_else(|error| panic!("{pass}: {error}"));
    }
}

/// Returns the words of every lane of `lanes`, with the lane id.
fn host_words(lanes: &HostLanes, file: &GraphFile) -> Vec<(String, Vec<u32>)> {
    file.document()
        .lanes
        .iter()
        .map(|lane| {
            let column = lanes.column(&lane.id).unwrap_or_else(|| panic!("no lane {}", lane.id));
            (lane.id.clone(), column.words().to_vec())
        })
        .collect()
}

/// Returns the positions of the lane buffers of `file` on the device, each with the lane id, in document order.
fn lane_buffers(file: &GraphFile) -> Vec<(String, usize)> {
    let program = GridProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let buffers = program.spec().buffers;
    file.document()
        .lanes
        .iter()
        .map(|lane| {
            let label = format!("lane_{}_cells", lane.id);
            let position = buffers
                .iter()
                .position(|buffer| buffer.label == label)
                .unwrap_or_else(|| panic!("no buffer {label}"));
            (lane.id.clone(), position)
        })
        .collect()
}

/// Returns the words of every lane of `state`, with the lane id, read from the buffers `buffers` names.
fn device_words(state: &dyn GpuSimState, buffers: &[(String, usize)]) -> Vec<(String, Vec<u32>)> {
    buffers
        .iter()
        .map(|(id, position)| (id.clone(), state.read_buffer(*position).expect("a lane buffer")))
        .collect()
}

/// Returns the words of lane `id` among `words`.
fn lane<'a>(words: &'a [(String, Vec<u32>)], id: &str) -> &'a [u32] {
    words
        .iter()
        .find(|(lane, _)| lane == id)
        .map(|(_, words)| words.as_slice())
        .unwrap_or_else(|| panic!("no lane {id}"))
}

/// Hooks of a graph whose build uploads planted words instead of some of its seeded buffers. Every other hook
/// forwards to the graph's own.
struct PlantedHooks {
    /// Hooks of the graph itself.
    inner: Box<dyn GridHooks>,
    /// Buffers to replace, each as its position and its words.
    planted: Vec<(usize, Vec<u32>)>,
}

impl GridHooks for PlantedHooks {
    fn dims(&self, params: &[ParamValue]) -> (u32, u32) {
        self.inner.dims(params)
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        self.inner.buffer_lens(width, height)
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        self.inner.step_dims(width, height)
    }

    fn seed_buffers(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        let mut buffers = self.inner.seed_buffers(width, height, params, seed);
        for (position, words) in &self.planted {
            assert_eq!(
                buffers[*position].len(),
                words.len(),
                "planted words of buffer {position}"
            );
            buffers[*position].clone_from(words);
        }
        buffers
    }

    fn pass_params_bytes(&self, pass: GridPassId, ctx: GridPassCtx, params: &[ParamValue]) -> Vec<u8> {
        self.inner.pass_params_bytes(pass, ctx, params)
    }

    fn rerun(
        &self,
        action: usize,
        ctx: GridPassCtx,
        params: &[ParamValue],
        stream: &mut u64,
    ) -> Result<RerunUploads, String> {
        self.inner.rerun(action, ctx, params, stream)
    }

    fn stats(&self, counts: &[u32], ctx: GridPassCtx, params: &[ParamValue]) -> Vec<StatValue> {
        self.inner.stats(counts, ctx, params)
    }

    fn stats_with_sums(&self, counts: &[u32], sums: &[f32], ctx: GridPassCtx, params: &[ParamValue]) -> Vec<StatValue> {
        self.inner.stats_with_sums(counts, sums, ctx, params)
    }
}

/// Returns the entry of `file`, a graph on a grid counting the fires of `probes`, whose build uploads the words of
/// `planted` instead of the seeded lanes they name by id.
fn planted_entry(file: &GraphFile, probes: &Probes, planted: &[(String, Vec<u32>)]) -> ModelEntry {
    let program = GridProgram::compile_with_probes(file, probes).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let plain = program.entry();
    let mut spec = program.spec();
    let buffers = lane_buffers(file);
    let planted = planted
        .iter()
        .map(|(id, words)| {
            let position = buffers
                .iter()
                .find(|(lane, _)| lane == id)
                .map(|(_, position)| *position)
                .unwrap_or_else(|| panic!("no lane {id}"));
            (position, words.clone())
        })
        .collect();
    // A second spec of the same program lends its hooks. Those hooks share the program's compiled parts.
    spec.hooks = Box::new(PlantedHooks {
        inner: program.spec().hooks,
        planted,
    });
    let mut declarations = EntryDeclarations::new(
        plain.id().to_owned(),
        plain.name().to_owned(),
        plain.description().to_owned(),
        plain.param_descriptors().to_vec(),
        TopologyHint::GRID,
        spec.structure(),
    );
    declarations.stats = plain.stat_descriptors().to_vec();
    declarations.actions = plain.action_descriptors().to_vec();
    declarations.palette.clone_from(&plain.metadata().palette);
    declarations.time.clone_from(&plain.metadata().time);
    ModelEntry::from_gpu_grid_spec(declarations, plain.source().clone(), Arc::new(spec))
}

/// Returns the lanes of `file` on `side` by `side` cells, each at its initial value apart from lane `id`, whose cells
/// at `cells` (column and row) hold category `category`.
fn planted_lanes(
    file: &GraphFile,
    side: u32,
    id: &str,
    category: u32,
    cells: &[(u32, u32)],
) -> Vec<(String, Vec<u32>)> {
    let evaluator = HostEvaluator::new(file).unwrap_or_else(|error| panic!("{error}"));
    let mut lanes = HostLanes::new(evaluator.document(), host_world(side)).expect("the lanes");
    let column = lanes.column_mut(id).unwrap_or_else(|| panic!("no lane {id}"));
    for &(x, y) in cells {
        assert!(column.set((y * side + x) as usize, HostValue::Category(category)));
    }
    host_words(&lanes, file)
}

/// An override whose id names no parameter of the entry stops the test instead of running at the default.
#[test]
#[should_panic(expected = "no parameter lightning_chanse")]
fn a_mistyped_override_id_panics() {
    let entry = entry_of(&sheet_text("forest_fire.toml"));
    device_params(&entry, 16, &[("lightning_chanse", 0.001)]);
}

/// Returns the cells of `words` holding `category`, as column and row in a world `side` cells wide.
fn cells_in(words: &[u32], side: u32, category: u32) -> Vec<(u32, u32)> {
    (0..side * side)
        .filter(|&cell| words[cell as usize] == category)
        .map(|cell| (cell % side, cell / side))
        .collect()
}

/// Returns a baseline device, or `None` with a warning naming the test `name` when there is no adapter.
pub(super) fn device(name: &str) -> Option<GpuContext> {
    let ctx = headless_test_device(&TestDeviceRequest::baseline());
    if ctx.is_none() {
        log::warn!("skipping {name}: no adapter");
    }
    ctx
}

/// Runs the lowered Game of Life on the device from the alive cells `cells` on a 32 by 32 wrapping world, and returns
/// the alive cells at tick 0 and after each of `ticks` ticks.
fn life_on_the_device(ctx: &GpuContext, cells: &[(u32, u32)], ticks: u32) -> Vec<Vec<(u32, u32)>> {
    let side = 32;
    let file = sheet_file(&sheet_text("game_of_life.toml"));
    let lowered = file.lowered();
    let entry = planted_entry(
        lowered,
        &Probes::default(),
        &planted_lanes(lowered, side, "status", 1, cells),
    );
    let buffers = lane_buffers(lowered);
    let mut state = build_gpu(&entry, &device_params(&entry, side, &[]), Some(1), ctx);
    let mut states = Vec::new();
    for _ in 0..=ticks {
        states.push(cells_in(lane(&device_words(&*state, &buffers), "status"), side, 1));
        step(ctx, &mut *state, 1);
    }
    states
}

/// Returns `cells` moved by `shift` along both axes, sorted as [`cells_in`] returns them.
fn moved(cells: &[(u32, u32)], shift: u32) -> Vec<(u32, u32)> {
    let mut cells: Vec<(u32, u32)> = cells.iter().map(|&(x, y)| (x + shift, y + shift)).collect();
    cells.sort_by_key(|&(x, y)| (y, x));
    cells
}

#[test]
fn the_life_sheet_blinker_has_period_2_on_the_device() {
    let Some(ctx) = device("the_life_sheet_blinker_has_period_2_on_the_device") else {
        return;
    };
    let states = life_on_the_device(&ctx, &[(15, 14), (15, 15), (15, 16)], 4);
    assert_eq!(states[0], [(15, 14), (15, 15), (15, 16)]);
    assert_eq!(states[1], [(14, 15), (15, 15), (16, 15)]);
    assert_eq!(states[2], states[0]);
    assert_eq!(states[3], states[1]);
    assert_eq!(states[4], states[0]);
}

#[test]
fn the_life_sheet_glider_moves_by_one_cell_on_each_axis_every_4_ticks_on_the_device() {
    let Some(ctx) = device("the_life_sheet_glider_moves_by_one_cell_on_each_axis_every_4_ticks_on_the_device") else {
        return;
    };
    let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    let states = life_on_the_device(&ctx, &glider, 8);
    assert_eq!(states[0], moved(&glider, 0));
    assert_eq!(states[4], moved(&glider, 1), "after 4 ticks");
    assert_eq!(states[8], moved(&glider, 2), "after 8 ticks");
    assert!(states.iter().all(|cells| cells.len() == 5), "a glider keeps 5 cells");
}

/// Runs every tick pass of `evaluator` once over `lanes` as tick `tick`, as [`host_tick`] does, and returns the count
/// of each probe of `probes`.
fn host_counted_tick(
    evaluator: &HostEvaluator,
    lanes: &mut HostLanes,
    params: &[ParamValue],
    seed: u64,
    tick: u32,
    probes: &Probes,
) -> Vec<u32> {
    let mut counts = vec![0; probes.len()];
    let mut stream = 0;
    let ticks: Vec<String> = evaluator
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
        .map(|pass| pass.id.clone())
        .collect();
    for pass in &ticks {
        evaluator
            .run_pass_counting(
                pass,
                lanes,
                params,
                &RunContext::tick(Some(seed), tick),
                &mut stream,
                probes,
                &mut counts,
            )
            .unwrap_or_else(|error| panic!("{pass}: {error}"));
    }
    counts
}

/// A blinker planted in the tick-0 lanes gives 2 births and 2 deaths each tick on the device: line 2 counts 2 and line
/// 3 counts 2, read from the counts of the step that reached each tick.
#[test]
fn the_life_sheet_blinker_fires_lines_2_and_3_twice_each_tick_on_the_device() {
    let Some(ctx) = device("the_life_sheet_blinker_fires_lines_2_and_3_twice_each_tick_on_the_device") else {
        return;
    };
    let side = 32;
    let file = sheet_file(&sheet_text("game_of_life.toml"));
    let lowered = file.lowered();
    let fires = file.fire_lines();
    let blinker = [(15, 14), (15, 15), (15, 16)];
    let entry = planted_entry(
        lowered,
        fires.probes(),
        &planted_lanes(lowered, side, "status", 1, &blinker),
    );
    let mut state = build_gpu(&entry, &device_params(&entry, side, &[]), Some(1), &ctx);
    snapshot(&ctx, &mut *state);
    assert!(
        state.fire_counts().counts.iter().all(|&count| count == 0),
        "no step has run"
    );
    for tick in 1..=6u64 {
        step(&ctx, &mut *state, 1);
        snapshot(&ctx, &mut *state);
        let counts = state.fire_counts();
        assert_eq!(counts.tick, tick);
        assert_eq!(
            fires.counts_by_line(&counts.counts),
            BTreeMap::from([("l2".to_owned(), 2), ("l3".to_owned(), 2)]),
            "tick {tick}"
        );
    }
}

/// The forest fire's entry, built as a model file builds it, counts on the device what the host evaluator counts,
/// line by line, every tick for 50 ticks at 64 by 64 cells, and each of its four change lines applies in some tick.
#[test]
fn the_forest_fire_counts_equal_the_host_every_tick_for_50_ticks() {
    let Some(ctx) = device("the_forest_fire_counts_equal_the_host_every_tick_for_50_ticks") else {
        return;
    };
    let side = 64;
    let name = "forest_fire.toml";
    let text = sheet_text(name);
    let file = sheet_file(&text);
    let fires = file.fire_lines();
    let entry = entry_of(&text);
    let params = device_params(&entry, side, active_params(name));
    let evaluator = HostEvaluator::new(file.lowered()).unwrap_or_else(|error| panic!("{error}"));
    let seed = 3;
    let mut state = build_gpu(&entry, &params, Some(seed), &ctx);
    let mut lanes = evaluator
        .init(host_world(side), &params[2..], Some(seed))
        .unwrap_or_else(|error| panic!("{error}"));
    let mut applied: BTreeMap<String, u64> = BTreeMap::new();
    for tick in 0..50u32 {
        let host = host_counted_tick(&evaluator, &mut lanes, &params[2..], seed, tick, fires.probes());
        step(&ctx, &mut *state, 1);
        snapshot(&ctx, &mut *state);
        assert_eq!(
            state.fire_counts(),
            FireCounts::new(u64::from(tick) + 1, host.clone()),
            "the counts of tick {}",
            tick + 1
        );
        for (line, count) in fires.counts_by_line(&host) {
            *applied.entry(line).or_insert(0) += count;
        }
    }
    assert_eq!(
        applied.keys().map(String::as_str).collect::<Vec<_>>(),
        ["l2", "l3", "l4", "l5"]
    );
    assert!(applied.values().all(|&count| count > 0), "{applied:?}");
}

/// A press of Randomise on the device, the Game of Life sheet's Start again button, draws a new layout each press, the
/// layout the host's rerun of the same press draws, and a second build replays the presses.
#[test]
fn a_start_again_button_draws_a_new_layout_each_press() {
    let Some(ctx) = device("a_start_again_button_draws_a_new_layout_each_press") else {
        return;
    };
    let side = 64;
    let text = sheet_text("game_of_life.toml");
    let file = sheet_file(&text);
    let entry = entry_of(&text);
    let randomise = entry.action_index("randomise").expect("the Randomise button");
    let params = device_params(&entry, side, &[]);
    let buffers = lane_buffers(file.lowered());
    let evaluator = HostEvaluator::new(file.lowered()).unwrap_or_else(|error| panic!("{error}"));
    let presses = || {
        let mut state = build_gpu(&entry, &params, Some(7), &ctx);
        let mut layouts = vec![device_words(&*state, &buffers)];
        for _ in 0..2 {
            press(&ctx, &mut *state, randomise);
            layouts.push(device_words(&*state, &buffers));
        }
        layouts
    };
    let layouts = presses();
    assert_ne!(layouts[1], layouts[0], "the first press draws a new layout");
    assert_ne!(layouts[2], layouts[1], "the second press draws a new layout");
    assert_eq!(presses(), layouts, "a second build replays the presses");

    let mut lanes = evaluator
        .init(host_world(side), &params[2..], Some(7))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(host_words(&lanes, file.lowered()), layouts[0], "the start");
    let mut stream = action_seed(Some(7));
    for press in 1..=2u32 {
        evaluator
            .rerun("randomise", press, &mut lanes, &params[2..], Some(7), &mut stream)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            host_words(&lanes, file.lowered()),
            layouts[press as usize],
            "press {press} on the host"
        );
    }
}

/// Two builds of the Game of Life sheet at one seed start alike on the device, as the app's Build replays the world's
/// start, and a build at another seed draws another start.
#[test]
fn a_rebuild_replays_the_same_start() {
    let Some(ctx) = device("a_rebuild_replays_the_same_start") else {
        return;
    };
    let side = 64;
    let text = sheet_text("game_of_life.toml");
    let file = sheet_file(&text);
    let entry = entry_of(&text);
    let params = device_params(&entry, side, &[]);
    let buffers = lane_buffers(file.lowered());
    let start = |seed: u64| {
        let state = build_gpu(&entry, &params, Some(seed), &ctx);
        device_words(&*state, &buffers)
    };
    let first = start(7);
    assert_eq!(start(7), first, "a rebuild at seed 7");
    assert_ne!(start(8), first, "a build at seed 8");
    // About 30% of 4096 cells start alive.
    let alive = cells_in(lane(&first, "status"), side, 1).len();
    assert!((1000..1500).contains(&alive), "{alive} alive");
}

/// Parameters each grid sheet runs at in the comparisons of the host and the device, chosen so every draw of its card
/// decides some cells within 50 ticks of a 64 by 64 world.
fn active_params(name: &str) -> &'static [(&'static str, f32)] {
    match name {
        "forest_fire.toml" => &[("lightning_chance", 0.001), ("growth_chance", 0.05)],
        _ => &[],
    }
}

/// Every grid sheet steps to equal lanes on the host evaluator and on the device every tick for 50 ticks at 64 by 64
/// cells, from the start both draw at two seeds.
#[test]
fn every_grid_sheet_steps_alike_on_the_host_and_the_device() {
    let Some(ctx) = device("every_grid_sheet_steps_alike_on_the_host_and_the_device") else {
        return;
    };
    let side = 64;
    let sheets = grid_sheets();
    assert_eq!(
        sheets.len(),
        5,
        "Game of Life, the rates SIR, Vote, the forest fire and the chance SIR"
    );
    for (name, text) in &sheets {
        let file = sheet_file(text);
        let lowered = file.lowered();
        let entry = entry_of(text);
        let params = device_params(&entry, side, active_params(name));
        let buffers = lane_buffers(lowered);
        let evaluator = HostEvaluator::new(lowered).unwrap_or_else(|error| panic!("{name}: {error}"));
        for seed in [3, 11] {
            let mut state = build_gpu(&entry, &params, Some(seed), &ctx);
            let mut lanes = evaluator
                .init(host_world(side), &params[2..], Some(seed))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let start = host_words(&lanes, lowered);
            let mut changed = false;
            for tick in 0..=50u32 {
                let host = host_words(&lanes, lowered);
                changed |= host != start;
                assert!(
                    device_words(&*state, &buffers) == host,
                    "{name} at seed {seed}: the lanes differ at tick {tick}"
                );
                step(&ctx, &mut *state, 1);
                host_tick(&evaluator, &mut lanes, &params[2..], seed, tick);
            }
            assert!(changed, "{name} at seed {seed}: no cell changed");
        }
    }
}

/// Returns the count of cells in each of the `states` states of `words`.
fn state_counts(words: &[u32], states: usize) -> Vec<usize> {
    let mut counts = vec![0; states];
    for &word in words {
        counts[word as usize] += 1;
    }
    counts
}

/// The chance SIR on the device keeps Susceptible, Infected and Recovered summing to the cell count every tick for 200
/// ticks at 64 by 64 cells, in its lanes and its measures, across a press of New outbreak after the first tick.
#[test]
fn the_chance_sir_sheet_conserves_its_cells_every_tick_on_the_device() {
    let Some(ctx) = device("the_chance_sir_sheet_conserves_its_cells_every_tick_on_the_device") else {
        return;
    };
    let side = 64u32;
    let cells = (side * side) as usize;
    let text = sheet_text("sir_chance.toml");
    let file = sheet_file(&text);
    let entry = entry_of(&text);
    let new_outbreak = entry.action_index("new_outbreak").expect("the New outbreak button");
    let buffers = lane_buffers(file.lowered());
    let mut state = build_gpu(&entry, &device_params(&entry, side, &[]), Some(5), &ctx);
    let mut peak = 0;
    for tick in 0..=200u32 {
        if tick == 1 {
            let before = state_counts(lane(&device_words(&*state, &buffers), "health"), 3);
            press(&ctx, &mut *state, new_outbreak);
            let after = state_counts(lane(&device_words(&*state, &buffers), "health"), 3);
            assert!(
                after[1] > before[1],
                "New outbreak infects more cells: {before:?}, then {after:?}"
            );
        }
        let counts = state_counts(lane(&device_words(&*state, &buffers), "health"), 3);
        assert_eq!(
            counts.iter().sum::<usize>(),
            cells,
            "the lanes at tick {tick}: {counts:?}"
        );
        snapshot(&ctx, &mut *state);
        let stats = state.stats();
        let measured: Vec<f64> = ["Susceptible", "Infected", "Recovered"]
            .iter()
            .map(|label| stat(&stats, label))
            .collect();
        let counted: Vec<f64> = counts.iter().map(|&count| count as f64).collect();
        assert_eq!(measured, counted, "the measures at tick {tick}");
        peak = peak.max(counts[1]);
        step(&ctx, &mut *state, 1);
    }
    let end = state_counts(lane(&device_words(&*state, &buffers), "health"), 3);
    assert!(
        peak > 0 && end[2] > 0,
        "the epidemic spreads and recovers: {peak}, {end:?}"
    );
}

/// Seeds of the comparisons with the hand-lowered files.
const SEEDS: [u64; 4] = [1, 2, 3, 4];

/// Ticks of the equality runs against the hand-lowered files.
const EQUAL_TICKS: u32 = 200;

/// Side of the equality runs against the hand-lowered files.
const EQUAL_SIDE: u32 = 64;

/// Sheets whose cards draw nothing, each with its hand-lowered file.
const DRAWLESS: [(&str, &str); 2] = [("game_of_life.toml", "game_of_life.toml"), ("vote.toml", "vote.toml")];

/// Returns the host parameters of the graph on a grid `file`: each parameter at its default, without the grid size.
fn default_host_params(file: &GraphFile) -> Vec<ParamValue> {
    let entry = GridProgram::compile(file)
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
        .entry();
    device_params(&entry, EQUAL_SIDE, &[]).split_off(2)
}

/// Returns the tick-0 lanes the sheet `file` draws at `seed` on the host, planted in both runs of an equality test.
fn shared_start(file: &SheetFile, seed: u64) -> Vec<(String, Vec<u32>)> {
    let evaluator = HostEvaluator::new(file.lowered()).unwrap_or_else(|error| panic!("{error}"));
    let lanes = evaluator
        .init(host_world(EQUAL_SIDE), &default_host_params(file.lowered()), Some(seed))
        .unwrap_or_else(|error| panic!("{error}"));
    host_words(&lanes, file.lowered())
}

/// Returns the host lanes of `evaluator` on the equality world, each planted with the words `start` holds for it.
fn host_lanes_from(evaluator: &HostEvaluator, start: &[(String, Vec<u32>)]) -> HostLanes {
    let mut lanes = HostLanes::new(evaluator.document(), host_world(EQUAL_SIDE)).expect("the lanes");
    for (id, words) in start {
        let column = lanes.column_mut(id).unwrap_or_else(|| panic!("no lane {id}"));
        for (cell, &word) in words.iter().enumerate() {
            assert!(column.set(cell, HostValue::Category(word)), "lane {id}, cell {cell}");
        }
    }
    lanes
}

/// Game of Life and Vote, whose cards draw nothing, equal their hand-lowered files in every lane every tick for 200
/// ticks on the host evaluator, from the tick-0 lanes each sheet draws at seeds 1 to 4.
#[test]
fn the_drawless_sheets_equal_their_hand_lowered_files_every_tick_on_the_host() {
    for (sheet, reference) in DRAWLESS {
        let file = sheet_file(&sheet_text(sheet));
        let reference = hand_lowered(reference);
        let sheet_host = HostEvaluator::new(file.lowered()).unwrap_or_else(|error| panic!("{sheet}: {error}"));
        let reference_host = HostEvaluator::new(&reference).unwrap_or_else(|error| panic!("{sheet}: {error}"));
        let sheet_params = default_host_params(file.lowered());
        let reference_params = default_host_params(&reference);
        for seed in SEEDS {
            let start = shared_start(&file, seed);
            let mut sheet_lanes = host_lanes_from(&sheet_host, &start);
            let mut reference_lanes = host_lanes_from(&reference_host, &start);
            let mut changed = false;
            for tick in 0..EQUAL_TICKS {
                host_tick(&sheet_host, &mut sheet_lanes, &sheet_params, seed, tick);
                host_tick(&reference_host, &mut reference_lanes, &reference_params, seed, tick);
                let words = host_words(&sheet_lanes, file.lowered());
                changed |= words != start;
                assert!(
                    words == host_words(&reference_lanes, &reference),
                    "{sheet} at seed {seed}: the lanes differ after tick {tick}"
                );
            }
            assert!(changed, "{sheet} at seed {seed}: no cell changed");
        }
    }
}

/// Game of Life and Vote, whose cards draw nothing, equal their hand-lowered files in every lane every tick for 200
/// ticks on the device, from the tick-0 lanes each sheet draws at seeds 1 to 4, planted in both.
#[test]
fn the_drawless_sheets_equal_their_hand_lowered_files_every_tick_on_the_device() {
    let Some(ctx) = device("the_drawless_sheets_equal_their_hand_lowered_files_every_tick_on_the_device") else {
        return;
    };
    for (sheet, reference) in DRAWLESS {
        let file = sheet_file(&sheet_text(sheet));
        let reference = hand_lowered(reference);
        let sheet_buffers = lane_buffers(file.lowered());
        let reference_buffers = lane_buffers(&reference);
        for seed in SEEDS {
            let start = shared_start(&file, seed);
            let sheet_entry = planted_entry(file.lowered(), &Probes::default(), &start);
            let reference_entry = planted_entry(&reference, &Probes::default(), &start);
            let mut sheet_state = build_gpu(
                &sheet_entry,
                &device_params(&sheet_entry, EQUAL_SIDE, &[]),
                Some(seed),
                &ctx,
            );
            let mut reference_state = build_gpu(
                &reference_entry,
                &device_params(&reference_entry, EQUAL_SIDE, &[]),
                Some(seed),
                &ctx,
            );
            assert_eq!(
                device_words(&*sheet_state, &sheet_buffers),
                start,
                "{sheet}: the planted start"
            );
            for tick in 0..EQUAL_TICKS {
                step(&ctx, &mut *sheet_state, 1);
                step(&ctx, &mut *reference_state, 1);
                assert!(
                    device_words(&*sheet_state, &sheet_buffers) == device_words(&*reference_state, &reference_buffers),
                    "{sheet} at seed {seed}: the lanes differ after tick {tick}"
                );
            }
        }
    }
}

/// A sheet held to its hand-lowered file by bands, with the run the bands are taken over.
struct BandCase {
    /// File name of the sheet.
    sheet: &'static str,
    /// File name of the hand-lowered file.
    reference: &'static str,
    /// Id of the one lane, a States property.
    lane: &'static str,
    /// Number of states of the lane.
    states: usize,
    /// State whose peak and ever-reached share the bands hold: Burning or Infected.
    active: u32,
    /// Side of the world.
    side: u32,
    /// Ticks of the run.
    ticks: u32,
    /// Parameters set away from their defaults.
    overrides: &'static [(&'static str, f32)],
    /// Text of the planted rule error: a line of the sheet, and the same line with its "from" state dropped.
    planted: (&'static str, &'static str),
}

/// The forest fire and the chance SIR, at sizes and parameters where a run holds many fires or one whole epidemic.
const BAND_CASES: [BandCase; 2] = [
    BandCase {
        sheet: "forest_fire.toml",
        reference: "forest_fire.toml",
        lane: "cover",
        states: 3,
        active: 2,
        side: 512,
        ticks: 150,
        overrides: &[("lightning_chance", 0.00005)],
        // Line 3, "otherwise become Burning from Tree, with chance Lightning chance", made to strike any state.
        planted: (
            "new_state = \"burning\"\nstart_state = \"tree\"\n\n[card.line.l3.condition]",
            "new_state = \"burning\"\nstart_state = \"any_state\"\n\n[card.line.l3.condition]",
        ),
    },
    BandCase {
        sheet: "sir_chance.toml",
        reference: "sir.toml",
        lane: "health",
        states: 3,
        active: 1,
        side: 512,
        ticks: 60,
        overrides: &[("infection_chance", 0.1), ("recovery_chance", 0.1)],
        // Line 3, "Become Recovered from Infected, with chance Recovery chance", made to recover any state.
        planted: (
            "new_state = \"recovered\"\nstart_state = \"infected\"",
            "new_state = \"recovered\"\nstart_state = \"any_state\"",
        ),
    },
];

/// Measures of one run that the bands hold: the peak share of the active state, the share of each state at the last
/// tick, and the share of cells that were ever in the active state.
fn band_measures(ctx: &GpuContext, case: &BandCase, entry: &ModelEntry, file: &GraphFile, seed: u64) -> Vec<f64> {
    let buffers = lane_buffers(file);
    let cells = f64::from(case.side * case.side);
    let mut state = build_gpu(entry, &device_params(entry, case.side, case.overrides), Some(seed), ctx);
    let mut reached = vec![false; (case.side * case.side) as usize];
    let mut peak = 0usize;
    let mut last = Vec::new();
    for tick in 0..=case.ticks {
        let words = device_words(&*state, &buffers);
        let words = lane(&words, case.lane);
        let counts = state_counts(words, case.states);
        assert_eq!(counts.iter().sum::<usize>() as f64, cells, "the lanes at tick {tick}");
        peak = peak.max(counts[case.active as usize]);
        for (reached, &word) in reached.iter_mut().zip(words) {
            *reached |= word == case.active;
        }
        last = counts;
        if tick < case.ticks {
            step(ctx, &mut *state, 1);
        }
    }
    let mut measures = vec![peak as f64 / cells];
    measures.extend(last.iter().map(|&count| count as f64 / cells));
    measures.push(reached.iter().filter(|&&reached| reached).count() as f64 / cells);
    measures
}

/// Half-width of each band, in standard errors.
const BAND_SIGMAS: f64 = 5.0;

/// Returns the mean of each measure over `runs`, one run per seed.
fn means(runs: &[Vec<f64>]) -> Vec<f64> {
    (0..runs[0].len())
        .map(|measure| runs.iter().map(|run| run[measure]).sum::<f64>() / runs.len() as f64)
        .collect()
}

/// Returns the band of each measure, as its centre and half-width, for the mean of a sheet's runs `sheet` against the
/// hand-lowered file's runs `reference`, one run per seed on each side.
///
/// The centre is the hand-lowered file's mean. The half-width is [`BAND_SIGMAS`] standard errors of the difference
/// of the two means. The standard error takes the larger of two variances of one run: the variance about each side's
/// own mean, pooled over both sides, and the binomial variance `p (1 - p) / cells`, with `p` the centre kept at least
/// one cell from 0 and from 1.
fn bands(reference: &[Vec<f64>], sheet: &[Vec<f64>], cells: f64) -> Vec<(f64, f64)> {
    let (reference_means, sheet_means) = (means(reference), means(sheet));
    let degrees = (reference.len() + sheet.len() - 2) as f64;
    let weight = 1.0 / reference.len() as f64 + 1.0 / sheet.len() as f64;
    reference_means
        .iter()
        .zip(&sheet_means)
        .enumerate()
        .map(|(measure, (&center, &sheet_mean))| {
            let squares =
                |runs: &[Vec<f64>], mean: f64| runs.iter().map(|run| (run[measure] - mean).powi(2)).sum::<f64>();
            let pooled = (squares(reference, center) + squares(sheet, sheet_mean)) / degrees;
            let p = center.clamp(1.0 / cells, 1.0 - 1.0 / cells);
            let binomial = p * (1.0 - p) / cells;
            (center, BAND_SIGMAS * (pooled.max(binomial) * weight).sqrt())
        })
        .collect()
}

/// Returns each measure whose mean over the sheet's runs `sheet` sits outside its band against the hand-lowered file's
/// runs `reference`, as its position, the sheet's mean and the band.
fn outside(reference: &[Vec<f64>], sheet: &[Vec<f64>], cells: f64) -> Vec<(usize, f64, (f64, f64))> {
    means(sheet)
        .into_iter()
        .zip(bands(reference, sheet, cells))
        .enumerate()
        .filter(|(_, (mean, (center, half)))| (mean - center).abs() > *half)
        .map(|(position, (mean, band))| (position, mean, band))
        .collect()
}

/// The forest fire and the chance SIR draw with other keys than their hand-lowered files, so each is held to bands
/// taken over seeds 1 to 4, and a sheet with a rule error planted falls outside them.
///
/// The band compares the two means at five standard errors of their difference, with the binomial variance of
/// `graph_sir_parity.rs` as a floor:
///
/// 1. Run the hand-lowered file on the device from its own start at each seed 1 to 4, at the case's size, ticks and
///    parameters, and record each measure of [`band_measures`]: the peak share of the active state (Burning or
///    Infected), the share of each state at the last tick, and the share of cells ever in the active state, the burnt
///    share of the forest fire.
/// 2. Run the sheet at the same seeds and record the same measures.
/// 3. Hold the sheet's mean of each measure over the four seeds within its band, as [`bands`] takes it: the hand-lowered
///    file's mean, give or take five standard errors of the difference of the two means. A run of an epidemic or of
///    fires spreads further from seed to seed than a binomial share, as the cells of one wave decide together. The
///    variance of one run is the larger of the variance pooled over the eight runs and the binomial variance.
/// 4. Plant the rule error, the "from" state of one Become dropped, run the sheet so edited at the same seeds, and hold
///    at least one of its means outside its band, against the same four runs of the hand-lowered file.
#[test]
fn the_forest_fire_and_the_chance_sir_sheets_sit_within_their_hand_lowered_bands() {
    let Some(ctx) = device("the_forest_fire_and_the_chance_sir_sheets_sit_within_their_hand_lowered_bands") else {
        return;
    };
    for case in &BAND_CASES {
        let cells = f64::from(case.side * case.side);
        let runs = |entry: &ModelEntry, file: &GraphFile| -> Vec<Vec<f64>> {
            SEEDS
                .iter()
                .map(|&seed| band_measures(&ctx, case, entry, file, seed))
                .collect()
        };
        let reference = hand_lowered(case.reference);
        let reference_entry = GridProgram::compile(&reference)
            .unwrap_or_else(|problems| panic!("{problems:#?}"))
            .entry();
        let reference_runs = runs(&reference_entry, &reference);

        let text = sheet_text(case.sheet);
        let file = sheet_file(&text);
        let sheet_runs = runs(&entry_of(&text), file.lowered());
        let outliers = outside(&reference_runs, &sheet_runs, cells);
        assert!(
            outliers.is_empty(),
            "{}: {outliers:?} outside the bands, the hand-lowered runs {reference_runs:?}, the sheet's {sheet_runs:?}",
            case.sheet
        );

        let (line, wrong) = case.planted;
        assert_eq!(
            text.matches(line).count(),
            1,
            "{}: the line to plant the error in",
            case.sheet
        );
        let planted_text = text.replace(line, wrong);
        let planted = sheet_file(&planted_text);
        let planted_runs = runs(&entry_of(&planted_text), planted.lowered());
        assert!(
            !outside(&reference_runs, &planted_runs, cells).is_empty(),
            "{}: the planted rule error stays within the bands, the hand-lowered runs {reference_runs:?}, the \
             planted sheet's {planted_runs:?}",
            case.sheet
        );
    }
}

/// The entry of every grid sheet passes every check of the testing kit, its GPU checks on a baseline device included.
#[test]
fn every_grid_sheet_entry_conforms() {
    let mut models = ModelSet::new(henad_core::build_info!());
    for (name, text) in grid_sheets() {
        models
            .insert(entry_of(&text))
            .unwrap_or_else(|error| panic!("{name}: the set refused the entry: {error}"));
    }
    let mut settings = CheckSettings::default();
    if let Some(device) = headless_test_device(&TestDeviceRequest::baseline()) {
        settings = settings.gpu(device);
    } else {
        log::warn!("checking the grid sheets without their GPU checks: no adapter");
    }
    assert_set_conforms(&models, &settings);
}

/// Returns the position of parameter `id` among the parameters of `entry`.
fn param_index(entry: &ModelEntry, id: &str) -> usize {
    entry
        .param_descriptors()
        .iter()
        .position(|descriptor| descriptor.id() == id)
        .unwrap_or_else(|| panic!("{}: no parameter {id}", entry.id()))
}

/// A live edit of the infection rate on a running SIR sheet keeps the tick and the cells, and every later tick steps
/// on the device as the host steps at the new rate. The start's share and the tick length stay reload-only.
#[test]
fn a_live_rate_edit_keeps_the_tick_and_the_cells_and_later_ticks_follow_it_on_the_device() {
    let Some(ctx) = device("a_live_rate_edit_keeps_the_tick_and_the_cells_and_later_ticks_follow_it_on_the_device")
    else {
        return;
    };
    let (side, seed, before, after) = (64, 5, 10u32, 30u32);
    let text = sheet_text("sir.toml");
    let file = sheet_file(&text);
    let lowered = file.lowered();
    let entry = entry_of(&text);
    let rate = param_index(&entry, "infection_rate");
    let mut params = device_params(&entry, side, &[("infection_rate", 0.3)]);
    let buffers = lane_buffers(lowered);
    let evaluator = HostEvaluator::new(lowered).unwrap_or_else(|error| panic!("{error}"));

    let mut state = build_gpu(&entry, &params, Some(seed), &ctx);
    let mut kept = build_gpu(&entry, &params, Some(seed), &ctx);
    let mut lanes = evaluator
        .init(host_world(side), &params[2..], Some(seed))
        .unwrap_or_else(|error| panic!("{error}"));
    for tick in 0..before {
        step(&ctx, &mut *state, 1);
        step(&ctx, &mut *kept, 1);
        host_tick(&evaluator, &mut lanes, &params[2..], seed, tick);
    }
    let cells = device_words(&*state, &buffers);

    params[rate] = ParamValue::F32(2.0);
    assert!(state.set_param(rate, &params[rate]), "the infection rate is live");
    for id in ["initial_infected", "tick_length", "grid_width"] {
        let index = param_index(&entry, id);
        assert!(!state.set_param(index, &params[index]), "{id} is reload-only");
    }
    assert_eq!(state.tick(), u64::from(before), "the edit keeps the tick");
    assert!(device_words(&*state, &buffers) == cells, "the edit keeps the cells");

    let mut parted = false;
    for tick in before..before + after {
        step(&ctx, &mut *state, 1);
        step(&ctx, &mut *kept, 1);
        host_tick(&evaluator, &mut lanes, &params[2..], seed, tick);
        let words = device_words(&*state, &buffers);
        assert!(
            words == host_words(&lanes, lowered),
            "the lanes differ from the host at the new rate at tick {}",
            tick + 1
        );
        parted |= words != device_words(&*kept, &buffers);
    }
    assert!(
        parted,
        "the run at the new rate never parts from the run that kept the old rate"
    );
}
