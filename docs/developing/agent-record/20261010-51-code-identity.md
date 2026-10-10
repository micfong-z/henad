---
date: 2026-10-10
title: "ModelSet::replace and code identity"
description: "Milestone M2 of #49. ModelSet::replace puts an entry in place of the one with its id, the five register functions go through one crate-private constructor from values, and every model source records whether its code is compiled, composed at run time or loaded from a plugin. A resume, a search resume and a merge reject runtime or plugin code that changed, and compiled models keep their warning."
icon: material/fingerprint
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#49"
state: "replace, the entry constructor, code origins, the code hash, inline code in manifests and the three rejections in place, reviewed and fixed, checks green, uncommitted on top of 2f62d9c"
baseline_commit: 2f62d9c
delta_state: "uncommitted on `49/m2-replace-code-identity`, 25 changed files and this record"
---

# ModelSet::replace and code identity

> Milestone M2 of #49 (dynamic model loading), decisions 7 and 10 of its plan.
> A host that swaps the code behind an id needs a set that takes an entry in place of another, and a results folder that can tell two versions of a model's code apart.
> `ModelSet::replace` puts an entry at the position of the one with its id and returns the old one, or appends it.
> The five `register_*` functions now fill a crate-private `EntryDeclarations` and call one of two crate-private constructors, `ModelEntry::new_cpu` and `new_gpu`.
> henad-core gains `CodeOrigin` (compiled, runtime, plugin) and `ModelCode`, the canonical text of a runtime model with its FNV-1a hash, and `ModelSource` records both.
> A manifest records each model build's origin and code hash, and a runtime model's text inline, as optional fields at format version 1.
> When either side is runtime or plugin code, a sweep resume, a search resume and a merge compare code identity alone and reject a change, while compiled builds keep the `BuildChanged` warning.
> The 0.2.0 fixture tests, the schema hashes, the CLI goldens and the byte-identity sweep tests pass unchanged.
> Two reviews followed, and the session fixed what they found: a merge compares every pair of shards before the plan, a resume checks the code before its probe build, a plugin is listed apart from the compiled build of its sources and never takes a set's build, and two helpers went crate-private.

## State before

`49/m2-replace-code-identity` was created from `49-dynamic-loading` at `2f62d9c`, which holds round 1 as two placeholder commits: D1, the dylib core, and M1, the owned descriptors (record #50).
M1's owned descriptors let a model read at run time declare its own parameters and stats, and M2 is the first milestone of round 2 to build on them.

A `ModelSet` had `insert`, `extend`, `get`, `lookup` and `runnable`, and nothing that replaced an entry.
Each `register_*` function filled the private `EntryParts` itself, five near-copies of one literal.
A `ModelSource` held a type path and, once a set stamped it, the registering crate's build.
A model composed at run time would therefore record the host's build, and two versions of its code would record the same build: an edited model resumed into an old folder with no error and no warning.
`RecordedBuild::same_build` compared package, version, engine crates, then commits, then source hashes.

## What was done

### henad-core: origins and model code

`provenance.rs` gains two types and three fields' worth of identity on `ModelSource`.

- `CodeOrigin` is `#[non_exhaustive]` with `Compiled`, `Runtime` and `Plugin`, and `as_str` names each as a manifest records it.
- `ModelCode` holds a format name, the canonical text as an `Arc<str>`, and a hash.
  The hash is FNV-1a over the length-prefixed tag `henad.model-code.1`, the format and the text, through the `Fnv1a64` that the schema and plan hashes use.
  Its `Debug` prints the format, the text's byte length and the hash, and leaves the text out.
- `ModelSource` holds an origin and an optional `ModelCode` beside its type path and build.
  `ModelSource::runtime(label, code)` returns a runtime source typed `runtime:<label>` with no build.
  `origin()`, `code()` and `code_hash()` read them, the last returning the code's hash for runtime code and the registering crate's source hash for compiled and plugin code.
  The hidden `__into_plugin` marks a compiled source as a plugin's and leaves a runtime source alone.
  The derived equality covers the origin and the code.

`model_code_hash_is_pinned` pins one code's hash to a literal, which a Python script recomputed independently of the Rust code before it was written in.
`model_code_hash_covers_format_and_text` checks that the format, the text and the length prefixes each change the hash, and `a_runtime_source_names_its_code_and_no_build` covers the accessors.

### henad-compute: one constructor from values, and replace

`entry/mod.rs` gains the crate-private `EntryDeclarations` (id, name, description, parameters, stats, actions, topology hint, structure, palette, replay flag) and two crate-private constructors.
`ModelEntry::new_cpu(declarations, source, build)` wraps the build in `catching`, and `ModelEntry::new_gpu(declarations, source, needs, build, demand)` wraps it in `catching_on` and returns the no-device fault without a device.
Both derive the metadata's backend, so an entry's backend, needs and capacity check always agree.
The five `register_*` functions fill the declarations from the trait consts and call one of the two, and none of them builds `EntryParts` any more.
The constructors are generic over the build closure, and the engines still monomorphise in the crate that calls `register_*`.
The factory gains one closure call per build, never per tick.

`an_entry_from_values_reads_like_the_registered_one` builds a test grid model with a parameter, a stat, an action and an owned palette from hand-written declarations, and compares it with `register_grid_model` of the same type: the whole `Debug` form (declarations, metadata, needs and source), the schema hash, and the stats of four steps on one seed.

`ModelSet::replace(entry)` returns `Result<Option<ModelEntry>, ModelSetError>`.
It checks the id grammar, stamps the set's build as `insert` does, and swaps the entry in place or pushes it.
`insert` and `replace` share a private `stamped`, which leaves a runtime or plugin source without the set's build.
The text identifies runtime code, and a plugin's identity is the source hash of its own crate, never the host's.
The hidden `ModelEntry::__into_plugin` marks an entry's source as a plugin's, and the hidden `ModelEntry::__with_code(code)` gives an entry a runtime source labelled with its id.
The second exists for tests, which give a registered model runtime code with it.

The set tests gain `replace_keeps_the_position_and_returns_the_old_entry`, `replace_appends_a_new_id`, `replace_refuses_an_id_outside_the_grammar`, `a_runtime_source_is_never_stamped_with_the_set_build` and `a_marked_plugin_source_reads_as_plugin`.
The last also replaces an entry marked as a plugin's before any set stamped it, and the source keeps no build and no code hash.

### henad-explore: identity in results and the three rejections

`RecordedBuild` is `#[non_exhaustive]` and gains `origin` (`runtime` or `plugin`) and `code_hash` (16 hexadecimal digits), both `#[serde(default, skip_serializing_if = "Option::is_none")]`.
`RecordedBuild::from(&ModelSource)` writes both for runtime and plugin code alone, so a compiled model's manifest is unchanged byte for byte in its keys.
`ManifestModel` gains `code: Option<ManifestModelCode>`, the format, hash and canonical text of a runtime model, skipped when absent.
`FORMAT_VERSION` stays 1, as the plan fixes.

The comparison rule:

- `same_build`: when either build records an origin, the two compare by code identity alone, a private `CodeIdentity` of `Runtime(code hash)` or `Rust(source hash)`, both known and equal.
  Compiled against compiled is unchanged.
- The crate-private `code_differs` is the rejection rule: either build has an origin, and the two are not the same build.
- `is_identified` requires the identity for a build with an origin, and `describe` reads `runtime code <hash>` for runtime code and puts `plugin` and the source hash in a plugin's parentheses.
- `reads_as` requires the same origin and code hash on top of the same build, so a list shows a plugin apart from the compiled build of its sources, and compares both between two unidentified builds.
- The private `code_origin()` reads the origin back as a `CodeOrigin`, `None` for a name this build does not know, which counts as other code with no identity.

`Manifest::run_model_builds` returns the model build of each session that wrote runs, the selection `recorded_builds` already made, now shared through a private `run_sessions`.
A session without a model build, as in a 0.2 folder, is `None`, and counts as other code against a runtime or plugin build.

- **Sweep resume.** `check_code` in `output/resume.rs` runs after the model id and before the schema hash, so an edit that also changed the declarations reports the code.
  `ResumeScan::read` takes the resuming model's `ModelSource` for it.
  It returns `ResumeError::CodeChanged { recorded, current }`, each a text naming the origin, the package and the hash, as in `plugin code of life-crate 1.0.0, sources 00000000000000aa`.
  `SweepPreparation::new` also calls the crate-private `check_directory_code` (mode, model id, code) on the folder before its probe build and its measure plan, so the probe never builds changed code.
  The spec is planned before either check, and a spec that names a parameter or a stat that the edit removed still fails its plan first.
- **Search resume.** `RecordedSearch::read` runs the same check at the same point, and `SearchPreparation::new` calls `check_directory_code` before its probe build.
- **Merge.** `check_shards` checks the model id (as `PlanDiffers`), then calls `check_code`, then the plan, schema, replicate and shard-count checks, so a merge reports changed code first, as a resume does.
  `check_code` compares every model build of each shard with every build of each lower shard and returns `MergeError::CodeChanged { dir, lower_dir, recorded, current }` before anything is written.
  A comparison with the lowest shard alone missed a pair: a compiled shard of sources `aa`, a plugin shard of `aa` and a compiled shard of `bb` each match the first shard, and the plugin shard and the last one differ.

`schema_json` writes `origin` and `code_hash` only for a model that is not compiled, so `--params --json` of every example model is unchanged and `tests/golden.rs` passes as it was.
`ResultSet::code_matches(&ModelSource)` answers the rejection rule for a host, true whenever folder and source hold compiled code alone.

Tests, in `src/tests/provenance.rs` unless named otherwise:

| Test | What it holds |
|---|---|
| `a_value_built_entry_sweeps_byte_identically_to_the_registered_one` | Game of Life with runtime code writes the three CSV files that the registered entry writes, and the same schema hash |
| `a_resume_refuses_runtime_code_that_changed` | The same runtime code resumes with no warning. Edited code is rejected and the folder keeps its manifest and `runs.csv`. Runtime against compiled is rejected both ways. Edited code whose build always faults is rejected as changed code, which pins the check before the probe build |
| `a_resume_refuses_a_plugin_whose_sources_changed` | A plugin of unchanged sources resumes, one of changed sources is rejected with its package and hash in the message, and one with no source hash is always rejected |
| `a_plugin_with_unchanged_sources_resumes_a_compiled_folder` | Compiled and plugin builds of one source hash resume each other with no warning, and other sources are rejected |
| `a_compiled_build_change_still_only_warns` | Two compiled builds of other commits and sources resume with one `BuildChanged` warning |
| `a_merge_refuses_shards_of_two_codes` | Shards of one runtime code merge and keep the code inline. Shards of two codes are rejected, naming both folders, and no output folder is made. Two codes and two plans report the code. Compiled `aa`, plugin `aa` and compiled `bb` shards are rejected on the last two |
| `a_manifest_carries_runtime_code_inline` | Format version 1, `model.code`, the session's origin, code hash and `runtime:` type path, the schema's origin, `code_matches` either way, and no `code`, `origin` or `code_hash` key in a compiled model's manifest |
| `two_codes_under_one_label_are_listed_apart` | Two runtime builds under one label: identified, not the same, not read alike, described apart, and listed twice by `recorded_builds`. A compiled and a plugin build of one source hash are the same build and are not read alike. An unhashed one is unidentified |
| `a_search_resume_refuses_changed_code` (`src/tests/search.rs`) | An interrupted search of runtime code rejects edited code whose build always faults, with the folder untouched, then resumes with its own code to the end |
| `schema_json_names_code_only_for_runtime_models` (`schema.rs`) | No origin for a compiled model, `runtime` and the code hash for runtime code with the schema hash unchanged, `plugin` and the source hash for a plugin |

`tests/support.rs` gains `unbuildable`, which wraps an entry's factory in one whose build always faults, for the two resume tests that name it.

### henad-app

The app reads `RecordedBuild` in the Results tab's `compare_builds` and `ModelSource` in the About window.
`compare_builds` needed no change: through the new `same_build`, a plugin of unchanged sources reads as unchanged and a runtime edit as changed.
The About window's Models row printed an empty package and "unknown build" for a runtime source, and now reads `<id> (runtime code <hash>)`, with `plugin` beside a plugin's build.
`runtime_and_plugin_models_name_their_code` tests both rows.
The install path, which replaces an entry in `AppState.models` and deals with what the swap leaves behind, waits for M5.

### The facade and the docs

The facade re-exports `CodeOrigin` and `ModelCode` at the root and `ManifestModelCode` under `henad::explore`, and `tests/facade_paths.rs` names the three.

- `docs/authoring/model-sets.md`: `replace` under Ids, with an example that replaces an example model under its own id, and an Origins subsection under Sources with the three origins, what identifies each, and the rejection.
  A plugin's source keeps its crate's build, and a set records none on it.
  `docs/authoring/registering.md` is a redirect stub to that page and is unchanged.
- `docs/guide/sweeps.md`: the rejection in Resuming a sweep, and one line under Sharding a sweep.
- `docs/reference/cli.md`: `model.code` in the manifest table, `origin` and `code_hash` in the Builds table, the identity rule, the rejection under Resuming, Shards and merging and Resuming a search with the order of the checks, and the two fields of `--params --json`.
- `docs/reference/api.md`: the two new root items.
- `docs/guide/library.md`: the provenance paragraph names the code hash and `code_matches`.
- `docs/developing/releasing.md`: the hidden items gain `ModelEntry::__into_plugin` and `__with_code`, and henad-compute's use of `ModelSource::__into_plugin`.
- `CHANGELOG.md`: two Added lines and two breaking Changed lines with their migration notes under Unreleased.
- `AGENTS.md`: the henad-core provenance paragraph, the henad-compute entry paragraph, a code-identity paragraph with its keep-list after the henad-explore provenance paragraph, and the About window's rows.

### Edited tree

```text
.
├── AGENTS.md, CHANGELOG.md, zensical.toml       ~ code identity, replace, nav #51
├── crates/
│   ├── henad-core/src/provenance.rs             ~ CodeOrigin, ModelCode, ModelSource origin and code, tests
│   ├── henad-compute/src/entry/
│   │   ├── mod.rs                               ~ EntryDeclarations, new_cpu, new_gpu, register_* over them,
│   │   │                                          __into_plugin, __with_code, an_entry_from_values_… test
│   │   └── set.rs                               ~ replace, runtime and plugin sources unstamped, five tests
│   ├── henad-explore/src/
│   │   ├── output/manifest.rs                   ~ RecordedBuild origin and code_hash, CodeIdentity, code_differs,
│   │   │                                          reads_as, run_model_builds, ManifestModelCode
│   │   ├── output/resume.rs                     ~ ResumeError::CodeChanged, check_code, check_directory_code,
│   │   │                                          ResumeScan::read takes the source
│   │   ├── search_run.rs                        ~ the same checks on a search resume
│   │   ├── merge.rs                             ~ MergeError::CodeChanged, check_code over every pair, before the plan
│   │   ├── sweep.rs                             ~ model.code in the manifest, check_directory_code before the probe
│   │   ├── schema.rs                            ~ origin and code_hash for non-compiled models, test
│   │   ├── result_set.rs                        ~ code_matches
│   │   ├── tests/provenance.rs, tests/search.rs ~ eight and one tests
│   │   └── tests/support.rs                     ~ unbuildable
│   ├── henad-app/src/ui/about.rs                ~ runtime and plugin Models rows, test
│   └── henad/                                   ~ CodeOrigin, ModelCode, ManifestModelCode re-exported, facade_paths
└── docs/
    ├── authoring/model-sets.md                  ~ replace, origins
    ├── guide/sweeps.md, guide/library.md        ~ the rejection, code_matches
    ├── reference/cli.md, reference/api.md       ~ manifest and build fields, rejections, root items
    └── developing/
        ├── releasing.md                         ~ hidden items
        └── agent-record/
            └── 20261010-51-code-identity.md     +
```

## State after

Everything is uncommitted on `49/m2-replace-code-identity`, on top of `2f62d9c`, in the worktree `.claude/worktrees/49-m2`.
Nothing was committed, pushed or tagged.

All checks ran on the final tree, after the review fixes:

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: the wasm32 typechecks, fmt of the workspace and the excluded packages, packaging, cargo-deny, clippy, 1250 Rust tests over 41 test binaries and the doc tests with none failed and 6 doc tests ignored as before, the README program, the docs with `-D warnings`, the 18 script tests and the web build.
  The one warning is the future-incompatibility note on naga, wgpu, wgpu-core and wgpu-hal, as on `master`.
  A rename of one local binding in `merge.rs` landed while that run's clippy started, and a clippy of henad-explore alone ran clean on the final file.
  The implementer's two runs before the review passed as well.
- Among the workspace tests, and passing unchanged: `schema_hashes_are_unchanged_since_0_2_0`, `a_0_2_0_manifest_still_resumes`, `a_0_2_0_manifest_still_replays_and_merges`, henad-cli's `tests/golden.rs` and `tests/json_lines.rs`, the tutorial's `snippets`, `parity` and `shaders` tests, `the_readme_program_matches_the_example`, and the byte-identity sweep tests.
  `a_location_survives_a_panic_on_a_rayon_worker`, which flakes at the base commit, passed.
- The two new resume cases were checked against the code without the fix: with the early check removed, `a_resume_refuses_runtime_code_that_changed` and `a_search_resume_refuses_changed_code` fail with `EveryConfigFaulted` from the probe.
- `HENAD_REQUIRE_GPU=1 ./scripts/check_hot.sh` passes: the lints of the excluded packages, `hot-check` with every check `ok`, the plugin models stepping as the static ones and the kit passing on them included, then `hot-check relink`.
- `uv run zensical build` reports no issues.
- The scratch directory `/Users/micfong/.claude/jobs/3144150c/tmp/m2-fix/` held the check logs alone, and is deleted.
  No template copy was built: the template names no item that changed.
- The app was not run. The one UI change is the About window's Models rows, which a unit test covers, and the install path that would show them waits for M5.

Proposed commit: `feat: ModelSet::replace and code identity`.

M2 has no benchmark gate.
No per-tick path changed: the factory gains one closure call per build, and the manifest, resume and merge checks run once per sweep.

## Issues found & future directions

- **The value-built entry in henad-explore's test is the registered entry with runtime code.**
  The constructor from values is crate-private, as the plan keeps it until #37, so henad-explore cannot call it.
  `a_value_built_entry_sweeps_byte_identically_to_the_registered_one` therefore sweeps `register_grid_model` (which goes through `new_cpu`) given a runtime source through `__with_code`, against the plain registered entry.
  The constructor itself is held to the registered entry in henad-compute by `an_entry_from_values_reads_like_the_registered_one`.
- **Nothing marks a plugin's entries yet.**
  `ModelEntry::__into_plugin` is in place, and henad-hot's loader does not call it.
  The plan lists the marking in M8's Apply sequence, and M2 leaves henad-hot alone.
  D2 changes it in the same round.
  Until then a plugin's entries record compiled code, which a resume compares as compiled.
- **A 0.2 folder rejects any runtime or plugin model.**
  Its sessions record no model build, and an unrecorded build counts as other code against a runtime or plugin build, by the rule "both known and equal".
  A runtime model that shadows an example model's id therefore never resumes a 0.2 folder of that model.
- **An origin this build does not know is always rejected.**
  `code_origin` returns `None` for it, and such a build has no identity.
  A later Henad that adds an origin is read by 0.4 as other code.
- **A 0.3 binary can still resume a 0.4 folder of runtime code.**
  The plan keeps format version 1, and 0.3 ignores `origin` and `code_hash`.
  0.3 compares the runtime build as a compiled one with an empty package, and warns that the model build changed, which is the most it can say.
- **A resume plans its spec before it checks the code.**
  `run_spec`, `plan_spec` and `SweepRun::start` plan the spec before the folder is read, so a spec that names a parameter or a stat that a runtime edit removed fails its plan, and the next attempt reports the changed code.
  Checking the code first would need the folder read in each of those entry points, and in `SweepStartError` a variant for it.
- **The design's `Result<_, String>` build closure is not here.**
  `new_cpu` and `new_gpu` take closures that return the state, the shape every `register_*` function needs.
  A build that refuses its values, as a runtime rule might, would need the `Result` form, which M4 can add with its first caller.
- **`ResultSet::model_code` is not here.**
  The design lists it for rebuilding a model from a folder, which the plan leaves out of #49.
  `manifest().model.code` holds the text for a host that needs it.
- **In-memory sweep results compare no builds.**
  `ResultsStore::for_sweep` records no changed builds, so after a replace the Results tab replays a finished in-memory sweep through the new code without a note, as research-code-map.md's entry-layer §3 lists.
  The install path in M5 owns that.

### Deviations from the plan

- The hidden markers are `ModelSource::__into_plugin`, `ModelEntry::__into_plugin` and `ModelEntry::__with_code`, in place of the design's `__with_origin(origin)`.
  An origin alone cannot make a source runtime, which needs its code, and the two names cover the two uses.
- `ModelSource::runtime` takes the label as `&str`, since the type path is built by formatting it.
- `RecordedBuild` is `#[non_exhaustive]`, as the design's API block has it.
  Adding the two fields already broke struct literals, and no crate outside henad-explore built one.
- The About window's Models rows changed, a small app change ahead of M5, since a runtime entry otherwise read as an unknown build.
- A merge compares every pair of shards, where design.md §1.6 compares each shard with the lowest shard that records builds.
  The design's rule misses a plugin shard between two compiled shards of other sources.
- `MergeError::CodeChanged` names both folders, `dir` and `lower_dir`, where the design's variant names one `shard`.

### Review findings

Two reviews read the first version of this milestone. The session fixed:

- a merge that compared each shard with the lowest shard alone, and so accepted a compiled shard of sources `aa`, a plugin shard of `aa` and a compiled shard of `bb` that a resume of the same sessions rejects;
- a merge that checked the code after the plan, unlike a resume, so two runtime codes that also changed the plan reported `PlanDiffers`;
- `reads_as`, which merged a plugin and the compiled build of its sources into one listed build and hid that a plugin ran;
- a resume that built the edited model in its probe before it checked the code, now checked first by `check_directory_code`;
- `stamped`, which gave the host's build to a plugin source that had none yet, and so made the host's sources the plugin's code identity;
- `code_differs` and `code_origin`, public with no caller outside henad-explore, now crate-private and private;
- the `__with_code` doc, which said the build was unchanged;
- the CHANGELOG's breaking lines, which lacked migration notes;
- AGENTS.md's keep-lists, which lacked `a_value_built_entry_sweeps_byte_identically_to_the_registered_one` and a "Keep it" for `an_entry_from_values_reads_like_the_registered_one`;
- a cleft sentence in this record.

No finding was rejected.
For the resume, the fix stops short of what one review asked: the spec is still planned before the folder is read, as the issue above records.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
