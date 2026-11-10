//! The agent sheet starters built and stepped on a baseline device: each runs, binds within the baseline's storage
//! buffers and meets the testing kit, a capped fold's fault names its author lines, and each holds a band against the
//! graph lowered by hand in `tests/fixtures/lowered/round1/` on seeds 1 to 3, with a planted rule error outside it.

use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::stepping::{run_steps, sample_stats};
use henad_core::params::ParamValue;
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};
use henad_graph::sheet::{SheetFile, starters};

use super::graph_ants_parity::graph_buffer;
use super::graph_grid_parity::{build_gpu, snapshot, stat, step};

/// Hand-lowered random walk of `tests/fixtures/lowered/round1/`.
const HAND_WALK: &str = include_str!("../../tests/fixtures/lowered/round1/random_walk.toml");
/// Hand-lowered boids of `tests/fixtures/lowered/round1/`.
const HAND_BOIDS: &str = include_str!("../../tests/fixtures/lowered/round1/boids.toml");
/// Hand-lowered bounded confidence of `tests/fixtures/lowered/round1/`.
const HAND_OPINIONS: &str = include_str!("../../tests/fixtures/lowered/round1/bounded_confidence.toml");
/// Hand-lowered ants of `tests/fixtures/lowered/round1/`.
const HAND_ANTS: &str = include_str!("../../tests/fixtures/lowered/round1/ants.toml");

/// Seeds every band runs at.
const SEEDS: [u64; 3] = [1, 2, 3];

/// Returns the agent sheet starters with their file names.
pub(super) fn agent_sheets() -> Vec<(&'static str, &'static str)> {
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

/// Returns the entry the model file `text`, a sheet or a graph, builds.
fn entry_of(text: &str) -> ModelEntry {
    ModelFile::parse(text)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{error}"))
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

/// Returns the components of the interleaved `vec2<f32>` lane `words`.
fn pairs(words: &[u32]) -> Vec<(f32, f32)> {
    words
        .chunks_exact(2)
        .map(|pair| (f32::from_bits(pair[0]), f32::from_bits(pair[1])))
        .collect()
}

/// Returns the baseline device, or `None` with a warning naming `test` when this machine has none.
fn device(test: &str) -> Option<GpuContext> {
    let ctx = headless_test_device(&TestDeviceRequest::baseline());
    if ctx.is_none() {
        log::warn!("skipping {test}: no adapter");
    }
    ctx
}

#[test]
fn every_agent_sheet_lowers_checks_and_compiles() {
    let Some(ctx) = device("every_agent_sheet_lowers_checks_and_compiles") else {
        return;
    };
    let names: Vec<&str> = agent_sheets().iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        ["random_walk.toml", "boids.toml", "bounded_confidence.toml", "ants.toml"]
    );
    for (name, text) in agent_sheets() {
        let file = ModelFile::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(file.warnings().is_empty(), "{name}: {:?}", file.warnings());
        let lowered = file.lowered_graph().expect("a sheet lowers to a graph");
        assert!(lowered.check().diagnostics.is_empty(), "{name}");
        let entry = file.entry().unwrap_or_else(|error| panic!("{name}: {error}"));
        let params = params_of(&entry, &[]);
        let mut state = build_gpu(&entry, &params, Some(1), &ctx);
        run_steps(&mut *state, &ctx, 100).unwrap_or_else(|fault| panic!("{name}: {fault}"));
        sample_stats(&mut *state, &ctx).unwrap_or_else(|fault| panic!("{name}: {fault}"));
    }
}

#[test]
fn the_lowered_agent_sheets_bind_at_most_8_storage_buffers() {
    let limits = wgpu::Limits::default();
    assert_eq!(limits.max_storage_buffers_per_shader_stage, 8);
    for (name, text) in agent_sheets() {
        let entry = entry_of(text);
        let demand = entry
            .demand(&params_of(&entry, &[]), &limits)
            .expect("a GPU entry has a demand");
        for pass in &demand.passes {
            assert!(
                pass.storage <= 8,
                "{name}: pass '{}' binds {}",
                pass.label,
                pass.storage
            );
        }
        // The ants' tick pass sits at the limit: three lanes and the Heading, the trails and their deposits, the
        // ground and the counters.
        if name == "ants.toml" {
            let tick = demand.passes.iter().find(|pass| pass.label == "ants_tick_ant_tick");
            assert_eq!(tick.map(|pass| pass.storage), Some(8));
        }
    }
}

#[test]
fn every_agent_sheet_entry_conforms() {
    let mut models = ModelSet::new(henad_core::build_info!());
    for (name, text) in agent_sheets() {
        models
            .insert(entry_of(text))
            .unwrap_or_else(|error| panic!("{name}: the set refused the entry: {error}"));
    }
    let mut settings = CheckSettings::default();
    if let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) {
        settings = settings.gpu(ctx);
    } else {
        log::warn!("checking the agent sheets without their GPU checks: no adapter");
    }
    assert_set_conforms(&models, &settings);
    // At its largest population, each sheet fits a device that holds its buffers, or is refused in its own words.
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: u64::MAX,
        max_buffer_size: u64::MAX,
        ..wgpu::Limits::default()
    };
    for (name, text) in agent_sheets() {
        let file = SheetFile::parse(text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let entry = entry_of(text);
        let largest = file.document().agents[0].max_count;
        let params = params_of(&entry, &[("num_agents", ParamValue::U32(largest))]);
        for line in file.shortfalls(&entry, &params, &limits) {
            assert!(
                line.contains("looks at too many") || line.contains("too much work"),
                "{name} at {largest}: {line}"
            );
            for word in henad_graph::sheet::sentence::FORBIDDEN_WORDS {
                assert!(!line.contains(word), "{name}: {line} holds {word}");
            }
        }
    }
}

/// A radius fold of the boids that meets more than 65536 birds faults with the author lines of its looks, and no pass,
/// node or line of the lowered graph. The planted move takes every bird to the origin after the first tick. The
/// submission budget charges a uniform spread and admits the run, and the second tick's fold meets every bird. The
/// device runs another model afterwards.
#[test]
fn a_capped_fold_fault_names_the_author_line() {
    let Some(ctx) = device("a_capped_fold_fault_names_the_author_line") else {
        return;
    };
    let gathered = edited(
        sheet("boids.toml"),
        "mode = \"by\"\nvalue = { my_new = \"velocity\" }",
        "mode = \"by\"\n\n[card.line.l7.value]\nkind = \"calculate\"\nformula = \"-(my.position)\"",
    );
    let entry = entry_of(&gathered);
    let params = params_of(&entry, &[("num_agents", ParamValue::U32(70_000))]);
    let mut state = build_gpu(&entry, &params, Some(1), &ctx);
    let fault = run_steps(&mut *state, &ctx, 3).expect_err("the fold faults");
    let message = fault.to_string();
    assert!(
        message.contains(
            "lines 1 and 2, Flockmates and Too close: looked at more than 65536 agents around one agent, and the run \
             stops"
        ),
        "{message}"
    );
    assert!(!message.contains("node") && !message.contains("pass"), "{message}");
    drop(state);
    let walk = entry_of(sheet("random_walk.toml"));
    let mut other = build_gpu(&walk, &params_of(&walk, &[]), None, &ctx);
    step(&ctx, &mut *other, 2);
    sample_stats(&mut *other, &ctx).expect("the device runs another model after the fault");
}

/// Mean over the agents of the squared distance moved in one tick, the shorter way round a torus `side` wide.
fn mean_squared_step(before: &[(f32, f32)], after: &[(f32, f32)], side: f32) -> f64 {
    let torus = |a: f32, b: f32| {
        let distance = (a - b).abs();
        f64::from(distance.min(side - distance))
    };
    let total: f64 = before
        .iter()
        .zip(after)
        .map(|(&(x0, y0), &(x1, y1))| torus(x0, x1).powi(2) + torus(y0, y1).powi(2))
        .sum();
    total / before.len() as f64
}

/// Returns the mean squared step of one tick of the random walk `text` at `seed`, after 3 ticks, with 4 steps a tick of
/// length 2.
fn walk_step(ctx: &GpuContext, text: &str, seed: u64) -> f64 {
    let entry = entry_of(text);
    let params = params_of(&entry, &[]);
    let mut state = build_gpu(&entry, &params, Some(seed), ctx);
    step(ctx, &mut *state, 3);
    let before = pairs(&graph_buffer(&entry, &*state, "lane_position_agents"));
    step(ctx, &mut *state, 1);
    let after = pairs(&graph_buffer(&entry, &*state, "lane_position_agents"));
    mean_squared_step(&before, &after, 1000.0)
}

/// Band: over a tick each walker moves a mean squared distance of `steps * length^2`, 16, the closed form of a walk
/// of uniform headings, within five standard errors, `sqrt(12 * length^4 / walkers)`, since the square of a sum of four
/// steps has variance `12 L^4`. Procedure: the mean over the 10000 walkers of the squared torus distance between the
/// positions after ticks 3 and 4, at the defaults, for the sheet and the hand-lowered file on seeds 1 to 3. The
/// planted error moves once a tick, outside the Repeat, and lands near 4.
#[test]
fn the_random_walk_sheet_steps_its_closed_form() {
    let Some(ctx) = device("the_random_walk_sheet_steps_its_closed_form") else {
        return;
    };
    let expected = 4.0 * 2.0f64.powi(2);
    let band = 5.0 * (12.0 * 2.0f64.powi(4) / 10_000.0).sqrt();
    let planted = edited(
        &edited(
            sheet("random_walk.toml"),
            "lines = [\"l1\"]",
            "lines = [\"l1\", \"l2\"]",
        ),
        "lines = [\"l2\"]",
        "lines = []",
    );
    for seed in SEEDS {
        for (name, text) in [("sheet", sheet("random_walk.toml")), ("hand", HAND_WALK)] {
            let found = walk_step(&ctx, text, seed);
            assert!(
                (found - expected).abs() <= band,
                "{name} at seed {seed}: a tick's mean squared step is {found}, and the closed form {expected}"
            );
        }
        let found = walk_step(&ctx, &planted, seed);
        assert!(
            (found - expected).abs() > band,
            "the planted error at seed {seed} steps {found}"
        );
    }
}

/// Returns the polarisation of `velocities`: the length of the mean of their unit vectors, 0 for headings spread evenly
/// and 1 for one heading.
fn polarisation(velocities: &[(f32, f32)]) -> f64 {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for &(vx, vy) in velocities {
        let length = f64::from(vx.hypot(vy)).max(1e-9);
        x += f64::from(vx) / length;
        y += f64::from(vy) / length;
    }
    x.hypot(y) / velocities.len() as f64
}

/// Run of the boids with 20000 birds over 300 ticks.
struct BoidsRun {
    /// Polarisation after the last tick.
    polarisation: f64,
    /// First speed outside the Keep between limits, 3 and 15, as the tick, the bird and the speed, `None` when every
    /// speed stayed within them or the speeds were not read.
    outside: Option<(u32, usize, f32)>,
}

/// Runs the boids `text` with 20000 birds for 300 ticks at `seed`. With `speeds`, reads every speed after each tick.
fn boids_run(ctx: &GpuContext, text: &str, seed: u64, speeds: bool) -> BoidsRun {
    let entry = entry_of(text);
    let params = params_of(&entry, &[("num_agents", ParamValue::U32(20_000))]);
    let mut state = build_gpu(&entry, &params, Some(seed), ctx);
    let mut outside = None;
    if speeds {
        for tick in 1..=300 {
            step(ctx, &mut *state, 1);
            if outside.is_some() {
                continue;
            }
            let velocities = pairs(&graph_buffer(&entry, &*state, "lane_velocity_agents"));
            outside = velocities
                .iter()
                .map(|&(vx, vy)| vx.hypot(vy))
                .enumerate()
                .find(|(_, speed)| !(3.0 - 1e-4..=15.0 + 1e-4).contains(speed))
                .map(|(bird, speed)| (tick, bird, speed));
        }
    } else {
        for _ in 0..6 {
            step(ctx, &mut *state, 50);
        }
    }
    BoidsRun {
        polarisation: polarisation(&pairs(&graph_buffer(&entry, &*state, "lane_velocity_agents"))),
        outside,
    }
}

/// Band: every speed of the sheet's birds lies within the Keep between limits, 3 and 15, after each of 300 ticks, and
/// the polarisation of 20000 birds after the last tick lies above 0 and at most 0.95 on each seed, for the sheet and
/// the hand-lowered file. Both form several flocks heading their own ways. A run on the device is not repeatable at one
/// seed, and the flocks' headings can almost cancel, so no lower edge above 0 holds. Procedure: each bird's speed after
/// every tick, and the length of the mean of the birds' unit velocities after tick 300, at the defaults with 20000
/// birds, on seeds 1 to 3. Two planted errors sit outside. One steers by the flockmates' average velocity in place of
/// towards it, and every bird joins one flock near 1. The other leaves the Keep between line out of the card, and a
/// speed leaves the limits.
#[test]
fn the_boids_sheet_keeps_its_speeds_and_flocks_in_band() {
    let Some(ctx) = device("the_boids_sheet_keeps_its_speeds_and_flocks_in_band") else {
        return;
    };
    let band = |polarisation: f64| polarisation > 0.0 && polarisation <= 0.95;
    let steers_by = edited(
        sheet("boids.toml"),
        "mode = \"towards\"\nvalue = { result = \"l1\", part = \"average_velocity\" }",
        "mode = \"by\"\nvalue = { result = \"l1\", part = \"average_velocity\" }",
    );
    let no_limits = edited(
        &edited(
            sheet("boids.toml"),
            "[card.line.l6]\nkind = \"keep_between\"\nproperty = \"velocity\"\nminimum = { param = \"min_speed\" \
             }\nmaximum = { param = \"max_speed\" }\n",
            "",
        ),
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l7\"]",
    );
    for seed in SEEDS {
        let run = boids_run(&ctx, sheet("boids.toml"), seed, true);
        assert_eq!(
            run.outside, None,
            "a speed of the sheet at seed {seed} as (tick, bird, speed)"
        );
        assert!(
            band(run.polarisation),
            "the sheet's polarisation at seed {seed} is {}",
            run.polarisation
        );
        let found = boids_run(&ctx, HAND_BOIDS, seed, false).polarisation;
        assert!(band(found), "the hand-lowered polarisation at seed {seed} is {found}");
        let found = boids_run(&ctx, &steers_by, seed, false).polarisation;
        assert!(
            !band(found),
            "the planted error's polarisation at seed {seed} is {found}"
        );
        assert!(
            boids_run(&ctx, &no_limits, seed, true).outside.is_some(),
            "every speed stays within the limits at seed {seed} without the Keep between line"
        );
    }
}

/// Returns the opinion groups of `opinions`: the runs of adjacent bins 0.01 wide over 0 to 1 that each hold at least
/// 1% of the people.
fn opinion_groups(opinions: &[u32]) -> usize {
    let mut bins = [0usize; 100];
    for &word in opinions {
        let opinion = f32::from_bits(word);
        if (0.0..=1.0).contains(&opinion) {
            // The range check makes the truncation a bin index from 0 to 100.
            bins[((opinion * 100.0) as usize).min(99)] += 1;
        }
    }
    let least = opinions.len() / 100;
    let mut groups = 0;
    let mut inside = false;
    for count in bins {
        let held = count >= least;
        if held && !inside {
            groups += 1;
        }
        inside = held;
    }
    groups
}

/// Returns the opinion groups of bounded confidence `text` after 100 ticks at `seed`.
fn opinion_groups_of(ctx: &GpuContext, text: &str, seed: u64) -> usize {
    let entry = entry_of(text);
    let mut state = build_gpu(&entry, &params_of(&entry, &[]), Some(seed), ctx);
    step(ctx, &mut *state, 50);
    step(ctx, &mut *state, 50);
    opinion_groups(&graph_buffer(&entry, &*state, "lane_opinion_agents"))
}

/// Band: one opinion group after 100 ticks on each seed, for the sheet and the hand-lowered file, since with a
/// Confidence of 0.2 people who wander mix and their opinions converge. Procedure: the runs of adjacent opinion bins
/// 0.01 wide that each hold at least 1% of the 20000 people after tick 100, at the defaults, on seeds 1 to 3. The
/// planted error steers by the like-minded average in place of towards it, and every opinion leaves 0 to 1, so no group
/// is left.
#[test]
fn the_bounded_confidence_sheet_forms_its_opinion_groups_in_band() {
    let Some(ctx) = device("the_bounded_confidence_sheet_forms_its_opinion_groups_in_band") else {
        return;
    };
    let planted = edited(sheet("bounded_confidence.toml"), "mode = \"towards\"", "mode = \"by\"");
    for seed in SEEDS {
        assert_eq!(
            opinion_groups_of(&ctx, sheet("bounded_confidence.toml"), seed),
            1,
            "the sheet at seed {seed}"
        );
        assert_eq!(
            opinion_groups_of(&ctx, HAND_OPINIONS, seed),
            1,
            "the hand-lowered file at seed {seed}"
        );
        assert_ne!(
            opinion_groups_of(&ctx, &planted, seed),
            1,
            "the planted error at seed {seed}"
        );
    }
}

/// Returns the deliveries of the ants `text` at `seed` after 2000 and after 3000 ticks, read from its statistic
/// `label`.
fn deliveries(ctx: &GpuContext, text: &str, label: &str, seed: u64) -> (f64, f64) {
    let entry = entry_of(text);
    let mut state = build_gpu(&entry, &params_of(&entry, &[]), Some(seed), ctx);
    let mut found = [0.0; 2];
    for (slot, submissions) in found.iter_mut().zip([40, 20]) {
        for _ in 0..submissions {
            step(ctx, &mut *state, 50);
        }
        snapshot(ctx, &mut *state);
        *slot = stat(&state.stats(), label);
    }
    found.into()
}

/// Band: by tick 2000 the ants deliver at most 1000 times, and by tick 3000 at least 500 times, on each seed, for the
/// sheet and the hand-lowered file. Both forage at their defaults, 2000 ants from the nest, and both first need a
/// trail. Round 1's hand-lowered ants deliver nothing by tick 2000 on one of the seeds, so no lower edge at tick 2000
/// catches an error that stops deliveries, and the band adds one at tick 3000.
///
/// Procedure: the Deliveries statistic after ticks 2000 and 3000 at the defaults on seeds 1 to 3. The planted error
/// picks the neighbouring cell with the least To food as the Food way, so no ant follows the food trail, and
/// deliveries by tick 3000 stay far below 500.
#[test]
fn the_ants_sheet_delivers_food_in_band() {
    let Some(ctx) = device("the_ants_sheet_delivers_food_in_band") else {
        return;
    };
    let in_band = |(by_2000, by_3000): (f64, f64)| by_2000 <= 1000.0 && by_3000 >= 500.0;
    let planted = edited(
        sheet("ants.toml"),
        "pick = \"most\"\nranked_by = \"to_food\"",
        "pick = \"least\"\nranked_by = \"to_food\"",
    );
    for seed in SEEDS {
        let found = deliveries(&ctx, sheet("ants.toml"), "Deliveries", seed);
        assert!(in_band(found), "the sheet at seed {seed} delivers {found:?}");
        let found = deliveries(&ctx, HAND_ANTS, "Deliveries", seed);
        assert!(
            in_band(found),
            "the hand-lowered file at seed {seed} delivers {found:?}"
        );
        let found = deliveries(&ctx, &planted, "Deliveries", seed);
        assert!(!in_band(found), "the planted error at seed {seed} delivers {found:?}");
    }
}
