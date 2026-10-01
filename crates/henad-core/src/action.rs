//! One-off steps a model offers the user, run between ticks.

use std::fmt;
use std::num::ParseIntError;

use crate::authoring::primitives::rng::mix_seed;
use crate::model::SimState;

/// An action a model declares.
///
/// The Parameters panel draws a button per entry and `henad-cli` names one with `--act`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDescriptor {
    /// Stable name, what `--act` matches on.
    pub id: &'static str,
    /// Button label.
    pub label: &'static str,
}

impl ActionDescriptor {
    pub const fn new(id: &'static str, label: &'static str) -> Self {
        Self { id, label }
    }
}

/// Domain separator for the action stream.
const ACTION_SALT: u64 = 0x00AC_7104_5EED_0001;

/// Where a state's action stream starts, from the seed the state was built with.
///
/// Kept apart from the tick stream. Otherwise a press draws the numbers the next tick would have.
pub fn action_seed(seed: Option<u64>) -> u64 {
    mix_seed(seed.unwrap_or(0) ^ ACTION_SALT)
}

/// Declares a model's actions and their indices in one place.
///
/// The index is the declaration's position, so it is derived rather than written down. Expands at
/// module scope, next to the impl that forwards `ACTIONS` to `ACTION_SPECS`.
///
/// ```ignore
/// actions! {
///     const RANDOMISE = ActionDescriptor::new("randomise", "Randomise");
///     const CLEAR = ActionDescriptor::new("clear", "Clear");
/// }
/// ```
#[macro_export]
macro_rules! actions {
    ($($(#[$meta:meta])* $vis:vis const $name:ident = $descriptor:expr;)+) => {
        $crate::__indices!(0usize, $([$(#[$meta])* $vis $name],)+);

        /// This model's actions, in index order.
        const ACTION_SPECS: &[$crate::__macro_support::ActionDescriptor] = &[$($descriptor),+];
    };
}

/// One `--act` entry, resolved against the model's declared actions.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Entries a state refused to run, in the order given.
pub type RefusedActions<'a> = Vec<&'a Scheduled>;

/// An `--act` entry that cannot be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// An entry not of the form `ID@TICK`.
    BadEntry { raw: String },
    /// An entry whose tick is not a whole number, for the reason in `source`.
    BadTick { raw: String, source: ParseIntError },
    /// An entry for a model that declares no actions.
    NoActions { model: String },
    /// An id `model` does not declare. `known` lists the ids it does.
    UnknownAction {
        id: String,
        model: String,
        known: Vec<&'static str>,
    },
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadEntry { raw } => write!(f, "bad --act '{raw}', expected ID@TICK"),
            Self::BadTick { raw, .. } => write!(f, "bad tick in --act '{raw}'"),
            Self::NoActions { model } => write!(f, "'{model}' declares no actions"),
            Self::UnknownAction { id, model, known } => {
                write!(f, "unknown action '{id}' for '{model}' (has {})", known.join(", "))
            }
        }
    }
}

impl std::error::Error for ScheduleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BadTick { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Actions to run at set ticks, in the order given.
///
/// Empty unless one was asked for, and every call below leaves immediately when it is, so a loop
/// with no actions pays a single test per step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schedule {
    entries: Vec<Scheduled>,
}

impl Schedule {
    /// Resolves each `ID@TICK` against `actions`, the actions of model `model_id`. Order is preserved, so two at
    /// one tick run as given.
    ///
    /// # Errors
    ///
    /// Returns [`ScheduleError`] for an entry that does not read as `ID@TICK`, or names no action in `actions`.
    pub fn parse(raw: &[String], model_id: &str, actions: &[ActionDescriptor]) -> Result<Self, ScheduleError> {
        let mut entries = Vec::with_capacity(raw.len());
        for spec in raw {
            let (id, tick) = spec
                .split_once('@')
                .ok_or_else(|| ScheduleError::BadEntry { raw: spec.clone() })?;
            let tick = tick.parse::<u64>().map_err(|source| ScheduleError::BadTick {
                raw: spec.clone(),
                source,
            })?;
            let Some(index) = actions.iter().position(|action| action.id == id) else {
                let known: Vec<&'static str> = actions.iter().map(|action| action.id).collect();
                if known.is_empty() {
                    return Err(ScheduleError::NoActions {
                        model: model_id.to_owned(),
                    });
                }
                return Err(ScheduleError::UnknownAction {
                    id: id.to_owned(),
                    model: model_id.to_owned(),
                    known,
                });
            };
            entries.push(Scheduled {
                index,
                id: id.to_owned(),
                tick,
            });
        }
        Ok(Self { entries })
    }

    /// Builds a schedule from resolved entries. Two due at one tick run in the order given.
    pub fn from_entries(entries: Vec<Scheduled>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[Scheduled] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Highest tick anything is due at.
    pub fn last_tick(&self) -> Option<u64> {
        self.entries.iter().map(|a| a.tick).max()
    }

    /// Returns the earliest tick after `tick` that anything is due at.
    pub fn next_due_after(&self, tick: u64) -> Option<u64> {
        self.entries
            .iter()
            .map(|entry| entry.tick)
            .filter(|&due| due > tick)
            .min()
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
            Fire::BeforeStep => start..start.saturating_add(count),
            Fire::AfterStep => start.saturating_add(1)..start.saturating_add(count).saturating_add(1),
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

    /// Runs whatever is due at the state's current tick, and returns the entries the state refused.
    ///
    /// A loop calls it either before each step and once at the end, or once at the start and after each step. Either
    /// way every tick from 0 to the one the run stops on fires once.
    #[inline]
    #[must_use = "a refused action is reported only through the returned entries"]
    pub fn run_due(&self, state: &mut dyn SimState) -> RefusedActions<'_> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        let tick = state.tick();
        let mut refused = Vec::new();
        for action in self.due(tick) {
            if !state.act(action.index) {
                refused.push(action);
            }
        }
        refused
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionDescriptor, Fire, Schedule, Scheduled};

    /// The macro has to expand in function scope as well as module scope (C-ANYWHERE).
    #[test]
    fn actions_macro_numbers_entries_in_declaration_order() {
        crate::actions! {
            const RANDOMISE = crate::action::ActionDescriptor::new("randomise", "Randomise");
            /// An entry can carry a doc comment.
            const CLEAR = crate::action::ActionDescriptor::new("clear", "Clear");
        }
        assert_eq!((RANDOMISE, CLEAR), (0, 1), "indices follow declaration order");
        assert_eq!(ACTION_SPECS.len(), 2);
        assert_eq!(ACTION_SPECS[CLEAR].label, "Clear");
    }

    /// Two states built from one seed must agree, and two from different seeds must not.
    #[test]
    fn the_action_stream_is_seeded_and_apart_from_the_tick_stream() {
        use crate::action::action_seed;
        assert_eq!(action_seed(Some(7)), action_seed(Some(7)));
        assert_ne!(action_seed(Some(7)), action_seed(Some(8)));
        assert_ne!(action_seed(Some(7)), crate::authoring::primitives::rng::mix_seed(7));
        assert_ne!(action_seed(None), 0, "an unseeded state still needs a usable state");
    }

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

    #[test]
    fn the_next_due_tick_is_strictly_after_the_one_given() {
        let schedule = schedule_at(&[40, 7, 10, 10]);
        assert_eq!(schedule.next_due_after(0), Some(7));
        assert_eq!(schedule.next_due_after(7), Some(10), "a tick is not due after itself");
        assert_eq!(schedule.next_due_after(10), Some(40));
        assert_eq!(schedule.next_due_after(40), None);
        assert_eq!(Schedule::default().next_due_after(0), None);
        assert!(Schedule::default().is_empty());
    }

    #[test]
    fn an_entry_resolves_to_its_declared_action() {
        let actions = [
            ActionDescriptor::new("randomise", "Randomise"),
            ActionDescriptor::new("clear", "Clear"),
        ];
        let raw = ["clear@5", "randomise@2", "clear@5"].map(str::to_owned);
        let schedule = Schedule::parse(&raw, "life", &actions).expect("declared actions");
        let resolved: Vec<(usize, u64)> = schedule
            .entries()
            .iter()
            .map(|entry| (entry.index, entry.tick))
            .collect();
        assert_eq!(resolved, [(1, 5), (0, 2), (1, 5)], "in the order given");

        let refuse = |raw: &str, actions: &[ActionDescriptor]| {
            Schedule::parse(&[raw.to_owned()], "life", actions)
                .expect_err("refused")
                .to_string()
        };
        assert_eq!(refuse("clear", &actions), "bad --act 'clear', expected ID@TICK");
        assert_eq!(refuse("clear@soon", &actions), "bad tick in --act 'clear@soon'");
        let bad_tick = Schedule::parse(&["clear@soon".to_owned()], "life", &actions).expect_err("refused");
        let source = std::error::Error::source(&bad_tick).map(ToString::to_string);
        assert_eq!(
            source.as_deref(),
            Some("invalid digit found in string"),
            "the tick's parse error is the source"
        );
        assert_eq!(refuse("clear@5", &[]), "'life' declares no actions");
        assert_eq!(
            refuse("reset@5", &actions),
            "unknown action 'reset' for 'life' (has randomise, clear)"
        );
    }
}
