//! The node graph copy of a sheet on the device: the converted Game of Life runs tick for tick like the sheet.

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_core::params::ParamValue;
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};
use henad_graph::sheet::{ConvertOptions, SheetFile, convert, starters};

use super::graph_grid_parity::{build_gpu, press, snapshot, stat, step};

/// Returns the entry the model file of `text` builds.
fn entry_of(text: &str) -> ModelEntry {
    ModelFile::parse(text)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Returns the build parameters of `entry` at `side` by `side` cells, every other parameter at its default.
fn params_at(entry: &ModelEntry, side: u32) -> Vec<ParamValue> {
    let mut params: Vec<ParamValue> = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind().default_value())
        .collect();
    params[0] = ParamValue::U32(side);
    params[1] = ParamValue::U32(side);
    params
}

/// Runs `entry`, Game of Life or a copy of it, for 200 ticks at seed 7 on a wrapping world 128 cells wide, pressing
/// Randomise at tick 60, Clear at tick 150 and Randomise at tick 160. Returns the status lane's words at tick 0 and
/// after each tick, and the Alive count after the last tick.
fn life_run(ctx: &henad_compute::gpu::GpuContext, entry: &ModelEntry) -> (Vec<Vec<u32>>, f64) {
    let randomise = entry.action_index("randomise").expect("the Randomise button");
    let clear = entry.action_index("clear").expect("the Clear button");
    let mut state = build_gpu(entry, &params_at(entry, 128), Some(7), ctx);
    let words = |state: &dyn GpuSimState| state.read_buffer(0).expect("the status lane");
    let mut run = vec![words(&*state)];
    for tick in 0..200u32 {
        match tick {
            60 | 160 => press(ctx, &mut *state, randomise),
            150 => press(ctx, &mut *state, clear),
            _ => {}
        }
        step(ctx, &mut *state, 1);
        run.push(words(&*state));
    }
    snapshot(ctx, &mut *state);
    let alive = stat(&state.stats(), "Alive");
    (run, alive)
}

#[test]
fn the_converted_game_of_life_runs_tick_for_tick_like_the_sheet_on_the_device() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_converted_game_of_life_runs_tick_for_tick_like_the_sheet_on_the_device: no adapter");
        return;
    };
    let text = starters()[0].text;
    let sheet = SheetFile::parse(text).unwrap_or_else(|error| panic!("{error}"));
    let options = ConvertOptions::new(sheet.document(), "2026-11-07", |_, _| false);
    let copy = convert(&sheet, &options).unwrap_or_else(|error| panic!("{error}"));
    let copy_entry = entry_of(&copy.to_toml());
    assert_eq!(copy_entry.id(), "sheet_game_of_life_expert");
    let (expected, expected_alive) = life_run(&ctx, &entry_of(text));
    let alive = |words: &[u32]| words.iter().filter(|&&word| word == 1).count();
    assert!(alive(&expected[150]) > 0, "cells live before Clear");
    assert_eq!(alive(&expected[151]), 0, "Clear leaves no cell alive");
    assert!(alive(&expected[200]) > 0, "Randomise brings cells back");
    let (actual, actual_alive) = life_run(&ctx, &copy_entry);
    for (tick, (copy_words, sheet_words)) in actual.iter().zip(&expected).enumerate() {
        assert!(
            copy_words == sheet_words,
            "the copy departs from the sheet at tick {tick}"
        );
    }
    assert_eq!(actual_alive, expected_alive);
    assert_eq!(expected_alive, alive(&expected[200]) as f64);
}
