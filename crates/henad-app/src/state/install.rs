//! Installation of model code in the running app. The set's entries are replaced by id, and the panels, the Sweep tab
//! and the Results tab are carried over to the replacements.
//!
//! A host installs a model set when a model's code changes. The live simulation keeps the entry it was built from. A
//! run still at tick 0 is rebuilt at once under the replacement, and any other run waits on the Rebuild model modal.
//! A rebuild starts the run again from tick 0.

use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::simulation::{RunSetup, SetupError};
use henad_core::action::{ActionDescriptor, Schedule, Scheduled};
use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::value::check_value;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};

use super::{AppState, setup_message, too_large_message};

/// Rebuild of the loaded model under replaced code. The Rebuild model modal asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRebuild {
    /// Id of the loaded model.
    pub model: String,
    /// Number of parameters of the new code that take their default in place of the run's value: parameters new to
    /// the code, and parameters whose running value the new code rejects.
    pub defaulted_params: usize,
    /// Number of the run's scheduled actions that the rebuild drops, for actions the new code does not declare.
    pub dropped_actions: usize,
}

/// Values carried from one version of a model's parameters to another by parameter id, as [`remap_values`] returns
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct RemappedValues {
    /// One value per parameter of the new version, in its order.
    pub values: Vec<ParamValue>,
    /// Number of parameters of the new version set to their default: parameters the previous version lacks, and
    /// parameters whose value the new version rejects.
    pub defaulted: usize,
}

/// Scheduled actions carried from one version of a model to another by action id, as [`remap_schedule`] returns them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemappedSchedule {
    /// Entries of the new version's actions, in the order given.
    pub schedule: Schedule,
    /// Number of entries dropped for an action the new version does not declare.
    pub dropped: usize,
}

impl AppState {
    /// Puts each entry of `models` in place of the set's entry with its id, or adds it at the end, and carries the
    /// app over to the replacements.
    ///
    /// A replacement with the source and the declarations of the entry it replaces changes nothing else. For each
    /// other replaced model, the app:
    ///
    /// - keeps the values, seed and scheduled actions of a selected model whose parameters, stats and actions are
    ///   unchanged. A selected model whose declarations changed gets its values carried by parameter id and its actions
    ///   by action id, as [`remap_values`] and [`remap_schedule`] carry them.
    /// - forgets the model's stat columns in the Sweep tab, and its draft when its declarations changed.
    /// - compares the results of the model with the replacement, so a replay of a run of other code warns.
    ///
    /// When the loaded model is replaced by other code, a run still at tick 0, neither playing nor running to a tick,
    /// is rebuilt at once. Any other run keeps running under the entry it was built from, and
    /// [`Self::pending_rebuild`] opens the Rebuild model modal. So does a run at tick 0 that the new code cannot
    /// rebuild, as [`Self::rebuild_refusal`] finds.
    ///
    /// Note that the caller compiles new code before it installs it. A device error at build time ends the loaded run,
    /// as it does on Build.
    pub fn install_models(&mut self, models: &ModelSet) {
        let mut loaded_replaced = false;
        for entry in models {
            let id = entry.id().to_owned();
            let previous = match self.models.replace(entry.clone()) {
                Ok(Some(previous)) => previous,
                // Results read while the set lacked the id compare their runs with the new entry.
                Ok(None) => {
                    let model = self.lookup(&id).cloned();
                    self.results.model_replaced(&id, model.as_ref());
                    continue;
                }
                // Every entry of a set has passed the id check that `replace` makes.
                Err(error) => {
                    log::error!("Model not installed: {error}");
                    continue;
                }
            };
            let Some(current) = self.models.get(&id).cloned() else {
                continue;
            };
            if same_code(&previous, &current) {
                continue;
            }
            let declarations_changed = schema_hash(&previous.schema()) != schema_hash(&current.schema());
            if self.selected_model.as_deref() == Some(id.as_str()) && declarations_changed {
                self.remap_selection(&previous, &current);
            }
            self.sweep.forget_model(&id, declarations_changed);
            let model = self.lookup(&id).cloned();
            self.results.model_replaced(&id, model.as_ref());
            loaded_replaced |= self.loaded_id() == Some(id.as_str());
        }
        if loaded_replaced {
            self.settle_loaded();
        }
    }

    /// Carries the values, pending marks and scheduled actions of the selection from `previous` to `current`, two
    /// versions of the selected model.
    fn remap_selection(&mut self, previous: &ModelEntry, current: &ModelEntry) {
        let remapped = remap_values(
            previous.param_descriptors(),
            &self.param_values,
            current.param_descriptors(),
        );
        self.pending_reload = current
            .param_descriptors()
            .iter()
            .map(|descriptor| {
                previous
                    .param_index(descriptor.id())
                    .and_then(|index| self.pending_reload.get(index))
                    .copied()
                    .unwrap_or(false)
            })
            .collect();
        self.param_values = remapped.values;
        self.schedule = remap_schedule(&self.schedule, current.action_descriptors()).schedule;
        self.schedule_action_input = 0;
    }

    /// Rebuilds the loaded model under the set's entry for its id once its code differs from the loaded code: at once
    /// while the run is at tick 0 and the new code can rebuild it, and otherwise after the Rebuild model modal.
    ///
    /// The modal counts what the new code does not take from the run, its values and its scheduled actions.
    pub fn settle_loaded(&mut self) {
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        let Some(current) = self.models.get(loaded.id()).cloned() else {
            return;
        };
        if current.source() == loaded.source() {
            self.pending_rebuild = None;
            return;
        }
        if self.run_at_start() && self.rebuild_refusal().is_none() {
            self.rebuild_loaded();
            return;
        }
        let values = remap_values(
            loaded.param_descriptors(),
            &self.loaded_values,
            current.param_descriptors(),
        );
        let schedule = remap_schedule(&self.loaded_schedule, current.action_descriptors());
        self.pending_rebuild = Some(PendingRebuild {
            model: loaded.id().to_owned(),
            defaulted_params: values.defaulted,
            dropped_actions: schedule.dropped,
        });
    }

    /// Returns whether the loaded run is at tick 0 and about to stay there: no snapshot past tick 0, not playing, and
    /// no run to a tick pending.
    ///
    /// The snapshot lags the run. A run to a tick or a play started this frame has no snapshot past tick 0 yet.
    pub fn run_at_start(&self) -> bool {
        !self.sim_running
            && self.run_to_target.is_none()
            && self.snapshot.as_ref().is_none_or(|snapshot| snapshot.tick == 0)
    }

    /// Returns the reason the loaded model cannot be rebuilt under the set's entry for its id, or `None` when it can
    /// or no model is loaded.
    ///
    /// The selected model is refused as Build refuses it ([`Self::build_refusal`]). A model loaded while another one
    /// is selected is refused when the set's entry rejects the values, seed and actions it runs with, carried by id, or
    /// the device cannot hold the model at those values.
    pub fn rebuild_refusal(&self) -> Option<String> {
        let loaded = self.loaded.as_ref()?;
        if self.selected_model.as_deref() == Some(loaded.id()) {
            return self.build_refusal();
        }
        let current = self.models.get(loaded.id())?;
        match self.carried_setup(loaded, current) {
            Ok(setup) => {
                let shortfalls = current.shortfalls(setup.values(), &self.runtime.granted);
                (!shortfalls.is_empty()).then(|| too_large_message(&shortfalls))
            }
            Err(error) => Some(setup_message(&error)),
        }
    }

    /// Returns the setup of `current`, the set's entry for the loaded model's id, from the values, seed and actions
    /// that `loaded` runs with, carried by id.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError`] when `current` rejects the carried values or actions.
    fn carried_setup(&self, loaded: &ModelEntry, current: &ModelEntry) -> Result<RunSetup, SetupError> {
        let values = remap_values(
            loaded.param_descriptors(),
            &self.loaded_values,
            current.param_descriptors(),
        );
        let schedule = remap_schedule(&self.loaded_schedule, current.action_descriptors());
        RunSetup::from_parts(current, &values.values, self.loaded_seed, schedule.schedule)
    }

    /// Tears down the loaded model's run and builds the model again from tick 0, under the set's entry for its id.
    ///
    /// The selected model builds as Build builds it, from the values, seed and actions of the next build. A model
    /// loaded while another one is selected builds from the values, seed and actions it runs with, carried to the
    /// set's entry by id. Does nothing while [`Self::rebuild_refusal`] returns a reason, and the run keeps going.
    pub fn rebuild_loaded(&mut self) {
        if let Some(reason) = self.rebuild_refusal() {
            log::warn!("Rebuild refused: {reason}");
            return;
        }
        self.pending_rebuild = None;
        let Some(loaded) = self.loaded.clone() else {
            return;
        };
        if self.selected_model.as_deref() == Some(loaded.id()) {
            self.reset_simulation();
            return;
        }
        let Some(current) = self.models.get(loaded.id()).cloned() else {
            return;
        };
        match self.carried_setup(&loaded, &current) {
            Ok(setup) => self.load(Some(setup)),
            Err(error) => log::warn!("Rebuild refused: {}", setup_message(&error)),
        }
    }
}

/// Returns whether `held` and `current`, two entries of one model id, run the same code with the same declarations.
///
/// Two entries with equal sources run the same code when they also declare the same parameters, stats and actions.
pub fn same_code(held: &ModelEntry, current: &ModelEntry) -> bool {
    held.source() == current.source() && schema_hash(&held.schema()) == schema_hash(&current.schema())
}

/// Returns `values`, one per parameter of `previous` in its order, carried to the parameters of `current` by
/// parameter id.
///
/// A choice is carried by the name of its option. A parameter of `current` that `previous` lacks, or whose carried
/// value it rejects, takes its default value.
pub fn remap_values(
    previous: &[ParamDescriptor],
    values: &[ParamValue],
    current: &[ParamDescriptor],
) -> RemappedValues {
    let mut defaulted = 0;
    let values = current
        .iter()
        .map(|descriptor| {
            previous
                .iter()
                .zip(values)
                .find(|(previous_descriptor, _)| previous_descriptor.id() == descriptor.id())
                .and_then(|(previous_descriptor, value)| {
                    carry_value(previous_descriptor.kind(), value, descriptor.kind())
                })
                .unwrap_or_else(|| {
                    defaulted += 1;
                    descriptor.kind().default_value()
                })
        })
        .collect();
    RemappedValues { values, defaulted }
}

/// Returns `value`, a value of a parameter of kind `previous`, as a value of a parameter of kind `current`, or `None`
/// when `current` rejects it.
///
/// A choice keeps the name of its option, at that option's index in `current`.
fn carry_value(previous: &ParamKind, value: &ParamValue, current: &ParamKind) -> Option<ParamValue> {
    if let (
        ParamKind::Choice {
            options: previous_options,
            ..
        },
        ParamValue::Choice(index),
        ParamKind::Choice {
            options: current_options,
            ..
        },
    ) = (previous, value, current)
    {
        let name = previous_options.get(*index)?;
        return current_options.position(name).map(ParamValue::Choice);
    }
    check_value(current, value).is_ok().then(|| value.clone())
}

/// Returns the entries of `schedule` carried to `actions` by action id, each at the index of its action, with the
/// number of entries dropped for an action that `actions` lacks.
pub fn remap_schedule(schedule: &Schedule, actions: &[ActionDescriptor]) -> RemappedSchedule {
    let entries: Vec<Scheduled> = schedule
        .entries()
        .iter()
        .filter_map(|scheduled| {
            let index = actions.iter().position(|action| action.id() == scheduled.id)?;
            Some(Scheduled {
                index,
                id: scheduled.id.clone(),
                tick: scheduled.tick,
            })
        })
        .collect();
    let dropped = schedule.entries().len() - entries.len();
    RemappedSchedule {
        schedule: Schedule::from_entries(entries),
        dropped,
    }
}
