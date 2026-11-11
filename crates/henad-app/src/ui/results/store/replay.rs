//! Replays of the runs of a results store, and the command line that reproduces one.

use std::collections::BTreeMap;
use std::sync::Arc;

use henad_compute::entry::ModelEntry;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::{Config, Plan};
use henad_core::explore::replay::Replay;
use henad_core::explore::value::{format_value, parse_value};
use henad_core::provenance::CodeOrigin;
use henad_explore::output::manifest::RecordedBuild;
use henad_explore::search_run::SearchPlan;

use crate::ui::text::describe_error;

use super::ResultsStore;
use super::axes::{ConfigTexts, plan_texts};
use super::recorded::RecordedSpec;

/// Reason no command line replays the runs of a model file.
pub const MODEL_FILE_COMMAND: &str =
    "Runs of a model file replay on the command line with --model-file PATH. Save the model file to replay them there.";

/// Plan used to replay the runs of a store.
#[derive(Debug, Clone)]
pub(super) enum ReplayPlan {
    Sweep(Arc<Plan>),
    /// A search, whose runs replay from the configs of their candidates.
    Search(Arc<SearchPlan>),
}

impl ReplayPlan {
    /// Plan of the settings every run shares, and of a sweep's configs.
    pub(super) fn base(&self) -> &Plan {
        match self {
            Self::Sweep(plan) => plan,
            Self::Search(search_plan) => search_plan.base(),
        }
    }
}

impl ResultsStore {
    /// Reason the runs cannot replay, `None` when they can.
    pub fn replay_refusal(&self) -> Option<&str> {
        self.plan.as_ref().err().map(String::as_str)
    }

    /// Returns the reason run `run_id` of a running search cannot replay yet, `None` for any other run.
    ///
    /// A search reports a run before the batch that tells its candidate, and the candidate's values arrive with the
    /// batch.
    pub fn untold_candidate(&self, run_id: u64) -> Option<String> {
        if !self.is_search() {
            return None;
        }
        let config_id = self.run(run_id)?.run.config_id;
        (!self.configs.contains_key(&config_id) && self.unassigned_runs.contains_key(&config_id))
            .then(|| format!("Candidate {config_id} will be known once its batch ends"))
    }

    /// Returns the replay of run `run_id`.
    ///
    /// # Errors
    ///
    /// Returns [`Self::replay_refusal`], or a message when the store holds no run `run_id` or the plan assigns the run
    /// a different config, replicate or seed than its record. A search's run fails when the value columns do not list
    /// the model's parameters in order. A run of either kind fails when its values do not produce the key that its
    /// record holds, unless the model has changed since the run.
    pub fn replay(&self, run_id: u64) -> Result<Replay, String> {
        let plan = self.plan.as_ref().map_err(Clone::clone)?;
        let outcome = self
            .run(run_id)
            .ok_or_else(|| format!("No run {run_id} in these results"))?;
        let plan = match plan {
            ReplayPlan::Sweep(plan) => plan,
            ReplayPlan::Search(search_plan) => return self.search_replay(search_plan, outcome),
        };
        let mismatch = || format!("Run {run_id} does not match the plan of its sweep");
        match plan.run(run_id) {
            Some(planned) if planned == outcome.run && self.key_matches(plan.run_key(&planned), outcome) => {
                plan.replay(run_id).ok_or_else(mismatch)
            }
            _ => Err(mismatch()),
        }
    }

    /// Returns whether `key`, the key that a plan assigns to `outcome`, matches the key its record holds.
    /// Any key matches once the model has changed since the run.
    ///
    /// The key hashes the model's declarations. After a change to the model no run's key matches, and the runs table
    /// warns that a replay might differ.
    pub(super) fn key_matches(&self, key: u64, outcome: &RunOutcome) -> bool {
        !self.schema_matches || key == outcome.run_key
    }

    /// Returns the replay of `outcome`, a run of the search of `search_plan`, from the values of its candidate.
    pub(super) fn search_replay(&self, search_plan: &SearchPlan, outcome: &RunOutcome) -> Result<Replay, String> {
        let run_id = outcome.run.run_id;
        if let Some(reason) = self.untold_candidate(run_id) {
            return Err(reason);
        }
        let mismatch = || format!("Run {run_id} does not match the search it comes from");
        let config = self.configs.get(&outcome.run.config_id).ok_or_else(mismatch)?;
        let param_count = self.descriptors.len();
        let columns = &self.value_columns;
        let names_params = columns
            .iter()
            .zip(&self.descriptors)
            .all(|(column, descriptor)| column == descriptor.id());
        let column_count = param_count + search_plan.base().actions().len();
        if !names_params || columns.len() != column_count || config.texts.len() != column_count {
            return Err(mismatch());
        }
        let (param_texts, tick_texts) = config.texts.split_at(param_count);
        let params = self
            .descriptors
            .iter()
            .zip(param_texts)
            .map(|(descriptor, text)| parse_value(descriptor.kind(), text).ok())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(mismatch)?;
        let action_ticks = tick_texts
            .iter()
            .map(|text| text.parse().ok())
            .collect::<Option<Vec<u64>>>()
            .ok_or_else(mismatch)?;
        let config = Config {
            block: config.block,
            params,
            action_ticks,
        };
        if !self.key_matches(search_plan.run_key(&outcome.run, &config), outcome) {
            return Err(mismatch());
        }
        Ok(search_plan.replay(&outcome.run, &config))
    }

    /// Returns the reason no command line replays the runs, `None` when [`Self::cli_command`] can write one.
    ///
    /// A run of a model file needs the file on the command line, and the results hold no path to it.
    pub fn command_refusal(&self) -> Option<&'static str> {
        let runtime = |build: &RecordedBuild| build.origin.as_deref() == Some(CodeOrigin::Runtime.as_str());
        self.recorded
            .model_builds
            .iter()
            .any(runtime)
            .then_some(MODEL_FILE_COMMAND)
    }

    /// Returns a command line of `program`, a host's equivalent of `henad-cli`, that steps run `run_id` to the tick it
    /// ended on and writes its stats.
    ///
    /// The command sets each parameter that differs from the model's default and fires each action due by the end.
    ///
    /// # Errors
    ///
    /// Returns [`Self::command_refusal`] for the runs of a model file, and the message of [`Self::replay`] for a run
    /// that does not replay.
    pub fn cli_command(&self, program: &str, run_id: u64) -> Result<String, String> {
        if let Some(reason) = self.command_refusal() {
            return Err(reason.to_owned());
        }
        let replay = self.replay(run_id)?;
        let (Ok(plan), Some(outcome)) = (&self.plan, self.run(run_id)) else {
            return Err(format!("No run {run_id} in these results"));
        };
        let plan = plan.base();
        let mut words = vec![
            shell_word(program),
            shell_word(&replay.model),
            "--seed".to_owned(),
            replay.seed.to_string(),
        ];
        for (descriptor, value) in self.descriptors.iter().zip(&replay.params) {
            if *value != descriptor.kind().default_value() {
                let assignment = format!("{}={}", descriptor.id(), format_value(descriptor.kind(), value));
                words.extend(["--set".to_owned(), shell_word(&assignment)]);
            }
        }
        for scheduled in replay.schedule.entries() {
            if scheduled.tick <= outcome.ticks {
                words.extend([
                    "--act".to_owned(),
                    shell_word(&format!("{}@{}", scheduled.id, scheduled.tick)),
                ]);
            }
        }
        let warmup = plan.run_settings().warmup.min(outcome.ticks);
        words.extend([
            "--warmup".to_owned(),
            warmup.to_string(),
            "--steps".to_owned(),
            (outcome.ticks - warmup).to_string(),
        ]);
        let stats_every = plan.measure_settings().stats_every;
        if stats_every != 1 {
            words.extend(["--stats-every".to_owned(), stats_every.to_string()]);
        }
        words.extend(["--export-stats".to_owned(), format!("run-{run_id}.csv")]);
        Ok(words.join(" "))
    }
}

/// Returns the plan that replays the runs of `recorded`, the spec of a sweep read from files, through `model`, the
/// app's entry for the sweep's model or the reason the app has none, or the reason the runs cannot replay.
///
/// A sweep whose manifest records more than `recorded.max_planned` configs is never planned.
pub(super) fn replay_plan(recorded: &RecordedSpec, model: Result<&ModelEntry, String>) -> Result<ReplayPlan, String> {
    let entry = model?;
    if recorded.is_search {
        return SearchPlan::new(&recorded.spec, &entry.schema())
            .map(|search_plan| ReplayPlan::Search(Arc::new(search_plan)))
            .map_err(|error| {
                format!(
                    "{} refuses this search's spec: {}",
                    entry.name(),
                    describe_error(&error)
                )
            });
    }
    if recorded.configs > recorded.max_planned {
        return Err(format!(
            "Sweep has {} configurations, too many to replay in the app",
            recorded.configs
        ));
    }
    recorded
        .spec
        .plan(&entry.schema())
        .map(|plan| ReplayPlan::Sweep(Arc::new(plan)))
        .map_err(|error| format!("{} refuses this sweep's spec: {}", entry.name(), describe_error(&error)))
}

/// Returns the configs of `plan`, a sweep's plan through `entry`, to list beside the configs of `runs.csv`, or none
/// unless the entry declares what the sweep ran with (`schema_matches`) and the plan holds at most `max_planned`.
pub(super) fn planned_texts(
    plan: &Result<ReplayPlan, String>,
    entry: Option<&ModelEntry>,
    schema_matches: bool,
    max_planned: u64,
) -> BTreeMap<u64, ConfigTexts> {
    match (plan, entry) {
        (Ok(ReplayPlan::Sweep(plan)), Some(entry)) if schema_matches && plan.configs().len() as u64 <= max_planned => {
            plan_texts(plan, entry.param_descriptors())
        }
        _ => BTreeMap::new(),
    }
}

/// Returns `text` as one word of a shell command, quoted when it holds anything but plain characters.
pub(super) fn shell_word(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-.,:=@/+%".contains(character));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}
