//! Checks that the foraging tutorial's results have the same bits on any number of threads.

use henad::engine::AgentModelState;
use henad::params::ParamValue;
use henad::runner::SimState as _;
use henad_tutorial::foraging::ForagingModel;

/// Returns the cell of every ant after 200 ticks on a pool of `threads` workers.
fn ant_cells(threads: usize) -> Vec<u32> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("rayon pool");
    pool.install(|| {
        // Three chunks of `ForagingModel::CHUNK`, so the chunks, each with its own seed, are split across workers.
        let mut state = AgentModelState::<ForagingModel>::from_params(&[
            ParamValue::U32(12_000),
            ParamValue::F32(200.0),
            ParamValue::F32(200.0),
        ]);
        for _ in 0..200 {
            state.step();
        }
        let lanes = state.lanes();
        lanes
            .pos_x
            .iter()
            .zip(&lanes.pos_y)
            .map(|(&x, &y)| y as u32 * 200 + x as u32)
            .collect()
    })
}

#[test]
fn results_do_not_depend_on_the_thread_count() {
    assert_eq!(ant_cells(1), ant_cells(7), "ant positions depend on the thread count");
}
