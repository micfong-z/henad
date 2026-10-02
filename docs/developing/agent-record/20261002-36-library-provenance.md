---
date: 2026-10-02
title: "Library M7: provenance"
description: The seventh milestone of #48. henad-build stamps each crate with its commit, a dirty flag and a hash of its sources, every manifest session records the engine, host and model builds that ran it, and a resume, a merge or a replay warns when the engine's or the model's build changed.
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M7 implemented and its review folded in, `HENAD_REQUIRE_GPU=1 ./check.sh` and the docs build green, the cargo-level freshness checks and step 15 run by hand
baseline_commit: ba46f64
delta_state: uncommitted on `48-library`
---

# Library M7: provenance

> M7 is the seventh of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> `henad_build::stamp_commit()` stamps the crate whose build script calls it with its commit, a dirty flag and an FNV-1a hash of its sources, read from a package's `.cargo_vcs_info.json` or from git, and the hash alone outside both.
> henad-explore's build script stamps `ENGINE_BUILD` over the four engine crates, and henad-compute and henad-models record their own source hashes.
> Every manifest session now records three builds, `engine`, `host` and `model_source`, and a resume, a search resume or a merge warns `BuildChanged` for each engine or model build that differs from the current one, while the host is recorded and never compared.
> A 0.2.0 sweep folder, recorded from the `v0.2.0` tag, resumes twice with one engine warning each time, and its runs match a fresh sweep byte for byte.
> The schema, plan and run hashes are unchanged.
> A review before the commit added `.gitattributes` for the byte-compared reference files, took the stamps off `.git/index.lock` and onto git before 2.31, sent a build with an unknown dirty flag to its source hash, and gave a merge's warning its own wording.

## State before

`48-library` stood at ba46f64, M6's commit, with a clean tree.
henad-cli and henad-app each stamped `HENAD_COMMIT` and `HENAD_COMMIT_DATE` from `git rev-parse` in their own build scripts, watching `<git-dir>/HEAD` and the loose branch ref.
`BuildInfo`, `build_info!` and `ModelSource` existed since M3, but no build script stamped them, and their commit, dirty flag and source hash read as unknown.
The manifest recorded one `ManifestEngine` block, rebuilt on every resume, and each session one commit string.
A resume compared the top-level engine block's commit with the host's, through `SweepWarning::CommitChanged`.
`Provenance` was a struct of public fields with a `Default`.

## What was done

### The stamps (henad-build)

`crates/henad-build/src/stamp/` holds the three stamps of [4.9.2].

- `stamp_commit()` for a host or model crate, and the hidden `stamp_engine_commit()` for henad-explore and `stamp_source_hash()` for henad-compute and henad-models. Each reads `CARGO_MANIFEST_DIR`, computes a `Stamp` for its `StampScope`, and prints `HENAD_BUILD_COMMIT`, `HENAD_BUILD_COMMIT_DATE`, `HENAD_BUILD_DIRTY` and `HENAD_BUILD_SOURCE_HASH` in the single-colon form. The engine stamp adds `HENAD_BUILD_CRATE_HASH` and `HENAD_BUILD_STAMP_VERSION`, henad-build's own version.
- A package with a `.cargo_vcs_info.json` takes `git.sha1` cut to eight characters and `git.dirty`, with an empty date. Under git the full `rev-parse HEAD` is cut the same way, since `--short=8` lengthens an ambiguous hash. The file is read by its two keys, without a JSON parser.
- Otherwise git, only when `git ls-files --error-unmatch Cargo.toml` succeeds in the crate's directory. The engine stamp keeps git's answer only when `--show-prefix` prints `crates/henad-explore/`, and reaches its siblings through `--show-toplevel`. `scripts/check_packaging.sh` refuses a `"../` path in `stamp/`.
- Every git command runs with `GIT_OPTIONAL_LOCKS=0`, and never takes `.git/index.lock` to refresh the index.
- The dirty flag is `git status --porcelain=v1 -z --untracked-files=all --no-renames` over `src`, `Cargo.toml` and the nearest lockfile, each path checked against the exclusions below the pathspec it falls under. A symlink to a directory or to nothing is left out (`is_link_to_no_file`), as the hash leaves it out.
- The source hash lists `git ls-files --cached --others --exclude-standard -- src` under git and walks `src` outside it. Both drop any path with a dotfile component, a name ending in `~` or `.swp`, and anything that is not a file once symlinks are followed. Files are hashed in label order, CRLF read as LF, a file that vanishes between the listing and the read skipped, with the manifest labelled `Cargo.toml` whether it is `Cargo.toml.orig` in a package or `Cargo.toml` outside one, and the nearest `Cargo.lock` outside a package.
- The watch list: `HEAD` and `logs/HEAD` through `--git-path --path-format=absolute`, or, on a git before 2.31 that echoes the option back (`echoes_path_format`), through `--git-path` with the path joined onto the crate's directory, the branch's loose ref through `--git-path refs/heads/<branch>`, `<common-dir>/packed-refs`, `<common-dir>/refs/heads` when `logs/HEAD` is missing, `<common-dir>/reftable` in place of the refs, and `src`, `Cargo.toml` and the lockfile, each one that exists.

`stamp_source_hash` lists the crate's files through git when git tracks the crate, and watches no git path and no lockfile.

### The stamps in the crates

- henad-explore gained `build.rs` calling `stamp_engine_commit`, henad-build as a build dependency, `exclude = ["/tests/"]`, and `ENGINE_BUILD` with the crate-private `CRATE_HASH` and `STAMP_VERSION` in `lib.rs`.
- henad-compute's and henad-models' build scripts call `stamp_source_hash` before their shader builds. henad-compute holds the hidden `__COMPUTE_BUILD`, and henad-core the hidden `__VERSION`. henad-models' `shader_build` docs region now covers the `ShaderBuild` line alone, so the hidden call stays out of the GPU Game of Life page.
- henad-cli and henad-app call `stamp_commit` in place of their git code, and henad-cli takes henad-build as a build dependency. The CLI's `provenance()`, the app's sweep session and its About window read `build_info!()`, through `HOST_BUILD` in the app. The About window adds a Sources row and marks a modified build.

The design gives each of henad-core, henad-compute and henad-explore a hidden version const. Only henad-core needed one: henad-compute's version comes from `__COMPUTE_BUILD` and henad-explore's from `ENGINE_BUILD`.

### The manifest and the comparisons (henad-explore)

- `RecordedBuild` replaced `ManifestEngine`. It keeps the five 0.2 keys, with `package` under `name`, and adds `dirty`, `source_hash`, `type_path`, `crate_hashes` and `crate_versions`, all defaulted. Hashes are written as 16 hexadecimal digits. `From<&BuildInfo>` and `From<&ModelSource>` fill it, and `RecordedBuild::engine()` records `ENGINE_BUILD` as `henad` with the four engine versions and the hashes of henad-compute and henad-explore.
- `ManifestModel` gained `replays_exactly` with `default_true`. `ManifestSession` gained `engine`, `host` and `model_source`, written by `running_manifest`. The top-level `engine` block names the latest build.
- `RecordedBuild::same_build` follows the six steps of [4.9.3]. A crate version or hash is compared only for a package both sides record, and a build with `dirty: null` counts as clean in step 4.
- `Manifest::recorded_builds(role)` lists the distinct builds the sessions record, a 0.2 session reading as the engine build of its commit and the top-level block's version. `record_session_engines` writes that build into each 0.2 session when a resume or a merge writes the manifest. Without it the top-level block, rewritten by the resume, would lend the 0.2 session the new version on the next resume.
- `SweepWarning::BuildChanged { role, recorded, current }` replaced `CommitChanged`, with both builds boxed, since two inline `RecordedBuild`s trip `large_enum_variant`. Its text names the role and both builds, the engine crate that differs when the two read alike, and for a model build that records neither a commit nor a hash, the advice to call `stamp_commit`. `build_warnings` serves the sweep and the search resume, which now take the inputs in `announce`. A merge warns for each build that differs from the lowest shard's.
- `Provenance` is `{ engine, host, arguments }`, private, built by `Provenance::new(host, arguments)`, with a test-only `with_engine`.
- `ResultSet::recorded_builds` reads the builds back.

### The hosts

- `henad-cli --json` prints each `SweepWarning` as an `explore_warning` line beside its text on stderr, merges included: `warning`, `message`, and for `build_changed` the `role` and both builds.
- The app's run details record `host` and `model_source`.
- `ResultsStore` holds `changed_builds`, the roles whose current build differs from any recorded build, and `replays_exactly`, false when the entry or the manifest declares it. The detail strip warns "Henad build changed since this sweep. The replay might differ." (or the model's, or both) where it warned about changed parameters, and notes "This model does not replay exactly. The opened run might differ from its row." beside Open.

### The 0.2.0 fixture

`crates/henad-explore/tests/fixtures/manifest-0.2.0/` holds a four-run SIR sweep with an outbreak action, written by the v0.2.0 binary at 773a7a5 in a detached worktree, with the procedure in its `README.md`.
`a_0_2_0_manifest_still_resumes` copies it, checks that it reads `replays_exactly` and records no session builds, then resumes it to 3 and to 4 replicates.
Each resume gives one `BuildChanged` with role `Engine`, recorded version 0.2.0 and commit `773a7a5b`, and the 0.2 build stays in `recorded_builds`.
The resumed folder then equals a fresh sweep of 4 replicates, timings aside, so the runs 0.2.0 wrote are the runs this build writes.

### Tests

- henad-build (`tests/stamp.rs`): `a_commit_changes_a_watched_path_under_packed_refs_and_in_a_worktree` packs the refs of a scratch repository, commits, then adds a linked worktree and commits there, and checks each time that a watched git path changed and that a new stamp names the new commit. `a_dotfile_under_src_changes_no_hash` covers an ignored dotfile, an untracked and unignored `src/.DS_Store`, a `.swp` and a `~` backup, and that an untracked kernel file marks the tree dirty. `an_uncommitted_edit_marks_the_tree_dirty_and_changes_the_hash` covers a source and a lockfile edit. `a_crate_outside_git_records_its_hash_alone`, `a_crlf_checkout_hashes_like_lf`, `the_engine_stamp_covers_its_siblings_only_in_henads_tree`, `a_source_hash_stamp_reads_no_git_commit` and `vcs_info_reads_the_commit_and_the_dirty_flag`.
- `the_engine_stamp_reads_cargo_vcs_info` runs `cargo package --no-verify --allow-dirty --offline` on the four engine crates into a scratch target, unpacks henad-explore's tarball, and checks the commit, the dirty flag and an empty date against the tarball's `.cargo_vcs_info.json`, and the tarball's crate hash against the checkout's. henad-explore alone does not package: Cargo resolves a lone package's path dependencies from the registry.
- henad-explore (`tests/provenance.rs`): every test [4.9.3] names but the CLI's and the app's, with `a_merge_of_shards_under_another_engine_build_warns` as the merge counterpart, `an_example_model_resume_from_an_unchanged_checkout_raises_no_warning`, `a_dry_run_of_a_resume_under_the_same_build_warns_nothing` and the fixture test. The edits are builds written by hand: a model crate's `BuildInfo` with another source hash, or `RecordedBuild::engine()` with another crate hash or version.
- `two_unidentified_builds_are_never_the_same` and `a_recorded_build_writes_its_hashes_in_hexadecimal` in `manifest.rs`, the second reading a 0.2 engine block.
- `a_dry_run_counts_the_runs_a_resume_skips_and_changes_nothing` expects `BuildChanged` with role `Engine` in place of `CommitChanged`, against `other_engine()`, a build that differs from Henad's whether the tree is clean or dirty. Its counts of 3 and 3 are unchanged.
- henad-cli: `a_manifest_from_the_cli_entry_records_its_host` (`tests/manifest.rs`) runs the binary, since `henad_cli::run` arrives in M8. It checks the host, engine and model builds, then resumes under `--json` and expects no `explore_warning` line. `a_build_change_prints_as_an_explore_warning_line` checks the line's fields.
- henad-app: `a_replay_warns_on_a_build_from_any_session` sweeps and resumes, finds no change, then edits the first of two sessions' engine version and `replays_exactly`, and expects the engine warning and the note.

### Checks by hand

- **Workspace freshness.** After one build, a second `cargo build --workspace --all-targets -v` reported all seven henad crates Fresh, with no Dirty line and no build script run.
- **Verified packaging.** `cargo package --workspace --locked --allow-dirty` into a fresh `CARGO_TARGET_DIR` verified all seven crates from their tarballs. henad-explore's tarball ships `build.rs` and no `tests/`.
- **Downstream.** A scratch crate under `target/`, ignored by git and so outside it for the stamp, depends on `=0.2.0` of henad-core, henad-compute, henad-explore and henad-build through `[patch.crates-io]` on the unpacked tarballs. `cargo metadata --locked` resolved all four with a null source. Its `build.rs` calls `stamp_commit`, and it registers one CPU grid model and runs `run_spec` from a small `main`. A second `cargo build -v --locked` reported every crate Fresh. Appending a comment line to `src/lib.rs` rebuilt that crate alone, and all four henad crates stayed Fresh.
- **Step 15.** The downstream binary swept two runs, then resumed to two replicates, printing each warning as an `explore_warning` line. The resume printed none. The manifest recorded the engine as commit `ba46f64b`, dirty from `--allow-dirty`, and the model as `vote-model` with an empty commit and its source hash. After a second edit to `src/lib.rs`, a resume printed one `build_changed` line naming both source hashes. The binary stands in for `henad_cli::run`, which arrives in M8, and the template, which arrives in M10d.
- **The app**, through the egui MCP server, on the second monitor with `app.ron` backed up and restored. The 0.2.0 fixture's copy opened in the Results tab and showed "Henad build changed since this sweep. The replay might differ." under a selected run. Open at end replayed run 1 to tick 20 at Susceptible 0, Infected 156 and Recovered 100, the values 0.2.0 wrote. A gpu_boids sweep folder showed "This model does not replay exactly. The opened run might differ from its row." and no build warning. The About window read the commit as "ba46f64b (2026-10-02), modified", with the host's source hash in its Sources row.

### The M7 review

The maintainer reviewed M7 before its commit and listed seven items.

1. **`.gitattributes`** sets `-text` on `crates/henad-explore/tests/fixtures/**` and `crates/henad-cli/tests/golden/**`. Every file there reads `text` unset, `git hash-object` gives the same hash with and without filters, `git ls-files --eol` reads `i/lf w/lf attr/-text` for the tracked golden files, and none holds a CR byte. `git -c core.autocrlf=true cat-file --filters` writes no CR for two golden files, where CHANGELOG.md, without the attribute, gains 175.
2. **`GIT_OPTIONAL_LOCKS=0`** on every git command a stamp runs.
3. **Robustness.** A git before 2.31 prints `--path-format=absolute` back ahead of a relative path, and the stamp then asks again without the option and joins the path onto the crate's directory. `same_build` counts a commit with `dirty: null` as clean only when the build records no source hash either, as a 0.2 session does. Any other build with an unknown flag goes to the source hashes. The commit is the full hash cut to eight characters. A file that disappears before it is read, as an editor's atomic save replaces one, is skipped. Dangling and directory symlinks stay out of the dirty flag as out of the hash, and AGENTS.md says so.
4. **A crate that holds models** builds its own `ModelSet::new(build_info!())` and calls `stamp_commit`. Otherwise its entries record the host's build, whose stamp sees only the host's `src`, and an uncommitted edit to the model's kernels goes unnoticed. The design's [4.9.2] and its [8] row for `guide/library.md` say so. dev-docs/ is gitignored, so that edit stays out of the commit.
5. **Tests.** `the_engine_stamp_reads_cargo_vcs_info` runs tar from the scratch directory with relative names, since Windows' tar reads the colon of `C:` as a remote host. The stamp tests create their repositories with `--ref-format=files`, falling back to a plain `git init` on a git before 2.45. `a_search_resume_under_another_engine_build_warns` resumes an aborted random search under `other_engine()` and expects one `BuildChanged` with role `Engine`.
6. **A merge's warning** sets the new `between_shards` field of `BuildChanged` and reads "the shards ran different engine builds: X in the lowest shard, and Y in another". `explore_warning` lines carry the field.
7. **Style.** `HOST_BUILD` has its own doc line, above `SWEEP_REPAINT_INTERVAL`'s. henad-build's crate doc and two AGENTS.md paragraphs are rewrapped. The trailing "which" clauses of `StampScope`, `__VERSION`, `HOST_BUILD`, `stamp_commit`, git.rs's watch list, henad-build's README and its test module doc became two sentences or a phrase. git.rs's module doc names its subject, and `commit`, `commit_date`, `run` and `text` open with a verb. `authoring/shaders.md` includes henad-app's build script, which calls `stamp_commit`, through a new `build_script` region, and says that a crate without shaders keeps `build.rs` for the stamp.

New tests from the review: `a_dangling_or_directory_symlink_changes_neither_hash_nor_flag` (Unix), `a_git_before_2_31_is_told_apart_by_its_echo`, `a_search_resume_under_another_engine_build_warns`, and the unknown-flag cases in `two_unidentified_builds_are_never_the_same`.

### Docs

- `guide/sweeps.md` names the builds each session records, says a resume warns and goes ahead, links the comparison rule, says a merge warns, and asks for `--locked` on every shard and resume, with `--locked` in the resume, shard, merge and Slurm commands. The GPU boids note mentions the Results tab's note.
- `reference/cli.md` gains a Builds section with the fields and the rule, updates the `engine`, `model` and `sessions` rows, the resume and merge warnings, and adds `explore_warning` to the JSON lines.
- henad-build's README and description mention the stamps, and `authoring/shaders.md` shows `stamp_commit` in a build script.
- AGENTS.md: the diagram and the dependency rule, `provenance.rs`, the henad-build bullet with `stamp/` and its tests, henad-explore's provenance, the app's run details, About and Results warnings, the CLI's `build.rs` line M7 lists, and the build scripts that call the stamps.

### Edited tree

```text
.
├── .gitattributes                            + -text on the byte-compared reference files
├── AGENTS.md                                 ~ stamps, recorded builds, BuildChanged, the CLI's build.rs
├── CHANGELOG.md                              ~ M7's Added and Changed
├── zensical.toml                             ~ nav entry #36
├── scripts/check_packaging.sh                ~ refuses a "../ path in henad-build's stamp/
├── crates/
│   ├── henad-core/src/lib.rs                 ~ __VERSION
│   ├── henad-build/
│   │   ├── Cargo.toml, README.md             ~ the stamps
│   │   └── src/
│   │       ├── lib.rs                        ~ stamp_commit, stamp_engine_commit, stamp_source_hash
│   │       ├── stamp/mod.rs                  + StampScope, Stamp, the cargo lines
│   │       ├── stamp/files.rs                + hashed files, exclusions, .cargo_vcs_info.json
│   │       ├── stamp/git.rs                  + commit, dirty flag, listing, watched paths
│   │       └── tests/{mod,support,stamp}.rs  ~ + the stamp tests
│   ├── henad-compute/
│   │   ├── build.rs                          ~ stamp_source_hash
│   │   └── src/lib.rs                        ~ __COMPUTE_BUILD
│   ├── henad-models/build.rs                 ~ stamp_source_hash, region narrowed to ShaderBuild
│   ├── henad-explore/
│   │   ├── Cargo.toml                        ~ build dependency, exclude
│   │   ├── build.rs                          + stamp_engine_commit
│   │   ├── tests/fixtures/manifest-0.2.0/    + the 0.2.0 folder and README.md
│   │   └── src/
│   │       ├── lib.rs                        ~ ENGINE_BUILD
│   │       ├── output/manifest.rs            ~ RecordedBuild, BuildRole, sessions' builds, replays_exactly
│   │       ├── sweep.rs                      ~ Provenance::new, BuildChanged, build_warnings
│   │       ├── search_run.rs, pumped.rs      ~ announce through the inputs
│   │       ├── merge.rs                      ~ build warnings, sessions' engines
│   │       ├── result_set.rs                 ~ recorded_builds
│   │       └── tests/{mod,support,resume,search,provenance}.rs   ~ + the provenance tests
│   ├── henad-cli/
│   │   ├── Cargo.toml, build.rs              ~ stamp_commit
│   │   ├── src/explore.rs                    ~ Provenance::new, explore_warning
│   │   └── tests/manifest.rs                 + the host's build and a quiet resume
│   └── henad-app/
│       ├── build.rs                          ~ stamp_commit, the build_script region
│       └── src/
│           ├── lib.rs                        ~ HOST_BUILD
│           ├── ui/about.rs                   ~ the host's stamp
│           ├── ui/export/metadata.rs         ~ host and model_source
│           ├── ui/sweep/session.rs           ~ Provenance::new
│           └── ui/results/{mod,store,table}.rs   ~ changed_builds, replays_exactly, the warning and note
└── docs/
    ├── authoring/shaders.md                  ~ stamp_commit in the build script
    ├── guide/sweeps.md                       ~ the builds, --locked
    ├── reference/cli.md                      ~ Builds, explore_warning
    └── developing/agent-record/20261002-36-library-provenance.md   +
```

## State after

M7 is implemented and uncommitted on `48-library`, on top of ba46f64.
The workspace version stays 0.2.0.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes after the review: 1038 tests, none failed, with the wasm32 typecheck, the packaging, cargo-deny and docs steps and the web build. M7 passed at 1035 before the review, and M6 at 1006.
- After the review a second workspace build again reported all seven henad crates Fresh, with no build script run. The downstream, step 15 and app checks ran before the review, which changed the stamps' git queries and no path those checks read, and were not rerun.
- `uv run --locked zensical build` passes with its path checks.
- `schema_hashes_are_unchanged_since_0_2_0` and the golden CLI tests pass unchanged, and no hash function changed.
- Nothing new runs per tick or per run. `RecordedBuild::engine()` reads consts when a manifest is written and when a resume, a merge or the Results tab compares builds.

Proposed commit message: `feat: build provenance`

## Issues found & future directions

- **Deviations from [4.9].** `BuildChanged` boxes its two builds and carries `between_shards`. A commit with an unknown dirty flag counts as clean only without a source hash. henad-core alone gained a version const. henad-build's version travels as `HENAD_BUILD_STAMP_VERSION`, a name the design leaves open. A resume or a merge writes the 0.2 fallback build into each 0.2 session, which the design implies but does not state.
- **Every commit recompiles henad-explore and the crates above it** in Henad's checkout, where it recompiled the two hosts alone, as [9] accepts. A Finder `.DS_Store` written under a watched `src` reruns that crate's script and recompiles it and its dependents. The stamp's output is unchanged, since the file is excluded, but Cargo cannot know that.
- **henad-core has no crate hash.** An edited `[patch]` copy of henad-core reads as clean, as [4.9.2] states, and `guide/library.md` carries the note when M11 writes it.
- **Step 15 ran on a stand-in host.** `henad_cli::run` (M8) and the template (M10d) do not exist yet. The `downstream` job runs the real step from M10d.
- **`the_engine_stamp_reads_cargo_vcs_info` runs Cargo inside a test.** It packages without building, takes about 3 s, and needs the registry index cached, since it runs with `--offline`. It skips outside Henad's checkout.
- **Docs left for later milestones.** `guide/library.md`'s provenance topic and `guide/your-project.md`'s lockfile and `stamp_commit` advice wait for the pages M11 and M10d write.
- **No git before 2.31 ran here.** The fallback rests on git's documented habit of printing an unknown `rev-parse` option back, and `a_git_before_2_31_is_told_apart_by_its_echo` checks the detection alone.
- **Next.** M8 (the CLI as a library) and M9 (the app as a library) depend on M7 and can start.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
