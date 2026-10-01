//! `--act ID@TICK`, the replayable form of the buttons the app draws.

use henad_core::action::{Fire, RefusedActions};

/// Rule under which a GPU benchmark rep's warm-up and timed runs fire actions, the one the CPU benchmark loop
/// follows.
pub const BENCH_FIRE: Fire = Fire::BeforeStep;

/// Prints a note for each action the model refused.
#[inline]
pub fn note_refused(refused: RefusedActions<'_>) {
    for action in refused {
        eprintln!("note: model refused action '{}' at tick {}", action.id, action.tick);
    }
}

#[cfg(test)]
mod tests {
    use super::BENCH_FIRE;
    use henad_core::action::{Fire, Schedule, Scheduled};

    fn schedule_at(ticks: &[u64]) -> Schedule {
        let entries = ticks
            .iter()
            .map(|&tick| Scheduled {
                index: 0,
                id: "act".to_owned(),
                tick,
            })
            .collect();
        Schedule::from_entries(entries)
    }

    /// Returns the ticks each run fires when runs of `counts` steps go back to back from tick 0.
    fn fire_runs(schedule: &Schedule, counts: &[u64], fire: Fire) -> Vec<Vec<u64>> {
        let mut start = 0;
        counts
            .iter()
            .map(|&count| {
                let fired = schedule.fire_ticks(start, count, fire);
                start += count;
                fired
            })
            .collect()
    }

    /// Checks that a GPU benchmark rep times the actions the CPU's timed loop times.
    ///
    /// A rep is a warm-up run and a timed run under [`BENCH_FIRE`], and the tick the rep stops on fires after the
    /// timer. Neither run fires that tick.
    #[test]
    fn a_timed_run_fires_the_ticks_the_cpu_loop_times() {
        let schedule = schedule_at(&(0..=8).collect::<Vec<u64>>());
        for warmup in 0..4 {
            for steps in 0..4 {
                let runs = fire_runs(&schedule, &[warmup, steps], BENCH_FIRE);
                let expected: [Vec<u64>; 2] = [(0..warmup).collect(), (warmup..warmup + steps).collect()];
                assert_eq!(runs, expected, "--warmup {warmup} --steps {steps}");
            }
        }
    }
}
