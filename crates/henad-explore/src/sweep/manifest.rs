//! Manifest of a sweep or a search while it runs and once it ends, and the warnings of a resume under another build.

use henad_compute::entry::ModelEntry;
use henad_core::explore::plan::Plan;

use super::{Provenance, SweepEnd, SweepInputs, SweepOutline, SweepWarning};
use crate::exec::BatchEnd;
use crate::output::OutputError;
use crate::output::manifest::{
    BuildRole, FORMAT, FORMAT_VERSION, Manifest, ManifestColumns, ManifestExecution, ManifestMode, ManifestModel,
    ManifestModelCode, ManifestPlan, ManifestSearch, ManifestSeeds, ManifestSession, ManifestStatus,
    ManifestTimestamps, RecordedBuild, ResultCounts, hex, now_unix_ms, rfc3339,
};
use crate::probe::ProbeReport;
use crate::schema::{backend_name, schema_json};
use crate::spec_file::{ExecutionTable, SpecFile};

/// Returns the advice for a model build that records neither a commit nor a source hash, checking the build that
/// runs first, or `None` when `recorded` and `current` both record a commit or a source hash.
///
/// An entry that was never inserted into a [`ModelSet`](henad_compute::entry::ModelSet) records no build at all, and
/// a build script alone cannot provide a build.
pub(super) fn unidentified_model_advice(recorded: &RecordedBuild, current: &RecordedBuild) -> Option<&'static str> {
    let unidentified = [current, recorded].into_iter().find(|build| !build.is_identified())?;
    Some(if unidentified.package.is_empty() {
        "The model's entry records no build. Insert it into a `ModelSet` built with `henad::build_info!()` in a crate \
         whose build script calls `henad_build::stamp_commit()`"
    } else {
        "The model's build records no commit or source hash. Call `henad_build::stamp_commit()` in the build script \
         of the crate that registers it"
    })
}

/// Returns a [`SweepWarning::BuildChanged`] for each build that [`Manifest::recorded_builds`] lists for `recorded` and
/// differs from the engine build of `provenance`, or from the build that registered `entry`.
pub(crate) fn build_warnings(recorded: &Manifest, provenance: &Provenance, entry: &ModelEntry) -> Vec<SweepWarning> {
    let model = RecordedBuild::from(entry.source());
    [(BuildRole::Engine, provenance.engine()), (BuildRole::Model, &model)]
        .into_iter()
        .flat_map(|(role, current)| {
            recorded
                .recorded_builds(role)
                .into_iter()
                .filter(|build| !build.same_build(current))
                .map(move |build| SweepWarning::BuildChanged {
                    role,
                    recorded: Box::new(build),
                    current: Box::new(current.clone()),
                    between_shards: false,
                })
        })
        .collect()
}

/// Parts of a manifest that differ between a sweep and a search.
pub(crate) struct ManifestParts<'a> {
    pub(crate) mode: ManifestMode,
    /// Plan the runs come from. For a search, the plan of its fixed values alone.
    pub(crate) plan: &'a Plan,
    pub(crate) probe: &'a ProbeReport,
    pub(crate) outline: &'a SweepOutline,
    pub(crate) manifest_plan: ManifestPlan,
    pub(crate) started_unix_ms: u64,
    /// Manifest of the directory a resume adds to, `None` for a fresh start.
    pub(crate) recorded: Option<&'a Manifest>,
    pub(crate) search: Option<ManifestSearch>,
}

/// Returns the manifest, with status `running`, of the sweep or search that `inputs` and `parts` describe.
///
/// A resume keeps the start time, sessions and merged shards the directory recorded. It records the author text of
/// the model it runs, or keeps the recorded author text when the model has none and runs the recorded code.
///
/// # Errors
///
/// Returns [`OutputError::Manifest`] when the spec cannot be written as JSON.
pub(crate) fn running_manifest(inputs: &SweepInputs<'_>, parts: &ManifestParts<'_>) -> Result<Manifest, OutputError> {
    let (entry, options) = (inputs.entry, inputs.options);
    let provenance = &options.provenance;
    let backend = backend_name(entry.metadata().backend).to_owned();
    let mut spec_file = SpecFile::from(inputs.spec);
    spec_file.execution = ExecutionTable {
        concurrent: options.concurrency,
        memory: options.memory_budget,
        gpu_memory: options.gpu_memory_budget,
    };
    let outline = parts.outline;
    let layout = outline.layout;
    let seeds = parts.plan.seed_settings();
    let spec = serde_json::to_value(&spec_file).map_err(OutputError::Manifest)?;
    let session = ManifestSession {
        started: rfc3339(parts.started_unix_ms),
        commit: provenance.engine().commit.clone(),
        skipped: outline.skipped,
        ran: 0,
        engine: Some(provenance.engine().clone()),
        host: Some(provenance.host().clone()),
        model_source: Some(RecordedBuild::from(entry.source())),
    };
    let code = entry.source().code().map(ManifestModelCode::from);
    // A graph with a sheet's code, such as the graph the sheet lowers to, carries no sheet. The folder still holds
    // runs of that sheet.
    let author = entry.source().author().map(ManifestModelCode::from).or_else(|| {
        let recorded = parts.recorded?;
        (code.is_some() && recorded.model.code == code)
            .then(|| recorded.model.author.clone())
            .flatten()
    });
    let (started_unix_ms, sessions, merged_shards) = match parts.recorded {
        Some(recorded) => {
            let mut sessions = recorded.clone().into_credited_sessions(outline.skipped);
            sessions.push(session);
            (
                recorded.timestamps.started_unix_ms,
                sessions,
                recorded.merged_shards.clone(),
            )
        }
        None => (parts.started_unix_ms, vec![session], None),
    };
    Ok(Manifest {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        mode: parts.mode,
        status: ManifestStatus::Running,
        engine: provenance.engine().clone(),
        model: ManifestModel {
            id: entry.id().to_owned(),
            name: entry.name().to_owned(),
            backend: backend.clone(),
            schema_hash: hex(parts.plan.schema_hash()),
            schema: schema_json(entry, Some(parts.probe)),
            replays_exactly: entry.metadata().replays_exactly,
            code,
            author,
        },
        spec,
        spec_source: (&options.spec_source).into(),
        argv: provenance.arguments().to_vec(),
        plan: parts.manifest_plan.clone(),
        seeds: ManifestSeeds {
            root: seeds.root,
            scheme: seeds.scheme.as_str().to_owned(),
            formula: seeds.scheme.formula().to_owned(),
        },
        columns: ManifestColumns {
            stats: outline.stat_columns.clone(),
            reducers: outline.reducer_columns.clone(),
        },
        shard: options.shard.into(),
        execution: ManifestExecution {
            backend,
            concurrency: options.concurrency.to_string(),
            cpu_lanes: layout.cpu_lanes,
            threads_per_lane: layout.threads_per_lane,
            gpu_tracks: layout.gpu_tracks,
            projected_bytes: outline.projected_bytes,
            memory_budget: options.memory_budget,
            gpu_memory_budget: options.gpu_memory_budget,
        },
        runtime: inputs.runtime.clone(),
        timestamps: ManifestTimestamps::started_at(started_unix_ms),
        sessions,
        results: None,
        merged_shards,
        search: parts.search.clone(),
    })
}

/// Marks `manifest` as ended after a batch that ended as `end` with `counts` rows written, and returns the end of the
/// sweep.
pub(crate) fn finish_manifest(manifest: &mut Manifest, end: BatchEnd, counts: ResultCounts) -> SweepEnd {
    let (status, end) = match end {
        BatchEnd::Complete => (ManifestStatus::Complete, SweepEnd::Complete),
        BatchEnd::Aborted => (ManifestStatus::Aborted, SweepEnd::Aborted),
        BatchEnd::DeviceLost => (ManifestStatus::Incomplete, SweepEnd::DeviceLost),
    };
    manifest.finish(status, counts, now_unix_ms());
    end
}
