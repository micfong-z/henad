//! Rates on the device: the known-answer gates of a sheet with rates, run by the GPU engine.
//!
//! The SIR starter states its changes as rates per day, so its epidemic curve in days converges as the tick halves,
//! while round 1's chance SIR at fixed chances per tick does not. The SIRD sheet,
//! `tests/fixtures/sheets/sird.toml`, leaves Infected at two rates of 0.05 per day each. Its two lines compete, and half
//! of the removed cells are Dead at any tick length, where the hand-lowered `lowered/rates/sird_higher_wins.toml`
//! lets the higher line win and gives 0.4134 at a tick of 7 days.

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::params::ParamValue;
use henad_core::view::StatValue;

use super::graph_grid_parity::{build_gpu, snapshot, stat, step};
use super::sheet_grid::{device, device_params, entry_of, fixture_text};

/// Returns the text of the sheet starter `file_name`.
fn starter_text(file_name: &str) -> &'static str {
    henad_graph::sheet::starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .map(|starter| starter.text)
        .unwrap_or_else(|| panic!("no sheet starter {file_name}"))
}

/// Side of the worlds of the tick-halving runs.
const HALVING_SIDE: u32 = 256;

/// Days of the tick-halving runs.
const HALVING_DAYS: u32 = 80;

/// Ticks per day of each tick-halving run: ticks of 1, 0.5, 0.25 and 0.125 days, then the reference of 0.0625 days.
const TICKS_PER_DAY: [u32; 5] = [1, 2, 4, 8, 16];

/// Returns the share of the cells that are Infected at the start of each day of a run of `entry` at `params` and
/// `seed`, `ticks_per_day` ticks a day.
fn infected_by_day(
    ctx: &GpuContext,
    entry: &ModelEntry,
    params: &[ParamValue],
    seed: u64,
    ticks_per_day: u32,
) -> Vec<f64> {
    let cells = f64::from(HALVING_SIDE * HALVING_SIDE);
    let mut state = build_gpu(entry, params, Some(seed), ctx);
    let mut shares = Vec::with_capacity(HALVING_DAYS as usize + 1);
    for day in 0..=HALVING_DAYS {
        snapshot(ctx, &mut *state);
        shares.push(stat(&state.stats(), "Infected") / cells);
        if day < HALVING_DAYS {
            step(ctx, &mut *state, ticks_per_day);
        }
    }
    shares
}

/// Returns the largest gap, over the seeds and the days, between the infected share of each tick length and that of
/// the shortest tick at the same seed, one gap per tick length but the shortest.
///
/// `runs` holds the run at each tick length of [`TICKS_PER_DAY`] for one seed, at that seed.
fn largest_gaps(runs: &[Vec<Vec<f64>>]) -> Vec<f64> {
    let mut gaps = vec![0.0f64; TICKS_PER_DAY.len() - 1];
    for seed in runs {
        let reference = &seed[TICKS_PER_DAY.len() - 1];
        for (gap, run) in gaps.iter_mut().zip(seed) {
            for (share, known) in run.iter().zip(reference) {
                *gap = gap.max((share - known).abs());
            }
        }
    }
    gaps
}

/// Returns whether each gap of `gaps` is at most 0.8 of the one before.
fn halves(gaps: &[f64]) -> bool {
    gaps.windows(2).all(|pair| pair[1] <= 0.8 * pair[0])
}

/// The rates SIR at beta = 0.3 and gamma = 0.1 per day on 256 by 256 cells for 80 days, at seeds 1 to 3: the largest
/// gap in infected share against a tick of 0.0625 days falls with each halving of the tick from 1 day, each gap at
/// most 0.8 of the one before. The chance SIR at its one-day chances per tick, read with the same ticks per day, fails
/// the rule, so the test tells the two apart.
#[test]
fn the_rates_sir_converges_as_the_tick_halves_and_the_chance_sir_does_not() {
    let Some(ctx) = device("the_rates_sir_converges_as_the_tick_halves_and_the_chance_sir_does_not") else {
        return;
    };
    let rates = entry_of(starter_text("sir.toml"));
    let chances = entry_of(&fixture_text("sheets/sir_chance.toml"));
    let one_day = [
        ("infection_chance", 1.0 - (-0.3f32).exp()),
        ("recovery_chance", 1.0 - (-0.1f32).exp()),
    ];
    let mut rate_runs = Vec::new();
    let mut chance_runs = Vec::new();
    for seed in 1..=3 {
        let mut rate_seed = Vec::new();
        let mut chance_seed = Vec::new();
        for ticks_per_day in TICKS_PER_DAY {
            let tick_length = 1.0 / ticks_per_day as f32;
            let params = device_params(&rates, HALVING_SIDE, &[("tick_length", tick_length)]);
            rate_seed.push(infected_by_day(&ctx, &rates, &params, seed, ticks_per_day));
            let params = device_params(&chances, HALVING_SIDE, &one_day);
            chance_seed.push(infected_by_day(&ctx, &chances, &params, seed, ticks_per_day));
        }
        rate_runs.push(rate_seed);
        chance_runs.push(chance_seed);
    }
    let rate_gaps = largest_gaps(&rate_runs);
    let chance_gaps = largest_gaps(&chance_runs);
    log::info!("rate gaps {rate_gaps:?}, chance gaps {chance_gaps:?}");
    assert!(halves(&rate_gaps), "the rate gaps {rate_gaps:?} do not halve");
    assert!(!halves(&chance_gaps), "the chance gaps {chance_gaps:?} halve");
    assert!(rate_gaps[0] > 0.05, "a tick of 1 day is coarse: {rate_gaps:?}");
}

/// Returns the share of the removed cells that are Dead at the end of the epidemic of `entry` at `params` and `seed`.
fn dead_share(ctx: &GpuContext, entry: &ModelEntry, params: &[ParamValue], seed: u64) -> f64 {
    let mut state = build_gpu(entry, params, Some(seed), ctx);
    for _ in 0..100 {
        snapshot(ctx, &mut *state);
        let stats = state.stats();
        if stat(&stats, "Infected") == 0.0 {
            let dead = stat(&stats, "Dead");
            return dead / (dead + stat(&stats, "Recovered"));
        }
        step(ctx, &mut *state, 10);
    }
    panic!("{}: the epidemic outlasted 1000 ticks", entry.id());
}

/// The SIRD sheet at a tick of 7 days on 512 by 512 cells, at seeds 1 to 3: the share of the removed cells that are
/// Dead lies within 0.01 of the competing-risks split, 0.5, and the hand-lowered model in which the higher line wins
/// lies within 0.01 of its own prediction, 0.4134.
#[test]
fn the_sird_sheet_splits_its_removed_cells_by_its_rates() {
    let Some(ctx) = device("the_sird_sheet_splits_its_removed_cells_by_its_rates") else {
        return;
    };
    let side = 512;
    let sheet = entry_of(&fixture_text("sheets/sird.toml"));
    let higher_wins = entry_of(&fixture_text("lowered/rates/sird_higher_wins.toml"));
    for seed in 1..=3 {
        let competing = dead_share(
            &ctx,
            &sheet,
            &device_params(&sheet, side, &[("tick_length", 7.0)]),
            seed,
        );
        let contrast = dead_share(
            &ctx,
            &higher_wins,
            &device_params(&higher_wins, side, &[("tick_length", 7.0)]),
            seed,
        );
        log::info!("seed {seed}: competing {competing}, higher wins {contrast}");
        assert!(
            (competing - 0.5).abs() <= 0.01,
            "seed {seed}: {competing} of the removed are Dead"
        );
        assert!(
            (contrast - 0.4134).abs() <= 0.01,
            "seed {seed}: {contrast} of the removed are Dead"
        );
    }
}

/// Two runs of the rates SIR at one seed and a tick of half a day write identical statistics every tick.
#[test]
fn two_runs_at_one_seed_write_identical_statistics() {
    let Some(ctx) = device("two_runs_at_one_seed_write_identical_statistics") else {
        return;
    };
    let entry = entry_of(starter_text("sir.toml"));
    let params = device_params(&entry, 64, &[("tick_length", 0.5)]);
    let run = || {
        let mut state = build_gpu(&entry, &params, Some(5), &ctx);
        let mut written: Vec<Vec<f64>> = Vec::new();
        for _ in 0..100 {
            snapshot(&ctx, &mut *state);
            written.push(
                state
                    .stats()
                    .iter()
                    .map(|entry| match entry.value() {
                        StatValue::Scalar(value) => *value,
                        other => panic!("{other:?}"),
                    })
                    .collect(),
            );
            step(&ctx, &mut *state, 1);
        }
        written
    };
    let first = run();
    assert_eq!(first, run());
    assert!(first.iter().any(|stats| stats[1] > first[0][1]), "the epidemic grows");
}
