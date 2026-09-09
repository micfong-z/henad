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

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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

    /// The next tick at or after `from` that anything is due at.
    pub fn next_due(&self, from: u64) -> Option<u64> {
        self.entries.iter().map(|a| a.tick).filter(|&t| t >= from).min()
    }

    /// Runs whatever is due at the state's current tick.
    ///
    /// Called before each step and once at the end, so a tick the run stops on still fires.
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
