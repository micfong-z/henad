//! Model and spec that the runs of a results store recorded, and their comparison with the app's entry.

use std::collections::BTreeMap;

use henad_compute::entry::{ModelEntry, ModelLookupError};
use henad_compute::gpu::runtime_grid::GridRule;
use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::spec::{ActionSpec, SweepSpec};
use henad_core::provenance::ModelCode;
use henad_explore::output::manifest::{BuildRole, Manifest, ManifestModelCode, RecordedBuild};
use henad_explore::result_set::ResultSet;
use henad_graph::sheet::SHEET_CODE_FORMAT;

use crate::state::lookup_message;
use crate::ui::editor::{EditableModel, editable_model};
use crate::ui::results::labels::action_label;

use super::ResultsStore;
use super::replay::{planned_texts, replay_plan};

/// Declarations and builds that a sweep recorded of its model. The store compares them with the app's entry.
#[derive(Debug, Clone)]
pub(super) struct RecordedModel {
    /// Henad's builds that the sessions recorded.
    pub(super) engine_builds: Vec<RecordedBuild>,
    /// Builds of the model's code that the sessions recorded.
    pub(super) model_builds: Vec<RecordedBuild>,
    /// Hash of the model's declarations, `None` for a manifest whose hash does not read as one.
    pub(super) schema_hash: Option<u64>,
    /// Whether the sweep recorded that the model replays exactly.
    pub(super) replays_exactly: bool,
    /// Manifest of a folder's sweep, `None` for a sweep run in this app. A resume of the folder compares the code of
    /// its sessions with the app's entry ([`Manifest::code_matches`]).
    pub(super) manifest: Option<Manifest>,
    /// Code of a model composed at run time, `None` for compiled code.
    pub(super) code: Option<ModelCode>,
    /// Author text of a model built from a sheet, `None` for a model built from its code alone.
    pub(super) author: Option<ModelCode>,
}

impl RecordedModel {
    /// Returns the model that the manifest of `set` records.
    pub(super) fn of_result_set(set: &ResultSet) -> Self {
        let model = &set.manifest().model;
        Self {
            engine_builds: set.recorded_builds(BuildRole::Engine),
            model_builds: set.recorded_builds(BuildRole::Model),
            schema_hash: u64::from_str_radix(&model.schema_hash, 16).ok(),
            replays_exactly: model.replays_exactly,
            manifest: Some(set.manifest().clone()),
            code: model.code.as_ref().map(ManifestModelCode::to_code),
            author: set.author(),
        }
    }

    /// Returns the model of a sweep of `entry` on this build of Henad.
    pub(super) fn of_entry(entry: &ModelEntry) -> Self {
        Self {
            engine_builds: vec![RecordedBuild::engine()],
            model_builds: vec![RecordedBuild::from(entry.source())],
            schema_hash: Some(schema_hash(&entry.schema())),
            replays_exactly: entry.metadata().replays_exactly,
            manifest: None,
            code: entry.source().code().cloned(),
            author: entry.source().author().cloned(),
        }
    }

    /// Returns whether `entry` runs the code a resume of the folder needs, `false` without an entry.
    ///
    /// For a sweep run in this app, always `true` with an entry. The Results tab never resumes such a sweep.
    pub(super) fn code_matches(&self, entry: Option<&ModelEntry>) -> bool {
        entry.is_some_and(|entry| {
            self.manifest
                .as_ref()
                .is_none_or(|manifest| manifest.code_matches(entry.source()))
        })
    }
}

/// Returns the roles whose current build, Henad's build or the build of the code of `entry`, differs from a build in
/// `recorded` for that role, and then the roles whose builds cannot be compared.
///
/// A build that records neither a commit nor a source hash is never the same as another build, itself included. A role
/// goes in the second list when each of its differences is between two builds of one package and version, and one
/// of the two is such a build.
pub(super) fn compare_builds(recorded: &RecordedModel, entry: &ModelEntry) -> (Vec<BuildRole>, Vec<BuildRole>) {
    let mut changed = Vec::new();
    let mut unidentified = Vec::new();
    for (role, recorded, current) in [
        (BuildRole::Engine, &recorded.engine_builds, RecordedBuild::engine()),
        (
            BuildRole::Model,
            &recorded.model_builds,
            RecordedBuild::from(entry.source()),
        ),
    ] {
        let mut differing = recorded
            .iter()
            .filter(|recorded| !recorded.same_build(&current))
            .peekable();
        if differing.peek().is_none() {
            continue;
        }
        if differing.any(|recorded| known_change(recorded, &current)) {
            changed.push(role);
        } else {
            unidentified.push(role);
        }
    }
    (changed, unidentified)
}

/// Returns whether `recorded` and `current`, two builds that are not the same, are known to differ: in package or
/// version, or with both identified by a commit or a source hash.
pub(super) fn known_change(recorded: &RecordedBuild, current: &RecordedBuild) -> bool {
    recorded.package != current.package
        || recorded.version != current.version
        || (recorded.is_identified() && current.is_identified())
}

/// Spec of a sweep read from files, which the store plans again against an entry that replaces the sweep's model.
#[derive(Debug, Clone)]
pub(super) struct RecordedSpec {
    pub(super) spec: SweepSpec,
    /// Whether the files hold a search.
    pub(super) is_search: bool,
    /// Number of configs the manifest records, 0 when it records none.
    pub(super) configs: u64,
    /// Largest number of configs the store plans, and lists from a plan.
    pub(super) max_planned: u64,
    /// Hash of the declarations of the entry the plan was made against, `None` for a plan made without an entry.
    pub(super) planned_with: Option<u64>,
}

impl ResultsStore {
    /// Returns the model the sweep ran as the Editor opens it: the sheet the sweep recorded, or the graph or grid rule
    /// of its code when it recorded no sheet. `None` for compiled code.
    ///
    /// A sweep run in this app records the entry it started with, and a folder records what its manifest holds.
    pub fn editable_model(&self) -> Option<EditableModel> {
        editable_model(self.recorded.author.as_ref(), self.recorded.code.as_ref())
    }

    /// Returns whether the sweep recorded a model the Editor can open, a sheet or code in a format it reads, without
    /// reading the text.
    pub fn records_editable_code(&self) -> bool {
        let sheet = self
            .recorded
            .author
            .as_ref()
            .is_some_and(|author| author.format() == SHEET_CODE_FORMAT);
        let code = self
            .recorded
            .code
            .as_ref()
            .is_some_and(|code| [henad_graph::CODE_FORMAT, GridRule::CODE_FORMAT].contains(&code.format()));
        sheet || code
    }

    /// Compares the declarations and builds the sweep recorded with `model`, the app's lookup of the sweep's model id
    /// after the set's entry was replaced, or the reason the app cannot run the model.
    ///
    /// A replay of a run of other code then carries the warning that the replay might differ, and a folder of runtime
    /// code that differs from the entry cannot be resumed. Results read from files plan their spec again
    /// against an entry whose declarations differ from those of the plan they hold, and take the entry's parameters
    /// and action labels. A sweep run in this app keeps the plan of its session.
    pub fn model_changed(&mut self, model: Result<&ModelEntry, &ModelLookupError>) {
        let entry = model.ok();
        self.schema_matches = entry.is_some_and(|entry| {
            entry.id() == self.model_id && self.recorded.schema_hash == Some(schema_hash(&entry.schema()))
        });
        self.compare_entry(entry);
        self.time = entry
            .filter(|entry| entry.id() == self.model_id)
            .and_then(|entry| entry.metadata().time.clone());
        if let Some(mut recorded) = self.recorded_spec.take() {
            self.take_declarations(entry, &recorded.spec.actions);
            let declarations = entry.map(|entry| schema_hash(&entry.schema()));
            if declarations.is_none() || declarations != recorded.planned_with {
                recorded.planned_with = declarations;
                self.plan = replay_plan(&recorded, model.map_err(lookup_message));
                let planned = planned_texts(&self.plan, entry, self.schema_matches, recorded.max_planned);
                self.rebuild_configs(planned);
            } else {
                let labels: Vec<String> = self.axes.iter().map(|axis| self.column_label(axis.column)).collect();
                for (axis, label) in self.axes.iter_mut().zip(labels) {
                    axis.label = label;
                }
            }
            self.recorded_spec = Some(recorded);
        }
        self.revision += 1;
    }

    /// Compares the builds and the code that the sweep recorded with `entry`, the app's entry of the sweep's model,
    /// `None` when the app cannot run the model.
    ///
    /// Note that [`Self::for_sweep`] does not call this. A build that records neither a commit nor a source hash
    /// compares unequal to itself, and a sweep that just started would list its own build as unidentified.
    pub(super) fn compare_entry(&mut self, entry: Option<&ModelEntry>) {
        (self.changed_builds, self.unidentified_builds) =
            entry.map_or_else(Default::default, |entry| compare_builds(&self.recorded, entry));
        self.replays_exactly =
            self.recorded.replays_exactly && entry.is_none_or(|entry| entry.metadata().replays_exactly);
        self.code_matches = self.recorded.code_matches(entry);
    }

    /// Takes the parameters of `entry`, and the labels that `entry` declares for `actions`, the recorded spec's
    /// actions. Both are empty when the app cannot run the model.
    pub(super) fn take_declarations(&mut self, entry: Option<&ModelEntry>, actions: &[ActionSpec]) {
        self.descriptors = entry
            .map(|entry| entry.param_descriptors().to_vec())
            .unwrap_or_default();
        self.action_labels = entry.map_or_else(BTreeMap::new, |entry| action_labels(actions, entry));
    }
}

/// Returns the label `entry` declares for each action of `actions`, by the name that the spec assigns to the action.
pub(super) fn action_labels(actions: &[ActionSpec], entry: &ModelEntry) -> BTreeMap<String, String> {
    actions
        .iter()
        .filter_map(|action| {
            let declared = entry
                .action_descriptors()
                .iter()
                .find(|declared| declared.id() == action.id)?;
            let label = action_label(declared.label(), &action.id, &action.name);
            Some((action.name.clone(), label))
        })
        .collect()
}
