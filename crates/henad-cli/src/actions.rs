//! `--act ID@TICK`, the replayable form of the buttons the app draws.

use anyhow::{Context as _, Result, bail};
use henad_core::model::SimState;
use henad_models::registry::ModelEntry;

/// One `--act` entry, resolved against the model's declared actions.
pub struct Scheduled {
    pub index: usize,
    pub id: String,
    pub tick: u64,
}

/// Side of a step on which a run of GPU steps fires the actions due.
///
/// Each rule mirrors one CPU loop. Two runs back to back share the tick where the first stops and the
/// second starts, and under one rule only one of the two fires it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fire {
    /// Before the step that leaves a tick, as the CPU benchmark loop does. A run leaves the tick it
    /// stops on to the caller.
    BeforeStep,
    /// After the step that reaches a tick, as the CPU stats loop does, so a sample taken there sees
    /// the action. A run leaves the tick it starts on to the caller.
    AfterStep,
}

/// Rule under which a GPU benchmark rep's warm-up and timed runs fire actions, the one the CPU benchmark loop
/// follows.
pub const BENCH_FIRE: Fire = Fire::BeforeStep;

/// Every `--act` entry, in the order given.
///
/// Empty unless the user asked for one, and every call below leaves immediately when it is, so a
/// benchmark that names no action pays a single test per step.
pub struct Schedule {
    entries: Vec<Scheduled>,
}

impl Schedule {
    /// Resolves each `ID@TICK` against `entry`. Order is preserved, so two at one tick run as given.
    pub fn parse(raw: &[String], entry: &ModelEntry) -> Result<Self> {
        let mut entries = Vec::with_capacity(raw.len());
        for spec in raw {
            let (id, tick) = spec
                .split_once('@')
                .with_context(|| format!("bad --act '{spec}', expected ID@TICK"))?;
            let tick: u64 = tick.parse().with_context(|| format!("bad tick in --act '{spec}'"))?;
            let Some(index) = entry.action_descriptors.iter().position(|a| a.id == id) else {
                let known: Vec<&str> = entry.action_descriptors.iter().map(|a| a.id).collect();
                if known.is_empty() {
                    bail!("'{}' declares no actions", entry.id);
                }
                bail!("unknown action '{id}' for '{}' (has {})", entry.id, known.join(", "));
            };
            entries.push(Scheduled {
                index,
                id: id.to_owned(),
                tick,
            });
        }
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[Scheduled] {
        &self.entries
    }

    /// Highest tick anything is due at.
    pub fn last_tick(&self) -> Option<u64> {
        self.entries.iter().map(|a| a.tick).max()
    }

    /// Actions due exactly at `tick`, in the order given.
    pub fn due(&self, tick: u64) -> impl Iterator<Item = &Scheduled> {
        self.entries.iter().filter(move |a| a.tick == tick)
    }

    /// Returns the ticks a run of `count` steps from `start` fires at under `fire`, in increasing
    /// order and each once.
    ///
    /// [`Fire::BeforeStep`] covers `start..start + count` and [`Fire::AfterStep`] covers
    /// `start + 1..=start + count`. A run of no steps fires nothing under either rule.
    pub fn fire_ticks(&self, start: u64, count: u64, fire: Fire) -> Vec<u64> {
        let window = match fire {
            Fire::BeforeStep => start..start + count,
            Fire::AfterStep => start + 1..start + count + 1,
        };
        let mut ticks: Vec<u64> = self
            .entries
            .iter()
            .map(|a| a.tick)
            .filter(|t| window.contains(t))
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        ticks
    }

    /// Runs whatever is due at the state's current tick.
    ///
    /// The benchmark loop and the `--export` run call it before each step and once at the end, and the stats loop
    /// calls it once at the start and after each step. Either way every tick from 0 to the one the run stops on fires
    /// once.
    #[inline]
    pub fn run_due(&self, state: &mut dyn SimState) {
        if self.entries.is_empty() {
            return;
        }
        let tick = state.tick();
        for action in self.due(tick) {
            if !state.act(action.index) {
                eprintln!("note: model refused action '{}' at tick {tick}", action.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BENCH_FIRE, Fire, Schedule, Scheduled};

    fn schedule_at(ticks: &[u64]) -> Schedule {
        let entries = ticks
            .iter()
            .map(|&tick| Scheduled {
                index: 0,
                id: "act".to_owned(),
                tick,
            })
            .collect();
        Schedule { entries }
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

    /// Checks that runs placed back to back fire a shared tick once. A run used to fire both of its ends,
    /// and the tick two runs shared fired twice. A GPU stats export then randomised twice at `--stats-every 1`
    /// and once at `--stats-every 3`.
    ///
    /// Under the before-step rule the caller fires tick 40, and under the after-step rule tick 0.
    #[test]
    fn back_to_back_runs_fire_each_tick_once() {
        // Two actions at tick 10 fire in one stop. Tick 41 is past the end.
        let schedule = schedule_at(&[0, 7, 10, 10, 14, 39, 40, 41]);
        let splits: [&[u64]; 4] = [&[40], &[10, 10, 10, 10], &[7, 7, 7, 7, 7, 5], &[0, 13, 0, 27, 0]];
        for split in splits {
            let before = fire_runs(&schedule, split, Fire::BeforeStep).concat();
            assert_eq!(before, [0, 7, 10, 14, 39], "before the step, runs of {split:?}");
            let after = fire_runs(&schedule, split, Fire::AfterStep).concat();
            assert_eq!(after, [7, 10, 14, 39, 40], "after the step, runs of {split:?}");
        }
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
