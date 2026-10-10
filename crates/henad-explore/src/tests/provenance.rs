//! Checks that every session records the builds it ran, and that a resume or a merge warns when a build changed.
//!
//! A model composed at run time or loaded from a plugin records the identity of its code, and a resume or a merge
//! refuses code that changed. The runtime entries here are the registered Game of Life with runtime code attached,
//! which steps exactly as the registered entry does.

use std::path::PathBuf;

use henad_compute::entry::{ModelEntry, ModelSet, register_grid_model};
use henad_core::authoring::model::grid_model::GridModel as _;
use henad_core::explore::plan::Shard;
use henad_core::explore::spec::SweepSpec;
use henad_core::provenance::{BuildInfo, CodeOrigin, ModelCode, ModelSource};
use henad_models::game_of_life::GameOfLifeModel;

use crate::exec::Concurrency;
use crate::merge::{MergeError, merge};
use crate::output::manifest::{BuildRole, FORMAT_VERSION, Manifest, ManifestModelCode, RecordedBuild};
use crate::output::resume::ResumeError;
use crate::output::{MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::result_set::ResultSet;
use crate::sweep::{ExploreError, Provenance, SweepOptions, SweepWarning};
use crate::tests::support::{
    OutputTables, Recorder, ScratchDir, entry, manifest, other_engine, provenance, sweep_options, sweep_with,
    unbuildable,
};

/// Returns Game of Life on an 8 by 8 grid for 2 steps, with `replicates` replicates.
fn life_spec(replicates: u64) -> SweepSpec {
    let mut spec = SweepSpec::new(GameOfLifeModel::ID);
    spec.fixed = vec![
        ("grid_width".to_owned(), "8".to_owned()),
        ("grid_height".to_owned(), "8".to_owned()),
    ];
    spec.run.steps = 2;
    spec.run.replicates = replicates;
    spec
}

/// Returns Game of Life registered by a crate whose build is `build`.
fn registered_by(build: BuildInfo) -> ModelEntry {
    let mut set = ModelSet::new(build);
    set.insert(register_grid_model::<GameOfLifeModel>())
        .expect("the set takes the entry");
    set.get(GameOfLifeModel::ID).expect("the entry is in the set").clone()
}

/// Returns the build of a model crate at commit `abc12345`, dirty or not, with the source hash `hash` given as 16
/// hexadecimal digits.
fn model_crate(dirty: Option<&'static str>, hash: Option<&'static str>) -> BuildInfo {
    BuildInfo::__from_env("life-crate", "1.0.0", Some("abc12345"), None, dirty, hash, false)
}

/// Returns the provenance of the tests with `engine` as Henad's build.
fn under_engine(engine: RecordedBuild) -> Provenance {
    provenance().with_engine(engine)
}

/// Sweeps `first` under `first_provenance` with one replicate, then resumes it with `second` under
/// `second_provenance` with two replicates, and returns the resume's warnings and the manifest it leaves.
fn resume_warnings(
    (first, first_provenance): (&ModelEntry, Provenance),
    (second, second_provenance): (&ModelEntry, Provenance),
) -> (Vec<SweepWarning>, Manifest) {
    let scratch = ScratchDir::new("provenance");
    let first_options = SweepOptions {
        provenance: first_provenance,
        ..sweep_options(false)
    };
    sweep_with(
        first,
        None,
        &life_spec(1),
        scratch.path(),
        &first_options,
        &mut Recorder::default(),
    )
    .expect("the sweep runs");
    let second_options = SweepOptions {
        provenance: second_provenance,
        ..sweep_options(true)
    };
    let mut progress = Recorder::default();
    sweep_with(
        second,
        None,
        &life_spec(2),
        scratch.path(),
        &second_options,
        &mut progress,
    )
    .expect("the resume runs");
    (progress.warnings, manifest(scratch.path()))
}

/// Returns the warning of a build that changed for `role` from `recorded` to `current`, the build that runs.
fn changed(role: BuildRole, recorded: RecordedBuild, current: RecordedBuild) -> SweepWarning {
    SweepWarning::BuildChanged {
        role,
        recorded: Box::new(recorded),
        current: Box::new(current),
        between_shards: false,
    }
}

/// Returns Henad's own build, at a clean commit `c0ffee00` with the source hash `source_hash`.
fn clean_engine(source_hash: u64) -> RecordedBuild {
    let mut engine = RecordedBuild::engine();
    engine.commit = "c0ffee00".to_owned();
    engine.dirty = Some(false);
    engine.source_hash = Some(source_hash);
    engine
}

#[test]
fn an_example_model_resume_from_an_unchanged_checkout_raises_no_warning() {
    let life = entry(GameOfLifeModel::ID, None);
    let (warnings, recorded) = resume_warnings((&life, provenance()), (&life, provenance()));
    assert_eq!(warnings, []);
    assert_eq!(recorded.sessions.len(), 2);
    assert!(
        recorded.sessions[0]
            .model_source
            .as_ref()
            .is_some_and(RecordedBuild::is_identified),
        "henad-models records its source hash"
    );
}

#[test]
fn a_sweep_from_a_bare_entry_records_its_type_path_and_host() {
    let bare = register_grid_model::<GameOfLifeModel>();
    let (warnings, recorded) = resume_warnings((&bare, provenance()), (&bare, provenance()));
    let session = &recorded.sessions[0];
    let source = session
        .model_source
        .as_ref()
        .expect("the session records the model's source");
    assert_eq!(
        source.type_path.as_deref(),
        Some(std::any::type_name::<GameOfLifeModel>())
    );
    assert_eq!(source.package, "");
    assert_eq!(session.host, Some(RecordedBuild::from(&henad_core::build_info!())));
    assert_eq!(session.engine.as_ref(), Some(provenance().engine()));
    assert_eq!(session.commit, provenance().engine().commit);

    let [SweepWarning::BuildChanged { role, .. }] = warnings.as_slice() else {
        panic!("one warning, got {warnings:?}");
    };
    assert_eq!(*role, BuildRole::Model);
    assert!(
        warnings[0].to_string().contains("Insert it into a `ModelSet`"),
        "the warning names its cause: {}",
        warnings[0]
    );
}

#[test]
fn a_resumed_manifest_keeps_each_sessions_model_source() {
    let first = registered_by(model_crate(Some("true"), Some("00000000000000aa")));
    let second = registered_by(model_crate(Some("true"), Some("00000000000000bb")));
    let (warnings, recorded) = resume_warnings((&first, provenance()), (&second, provenance()));
    let sources: Vec<Option<u64>> = recorded
        .sessions
        .iter()
        .map(|session| session.model_source.as_ref()?.source_hash)
        .collect();
    assert_eq!(sources, [Some(0xAA), Some(0xBB)]);
    assert_eq!(
        warnings,
        [changed(
            BuildRole::Model,
            RecordedBuild::from(first.source()),
            RecordedBuild::from(second.source())
        )]
    );
    assert_eq!(recorded.recorded_builds(BuildRole::Model).len(), 2);
}

#[test]
fn a_resume_after_an_uncommitted_edit_warns() {
    let edited = registered_by(model_crate(Some("true"), Some("00000000000000aa")));
    let again = registered_by(model_crate(Some("true"), Some("00000000000000aa")));
    let (warnings, _) = resume_warnings((&edited, provenance()), (&again, provenance()));
    assert_eq!(warnings, [], "the same edit is the same build");

    let edited_further = registered_by(model_crate(Some("true"), Some("00000000000000cc")));
    let (warnings, _) = resume_warnings((&edited, provenance()), (&edited_further, provenance()));
    assert!(
        matches!(
            warnings.as_slice(),
            [SweepWarning::BuildChanged {
                role: BuildRole::Model,
                ..
            }]
        ),
        "{warnings:?}"
    );
}

#[test]
fn a_cargo_update_warns_for_a_model_the_host_registers() {
    // A host outside git registers its own model: no commit, a source hash over its sources and its lockfile.
    let host_crate = |hash| BuildInfo::__from_env("my-model", "0.1.0", Some(""), None, Some(""), Some(hash), false);
    let before = registered_by(host_crate("0000000000000001"));
    let after = registered_by(host_crate("0000000000000002"));
    let (warnings, _) = resume_warnings((&before, provenance()), (&after, provenance()));
    assert_eq!(
        warnings,
        [changed(
            BuildRole::Model,
            RecordedBuild::from(before.source()),
            RecordedBuild::from(after.source())
        )]
    );
}

#[test]
fn a_resume_under_another_engine_build_warns() {
    let life = entry(GameOfLifeModel::ID, None);
    let mut older = RecordedBuild::engine();
    older.version = "0.1.0".to_owned();
    let (warnings, recorded) = resume_warnings((&life, under_engine(older.clone())), (&life, provenance()));
    assert_eq!(
        warnings,
        [changed(BuildRole::Engine, older.clone(), RecordedBuild::engine())]
    );
    assert_eq!(
        recorded.recorded_builds(BuildRole::Engine),
        [older, RecordedBuild::engine()]
    );
    assert_eq!(
        recorded.engine,
        RecordedBuild::engine(),
        "the engine block names the latest build"
    );
}

#[test]
fn a_resume_after_an_uncommitted_explore_edit_warns() {
    let life = entry(GameOfLifeModel::ID, None);
    let mut edited = clean_engine(1);
    edited.dirty = Some(true);
    let mut edited_further = edited.clone();
    edited_further.source_hash = Some(2);
    *edited_further
        .crate_hashes
        .entry("henad-explore".to_owned())
        .or_default() ^= 1;
    let (warnings, _) = resume_warnings(
        (&life, under_engine(edited.clone())),
        (&life, under_engine(edited_further.clone())),
    );
    assert_eq!(warnings, [changed(BuildRole::Engine, edited, edited_further)]);
}

#[test]
fn a_patched_compute_copy_with_a_clean_commit_warns() {
    let life = entry(GameOfLifeModel::ID, None);
    let registry = clean_engine(1);
    let mut patched = registry.clone();
    *patched.crate_hashes.entry("henad-compute".to_owned()).or_default() ^= 1;
    let (warnings, _) = resume_warnings(
        (&life, under_engine(registry.clone())),
        (&life, under_engine(patched.clone())),
    );
    assert_eq!(
        warnings,
        [changed(BuildRole::Engine, registry.clone(), patched.clone())]
    );
    assert!(
        warnings[0]
            .to_string()
            .contains("They differ in the sources of henad-compute"),
        "{}",
        warnings[0]
    );
}

#[test]
fn a_resume_after_a_core_patch_warns() {
    let life = entry(GameOfLifeModel::ID, None);
    let before = clean_engine(1);
    let mut patched = before.clone();
    patched
        .crate_versions
        .insert("henad-core".to_owned(), "0.2.99".to_owned());
    let (warnings, _) = resume_warnings(
        (&life, under_engine(before.clone())),
        (&life, under_engine(patched.clone())),
    );
    assert_eq!(warnings, [changed(BuildRole::Engine, before, patched)]);
    assert!(warnings[0].to_string().contains("henad-core"), "{}", warnings[0]);
}

#[test]
fn a_registry_build_and_a_checkout_of_one_commit_compare_equal() {
    // A checkout hashes its siblings and the lockfile, and a package its own files. The commit decides.
    let life = entry(GameOfLifeModel::ID, None);
    let checkout = clean_engine(1);
    let registry = clean_engine(2);
    let (warnings, _) = resume_warnings((&life, under_engine(checkout)), (&life, under_engine(registry)));
    assert_eq!(warnings, []);
}

#[test]
fn a_cli_sweep_opened_in_a_dirty_app_raises_no_build_warning() {
    let life = entry(GameOfLifeModel::ID, None);
    let cli = Provenance::new(
        BuildInfo::__from_env("henad-cli", "0.2.0", Some("abc12345"), None, Some("false"), None, false),
        Vec::new(),
    );
    let app = Provenance::new(
        BuildInfo::__from_env(
            "henad-app",
            "0.2.0",
            Some("abc12345"),
            None,
            Some("true"),
            Some("00000000000000ff"),
            false,
        ),
        Vec::new(),
    );
    let (warnings, recorded) = resume_warnings((&life, cli), (&life, app));
    assert_eq!(warnings, [], "no comparison reads the host");
    let hosts: Vec<&str> = recorded
        .sessions
        .iter()
        .filter_map(|session| session.host.as_ref())
        .map(|host| host.package.as_str())
        .collect();
    assert_eq!(hosts, ["henad-cli", "henad-app"]);
}

/// Runs shard `index` of 2 of the Game of Life sweep into `dir` under `engine`.
fn shard_under(dir: &std::path::Path, index: u64, engine: RecordedBuild) {
    let options = SweepOptions {
        provenance: under_engine(engine),
        shard: Shard::new(index, 2).expect("a valid shard"),
        ..sweep_options(false)
    };
    let life = entry(GameOfLifeModel::ID, None);
    sweep_with(&life, None, &life_spec(2), dir, &options, &mut Recorder::default()).expect("the shard runs");
}

#[test]
fn a_merge_of_shards_from_two_builds_records_both() {
    let scratch = ScratchDir::new("merge-builds");
    let dirs: Vec<PathBuf> = (0..2)
        .map(|index| scratch.path().join(format!("shard-{index}")))
        .collect();
    shard_under(&dirs[0], 0, RecordedBuild::engine());
    shard_under(&dirs[1], 1, other_engine());
    let merged = scratch.path().join("merged");
    let mut progress = Recorder::default();
    merge(&dirs, &merged, &mut progress).expect("the shards merge");
    let recorded = manifest(&merged);
    let engines: Vec<Option<&RecordedBuild>> = recorded
        .sessions
        .iter()
        .map(|session| session.engine.as_ref())
        .collect();
    assert_eq!(engines, [Some(&RecordedBuild::engine()), Some(&other_engine())]);
    assert_eq!(
        recorded.recorded_builds(BuildRole::Engine),
        [RecordedBuild::engine(), other_engine()]
    );
}

#[test]
fn a_merged_manifest_records_the_merging_build() {
    let scratch = ScratchDir::new("merge-engine");
    let dirs: Vec<PathBuf> = (0..2)
        .map(|index| scratch.path().join(format!("shard-{index}")))
        .collect();
    shard_under(&dirs[0], 0, other_engine());
    shard_under(&dirs[1], 1, other_engine());
    let merged = scratch.path().join("merged");
    merge(&dirs, &merged, &mut Recorder::default()).expect("the shards merge");
    let recorded = manifest(&merged);
    assert_eq!(recorded.engine, RecordedBuild::engine());
    assert_eq!(recorded.recorded_builds(BuildRole::Engine), [other_engine()]);
}

#[test]
fn a_merge_of_shards_under_another_engine_build_warns() {
    let scratch = ScratchDir::new("merge-warns");
    let dirs: Vec<PathBuf> = (0..2)
        .map(|index| scratch.path().join(format!("shard-{index}")))
        .collect();
    shard_under(&dirs[0], 0, RecordedBuild::engine());
    shard_under(&dirs[1], 1, other_engine());
    let mut progress = Recorder::default();
    merge(&dirs, &scratch.path().join("merged"), &mut progress).expect("the shards merge");
    assert_eq!(
        progress.warnings,
        [SweepWarning::BuildChanged {
            role: BuildRole::Engine,
            recorded: Box::new(RecordedBuild::engine()),
            current: Box::new(other_engine()),
            between_shards: true,
        }]
    );
    let text = progress.warnings[0].to_string();
    assert!(
        text.starts_with("the shards ran different engine builds") && text.ends_with("in another"),
        "{text}"
    );

    let same = ScratchDir::new("merge-same");
    let dirs: Vec<PathBuf> = (0..2).map(|index| same.path().join(format!("shard-{index}"))).collect();
    shard_under(&dirs[0], 0, RecordedBuild::engine());
    shard_under(&dirs[1], 1, RecordedBuild::engine());
    let mut progress = Recorder::default();
    merge(&dirs, &same.path().join("merged"), &mut progress).expect("the shards merge");
    assert_eq!(progress.warnings, []);
}

#[test]
fn a_dry_run_of_a_resume_under_the_same_build_warns_nothing() {
    let life = entry(GameOfLifeModel::ID, None);
    let scratch = ScratchDir::new("provenance-dry");
    crate::tests::support::sweep(&life, None, &life_spec(1), scratch.path(), Concurrency::Auto);
    let mut progress = Recorder::default();
    crate::tests::support::dry_run(
        &life,
        &life_spec(2),
        Some(scratch.path()),
        &sweep_options(true),
        &mut progress,
    )
    .expect("the dry run plans");
    assert_eq!(progress.warnings, []);
}

/// Returns the folder Henad 0.2.0 wrote.
///
/// `tests/fixtures/manifest-0.2.0/README.md` gives the procedure that recorded it from the `v0.2.0` tag.
///
/// # Panics
///
/// Panics when the folder holds no manifest. A test that skipped would let a change that breaks 0.2 folders pass.
fn folder_0_2_0() -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manifest-0.2.0");
    assert!(
        folder.join(MANIFEST_FILE).is_file(),
        "{} holds no manifest. Its README.md gives the procedure that records it",
        folder.display()
    );
    folder
}

#[test]
fn a_0_2_0_manifest_still_replays_and_merges() {
    let fixture = folder_0_2_0();
    let sir = entry("sir", None);
    let set = ResultSet::open_dir(&fixture, 0).expect("the fixture reads");
    assert_eq!(set.runs().len(), 4, "2 configs by 2 replicates");
    for recorded in set.runs() {
        let run = &recorded.outcome.run;
        let replay = set.replay(sir.schema(), run.run_id).expect("a 0.2 run replays");
        assert_eq!(
            (replay.seed, replay.ticks),
            (run.seed, recorded.outcome.ticks),
            "run {}",
            run.run_id
        );
    }

    let scratch = ScratchDir::new("merge-0.2.0");
    let merged = scratch.path().join("merged");
    let mut progress = Recorder::default();
    merge(std::slice::from_ref(&fixture), &merged, &mut progress).expect("a 0.2 folder merges as its one shard");
    assert_eq!(progress.warnings, []);
    assert_eq!(
        OutputTables::read(&merged),
        OutputTables::read(&fixture),
        "the merge of the one shard holds the runs 0.2.0 wrote"
    );
}

#[test]
fn a_0_2_0_manifest_still_resumes() {
    let fixture = folder_0_2_0();
    let scratch = ScratchDir::new("resume-0.2.0");
    let resumed_dir = scratch.path().join("resumed");
    std::fs::create_dir_all(&resumed_dir).expect("the folder can be created");
    for file in [MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE] {
        std::fs::copy(fixture.join(file), resumed_dir.join(file)).expect("the fixture can be copied");
    }
    let written = manifest(&resumed_dir);
    assert!(
        written.model.replays_exactly,
        "a 0.2 manifest reads as replaying exactly"
    );
    assert!(written.sessions.iter().all(|session| session.engine.is_none()));
    let (version, commit) = (written.engine.version.clone(), written.engine.commit.clone());
    assert_eq!(version, "0.2.0");

    let mut spec = ResultSet::open_dir(&resumed_dir, 0)
        .expect("the fixture reads")
        .spec()
        .clone();
    let sir = entry("sir", None);
    for replicates in [spec.run.replicates + 1, spec.run.replicates + 2] {
        spec.run.replicates = replicates;
        let mut progress = Recorder::default();
        sweep_with(&sir, None, &spec, &resumed_dir, &sweep_options(true), &mut progress).expect("the folder resumes");
        let [
            SweepWarning::BuildChanged {
                role,
                recorded,
                current,
                between_shards: false,
            },
        ] = progress.warnings.as_slice()
        else {
            panic!("one warning on each resume, got {:?}", progress.warnings);
        };
        assert_eq!(*role, BuildRole::Engine);
        assert_eq!(
            (recorded.version.as_str(), recorded.commit.as_str()),
            (version.as_str(), commit.as_str())
        );
        assert_eq!(**current, RecordedBuild::engine());

        let set = ResultSet::open_dir(&resumed_dir, usize::MAX).expect("the resumed folder reads");
        assert!(set.manifest().model.replays_exactly);
        assert!(
            set.recorded_builds(BuildRole::Engine)
                .iter()
                .any(|build| build.version == version && build.commit == commit && build.source_hash.is_none()),
            "the 0.2 build stays among the recorded builds"
        );
        assert_eq!(
            set.manifest().sessions[0]
                .engine
                .as_ref()
                .map(|build| build.version.as_str()),
            Some(version.as_str()),
            "the 0.2 session keeps the version it ran"
        );
    }

    let fresh_dir = scratch.path().join("fresh");
    sweep_with(
        &sir,
        None,
        &spec,
        &fresh_dir,
        &sweep_options(false),
        &mut Recorder::default(),
    )
    .expect("the fresh sweep runs");
    assert_eq!(
        OutputTables::read(&resumed_dir),
        OutputTables::read(&fresh_dir),
        "the runs 0.2.0 wrote match the runs this build writes"
    );
}

/// Returns the code of a rule in the test format, whose text is `text`.
fn rule(text: &str) -> ModelCode {
    ModelCode::new("test-rule/1", text.to_owned())
}

/// Returns Game of Life with the source of a model composed at run time from `code`.
fn runtime_life(code: &ModelCode) -> ModelEntry {
    entry(GameOfLifeModel::ID, None).__with_code(code.clone())
}

/// Returns Game of Life loaded from a plugin of the model crate whose sources hash to `hash`, given as 16
/// hexadecimal digits.
fn plugin_life(hash: &'static str) -> ModelEntry {
    registered_by(model_crate(Some("true"), Some(hash))).__into_plugin()
}

/// Sweeps `first` with one replicate into a fresh directory, then resumes it with `second` and two replicates.
///
/// Returns the warnings or the error of the resume, and whether the resume left the manifest and `runs.csv` as they
/// were.
fn resume_with(first: &ModelEntry, second: &ModelEntry) -> (Result<Vec<SweepWarning>, ExploreError>, bool) {
    let scratch = ScratchDir::new("code-identity");
    sweep_with(
        first,
        None,
        &life_spec(1),
        scratch.path(),
        &sweep_options(false),
        &mut Recorder::default(),
    )
    .expect("the sweep runs");
    let before = folder_bytes(scratch.path());
    let mut progress = Recorder::default();
    let result = sweep_with(
        second,
        None,
        &life_spec(2),
        scratch.path(),
        &sweep_options(true),
        &mut progress,
    )
    .map(|_| progress.warnings);
    let unchanged = folder_bytes(scratch.path()) == before;
    (result, unchanged)
}

/// Returns the bytes of the manifest and `runs.csv` in `dir`.
fn folder_bytes(dir: &std::path::Path) -> Vec<Vec<u8>> {
    [MANIFEST_FILE, RUNS_FILE]
        .map(|file| std::fs::read(dir.join(file)).expect("the file reads"))
        .to_vec()
}

/// Asserts that `result` is the refusal of changed code, and returns its message.
fn code_refusal(result: Result<Vec<SweepWarning>, ExploreError>) -> String {
    match result {
        Err(ExploreError::Resume(error @ ResumeError::CodeChanged { .. })) => error.to_string(),
        other => panic!("expected the refusal of changed code, got {other:?}"),
    }
}

#[test]
fn a_value_built_entry_sweeps_byte_identically_to_the_registered_one() {
    let registered = entry(GameOfLifeModel::ID, None);
    let runtime = runtime_life(&rule("life"));
    assert_eq!(runtime.source().origin(), CodeOrigin::Runtime);
    let scratch = ScratchDir::new("value-built");
    let (registered_dir, runtime_dir) = (scratch.path().join("registered"), scratch.path().join("runtime"));
    crate::tests::support::sweep(&registered, None, &life_spec(2), &registered_dir, Concurrency::Auto);
    crate::tests::support::sweep(&runtime, None, &life_spec(2), &runtime_dir, Concurrency::Auto);
    assert_eq!(
        OutputTables::read(&runtime_dir),
        OutputTables::read(&registered_dir),
        "the source is recorded in the manifest alone"
    );
    assert_eq!(
        manifest(&runtime_dir).model.schema_hash,
        manifest(&registered_dir).model.schema_hash
    );
}

#[test]
fn a_resume_refuses_runtime_code_that_changed() {
    let life = rule("life");
    let (result, _) = resume_with(&runtime_life(&life), &runtime_life(&life));
    assert_eq!(
        result.expect("the same code resumes"),
        [],
        "the same code is the same build"
    );

    let edited = rule("life, edited");
    let (result, unchanged) = resume_with(&runtime_life(&life), &runtime_life(&edited));
    let message = code_refusal(result);
    assert!(unchanged, "a refused resume leaves the directory alone");
    assert!(
        message.contains(&format!("runtime code {:016x}", life.hash()))
            && message.contains(&format!("expected runtime code {:016x}", edited.hash())),
        "{message}"
    );

    let compiled = entry(GameOfLifeModel::ID, None);
    let (result, _) = resume_with(&runtime_life(&life), &compiled);
    code_refusal(result);
    let (result, _) = resume_with(&compiled, &runtime_life(&life));
    code_refusal(result);

    // The code is checked before the probe build, which never builds the edited code. Otherwise every probed config
    // faults, and the resume reports the probe.
    let (result, unchanged) = resume_with(&runtime_life(&life), &unbuildable(runtime_life(&edited)));
    code_refusal(result);
    assert!(unchanged);
}

#[test]
fn a_resume_refuses_a_plugin_whose_sources_changed() {
    let (result, _) = resume_with(&plugin_life("00000000000000aa"), &plugin_life("00000000000000aa"));
    assert_eq!(result.expect("unchanged sources resume"), []);

    let (result, unchanged) = resume_with(&plugin_life("00000000000000aa"), &plugin_life("00000000000000bb"));
    let message = code_refusal(result);
    assert!(unchanged);
    assert!(
        message.contains("plugin code of life-crate 1.0.0, sources 00000000000000aa"),
        "{message}"
    );

    // A plugin whose build records no source hash cannot show that its code is unchanged.
    let unhashed = registered_by(model_crate(Some("false"), None)).__into_plugin();
    let (result, _) = resume_with(&unhashed, &unhashed);
    code_refusal(result);
}

#[test]
fn a_plugin_with_unchanged_sources_resumes_a_compiled_folder() {
    let compiled = registered_by(model_crate(Some("false"), Some("00000000000000aa")));
    let (result, _) = resume_with(&compiled, &plugin_life("00000000000000aa"));
    assert_eq!(result.expect("unchanged sources resume"), []);
    let (result, _) = resume_with(&plugin_life("00000000000000aa"), &compiled);
    assert_eq!(result.expect("unchanged sources resume"), []);

    let (result, _) = resume_with(&compiled, &plugin_life("00000000000000bb"));
    code_refusal(result);
}

#[test]
fn a_compiled_build_change_still_only_warns() {
    let before = registered_by(model_crate(Some("false"), Some("00000000000000aa")));
    let after = registered_by(BuildInfo::__from_env(
        "life-crate",
        "1.0.0",
        Some("def67890"),
        None,
        Some("false"),
        Some("00000000000000bb"),
        false,
    ));
    let (result, unchanged) = resume_with(&before, &after);
    let warnings = result.expect("a compiled build change resumes");
    assert!(!unchanged, "the resume ran");
    assert_eq!(
        warnings,
        [changed(
            BuildRole::Model,
            RecordedBuild::from(before.source()),
            RecordedBuild::from(after.source())
        )]
    );
}

#[test]
fn a_merge_refuses_shards_of_two_codes() {
    let scratch = ScratchDir::new("merge-codes");
    let dirs: Vec<PathBuf> = (0..3)
        .map(|index| scratch.path().join(format!("shard-{index}")))
        .collect();
    let (life, edited) = (rule("life"), rule("life, edited"));
    let shard = |dir: &std::path::Path, index: u64, count: u64, entry: &ModelEntry, spec: &SweepSpec| {
        let options = SweepOptions {
            shard: Shard::new(index, count).expect("a valid shard"),
            ..sweep_options(false)
        };
        sweep_with(entry, None, spec, dir, &options, &mut Recorder::default()).expect("the shard runs");
    };
    shard(&dirs[0], 0, 2, &runtime_life(&life), &life_spec(2));
    shard(&dirs[1], 1, 2, &runtime_life(&life), &life_spec(2));
    shard(&dirs[2], 1, 2, &runtime_life(&edited), &life_spec(2));

    let merged = scratch.path().join("merged");
    merge(&dirs[..2], &merged, &mut Recorder::default()).expect("shards of one code merge");
    assert_eq!(
        manifest(&merged).model.code,
        Some(ManifestModelCode::from(&life)),
        "the merged manifest keeps the code inline"
    );

    let refused = scratch.path().join("refused");
    let error = merge(&[dirs[0].clone(), dirs[2].clone()], &refused, &mut Recorder::default())
        .expect_err("shards of two codes");
    let MergeError::CodeChanged { dir, lower_dir, .. } = &error else {
        panic!("expected the refusal of changed code, got {error:?}");
    };
    assert_eq!((dir, lower_dir), (&dirs[2], &dirs[0]));
    assert!(!refused.exists(), "a refused merge writes nothing");
    assert!(error.to_string().contains("ran different model code"), "{error}");

    // The code is compared before the plan. Edited code whose shard also ran a longer plan reports the code.
    let longer = scratch.path().join("longer");
    let mut longer_spec = life_spec(2);
    longer_spec.run.steps = 3;
    shard(&longer, 1, 2, &runtime_life(&edited), &longer_spec);
    let error = merge(&[dirs[0].clone(), longer], &refused, &mut Recorder::default())
        .expect_err("shards of two codes and two plans");
    assert!(matches!(error, MergeError::CodeChanged { .. }), "{error:?}");

    // Every pair of shards is compared. A compiled shard of sources `aa`, a plugin shard of `aa` and a compiled shard
    // of `bb` each match the first shard, and the plugin shard and the last shard differ.
    let three: Vec<PathBuf> = (0..3)
        .map(|index| scratch.path().join(format!("three-{index}")))
        .collect();
    let compiled = |hash| registered_by(model_crate(Some("false"), Some(hash)));
    shard(&three[0], 0, 3, &compiled("00000000000000aa"), &life_spec(3));
    shard(&three[1], 1, 3, &plugin_life("00000000000000aa"), &life_spec(3));
    shard(&three[2], 2, 3, &compiled("00000000000000bb"), &life_spec(3));
    let error = merge(&three, &refused, &mut Recorder::default()).expect_err("a plugin and other sources");
    let MergeError::CodeChanged { dir, lower_dir, .. } = &error else {
        panic!("expected the refusal of changed code, got {error:?}");
    };
    assert_eq!((dir, lower_dir), (&three[2], &three[1]));
    assert!(!refused.exists(), "a refused merge writes nothing");
}

#[test]
fn a_manifest_carries_runtime_code_inline() {
    let code = rule("life\nwith a second line");
    let scratch = ScratchDir::new("code-inline");
    crate::tests::support::sweep(
        &runtime_life(&code),
        None,
        &life_spec(1),
        scratch.path(),
        Concurrency::Auto,
    );
    let recorded = manifest(scratch.path());
    assert_eq!(recorded.format_version, FORMAT_VERSION);
    assert_eq!(FORMAT_VERSION, 1, "runtime code adds optional fields alone");
    let inline = recorded.model.code.as_ref().expect("the manifest carries the code");
    assert_eq!(
        (inline.format.as_str(), inline.text.as_str()),
        ("test-rule/1", code.text())
    );
    assert_eq!(inline.fnv1a64, format!("{:016x}", code.hash()));
    let source = recorded.sessions[0]
        .model_source
        .as_ref()
        .expect("the session records the model");
    assert_eq!(source.origin.as_deref(), Some("runtime"));
    assert_eq!(source.code_hash, Some(code.hash()));
    assert_eq!(source.type_path.as_deref(), Some("runtime:game_of_life"));
    assert_eq!((source.package.as_str(), source.source_hash), ("", None));
    assert_eq!(recorded.model.schema["origin"], "runtime");

    let set = ResultSet::open_dir(scratch.path(), 0).expect("the folder reads");
    assert!(set.code_matches(&ModelSource::runtime("game_of_life", code.clone())));
    assert!(!set.code_matches(&ModelSource::runtime("game_of_life", rule("other"))));
    assert!(!set.code_matches(entry(GameOfLifeModel::ID, None).source()));

    let compiled = ScratchDir::new("code-compiled");
    crate::tests::support::sweep(
        &entry(GameOfLifeModel::ID, None),
        None,
        &life_spec(1),
        compiled.path(),
        Concurrency::Auto,
    );
    let text = std::fs::read_to_string(compiled.path().join(MANIFEST_FILE)).expect("the manifest reads");
    for key in ["\"code\"", "\"origin\"", "\"code_hash\""] {
        assert!(!text.contains(key), "a compiled model's manifest writes no {key}");
    }
    let set = ResultSet::open_dir(compiled.path(), 0).expect("the folder reads");
    assert!(set.code_matches(entry(GameOfLifeModel::ID, None).source()));
    assert!(!set.code_matches(&ModelSource::runtime("game_of_life", code)));
}

#[test]
fn two_codes_under_one_label_are_listed_apart() {
    let (life, edited) = (rule("life"), rule("life, edited"));
    let first = RecordedBuild::from(&ModelSource::runtime("life", life.clone()));
    let second = RecordedBuild::from(&ModelSource::runtime("life", edited.clone()));
    assert_eq!(first.type_path, second.type_path);
    assert!(first.is_identified() && second.is_identified());
    assert!(!first.same_build(&second) && !first.reads_as(&second));
    assert!(first.code_differs(&second));

    // A plugin of unchanged sources is the same code as the compiled crate, and a list still shows that a plugin ran.
    let compiled = RecordedBuild::from(registered_by(model_crate(Some("false"), Some("00000000000000aa"))).source());
    let plugin = RecordedBuild::from(plugin_life("00000000000000aa").source());
    assert!(compiled.same_build(&plugin) && !compiled.code_differs(&plugin));
    assert!(!compiled.reads_as(&plugin) && !plugin.reads_as(&compiled));
    assert_eq!(first.describe(), format!("runtime code {:016x}", life.hash()));
    assert_ne!(first.describe(), second.describe());

    let mut unhashed = first.clone();
    unhashed.code_hash = None;
    assert!(!unhashed.is_identified());
    assert!(
        unhashed.reads_as(&unhashed.clone()),
        "a list shows an unidentified build once"
    );
    assert!(!unhashed.same_build(&unhashed.clone()));
    assert_eq!(unhashed.describe(), "runtime code (unidentified)");

    let scratch = ScratchDir::new("two-codes");
    crate::tests::support::sweep(
        &runtime_life(&life),
        None,
        &life_spec(1),
        scratch.path(),
        Concurrency::Auto,
    );
    crate::tests::support::rewrite_manifest(scratch.path(), |recorded| {
        let mut session = recorded.sessions[0].clone();
        session.model_source = Some(RecordedBuild::from(&ModelSource::runtime(
            "game_of_life",
            edited.clone(),
        )));
        recorded.sessions.push(session);
    });
    let builds = manifest(scratch.path()).recorded_builds(BuildRole::Model);
    let hashes: Vec<Option<u64>> = builds.iter().map(|build| build.code_hash).collect();
    assert_eq!(hashes, [Some(life.hash()), Some(edited.hash())]);
}
