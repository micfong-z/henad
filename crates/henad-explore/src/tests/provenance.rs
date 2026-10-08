//! Checks that every session records the builds it ran, and that a resume or a merge warns when a build changed.

use std::path::PathBuf;

use henad_compute::entry::{ModelEntry, ModelSet, register_grid_model};
use henad_core::authoring::model::grid_model::GridModel as _;
use henad_core::explore::plan::Shard;
use henad_core::explore::spec::SweepSpec;
use henad_core::provenance::BuildInfo;
use henad_models::game_of_life::GameOfLifeModel;

use crate::exec::Concurrency;
use crate::merge::merge;
use crate::output::manifest::{BuildRole, Manifest, RecordedBuild};
use crate::output::{MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::result_set::ResultSet;
use crate::sweep::{Provenance, SweepOptions, SweepWarning};
use crate::tests::support::{
    OutputTables, Recorder, ScratchDir, entry, manifest, other_engine, provenance, sweep_options, sweep_with,
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
