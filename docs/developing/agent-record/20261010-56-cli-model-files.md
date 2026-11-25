---
date: 2026-10-10
title: "Model files on the command line"
description: "Milestone M7 of #49. henad-cli takes a repeatable --model-file, which reads a model file and puts its grid rule into a clone of the set before the GPU device is sized, so a rule benchmarks, exports, lists and sweeps like a compiled model. A resume or a merge of an edited file is refused, and a problem in the file stops the command at its line. A new reference page documents the model file format and the template a step is written against."
icon: material/file-code
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#49"
state: "--model-file, its seven integration tests, the model files reference page and the CLI and sweeps docs in place, check.sh and check_hot.sh passing with HENAD_REQUIRE_GPU=1, uncommitted on top of bf72686"
baseline_commit: bf72686
delta_state: "uncommitted on `49/m7-cli-model-files`, 3 new files and 8 changed files"
---

# Model files on the command line

> Milestone M7 of #49 (dynamic model loading), decisions 4, 7 and 10 of its plan.
> A grid rule saved as a model file now runs from the command line with no build.
> `--model-file PATH` is repeatable, and each file's model is `ModelSet::replace`d into a clone of the options' set, so a file can take the place of a model with its id, with a note on stderr in text mode.
> The files and `--spec` are read before the GPU device is acquired, and the device is sized to the files' models as well.
> Every mode takes the flag but `--merge`: the benchmark, both exports, `--list`, `--params` (whose JSON line then carries the rule's `origin` and `code_hash`), and sweeps and searches from flags or a spec, `--dry-run` and `--resume` included.
> A file that does not load stops the command with exit status 1, each problem printed as `path:line:column: error: message`.
> M2's refusals do the rest: a resume with an edited file and a merge of shards that ran two versions of a file are rejected before anything is written.
> `docs/reference/model-files.md` documents the format key by key, the template the step sees, what is refused and why, and the limits, and includes `life_rule.toml` whole.

## State before

`49/m7-cli-model-files` branched from `49-dynamic-loading` at `bf72686`, which holds rounds 1 to 3 as placeholder commits: D1, M1 (#50), M3 (#52), D2, M2 (#51), M4 (#53) and M5 (#54).
M4 left `henad_explore::model_file::ModelFile` (`parse`, `entry`, `code`, `to_toml`) and `specs/models/life_rule.toml`, and turned on `model-files` in henad-cli without a caller.
M2 left the code identity in manifests and the three refusals of changed runtime code, `ResumeError::CodeChanged`, the search resume's check and `MergeError::CodeChanged`, and `schema_json` writing `origin` and `code_hash` for a model that is not compiled.

henad-cli ran the models of its `CliOptions` set and nothing else.
`run_args` acquired the device from the set's `gpu_needs()` before it read `--spec`, at research-code-map.md's sweeps-cli-provenance §4.1.
No page described the model file format.

## What was done

### henad-cli

`Args` gains `model_file: Vec<PathBuf>` as `--model-file <PATH>`, a top-level flag outside `ExploreArgs`.
Every mode before sweeps keeps its meaning, since the flag selects no mode, and `existing_invocations_keep_their_mode` passes unchanged.
The positional `MODEL` is no longer required when `--model-file` is given, and `--merge` conflicts with the flag, as it does with `MODEL`.

`run_args` now runs in this order:

1. size the worker pool, and return from `--merge`, both as before;
2. `with_model_files` reads the files into a clone of the set;
3. `--spec` is read;
4. the device is acquired from the `gpu_needs()` of that set;
5. the model id is resolved: the positional id, else the spec's model, else the id of the one model file.

`Mode::of` reads one model file as a model, so `--info` with one file and no id prints its header and then runs the file's model, as `--info` beside an id does.
Two files or more with neither an id nor a spec are a usage error from `parse_args`, printed by clap with exit status 2, as `docs/reference/cli.md` gives for a missing flag.

`with_model_files` borrows the set unchanged when no file is given, and clones it otherwise (`Cow<ModelSet>`).
For each file, `load_model_file` reads the text, calls `ModelFile::parse` and `entry`, and the entry goes through `ModelSet::replace`, which keeps an example model's position when the file replaces it.
A replace that returns an old entry prints `note: model file 'PATH' replaces the model 'ID' of this build` on stderr in text mode.
Under `--json` the note is left out, and no JSON line kind is added.
Two files that declare one id are refused, naming both files.

A file that does not load becomes `cannot load model file 'PATH'`, with a cause that lists each `CodeDiagnostic` of the `ModelFileError` with the path as its file, one per line with its notes.
`run` prints it as any error, `Error:` and the causes, and returns 1:

```text
Error: cannot load model file '/tmp/m7bad.toml'

Caused by:
    /tmp/m7bad.toml:39:5: error: loops are not allowed in a grid rule. Without them a step's work is bounded
```

A file that cannot be read is `cannot read model file 'PATH'` with the operating system's error.
Both happen before the device is acquired, and a broken file costs no device.

Nothing downstream needed a change.
The rule's entry carries `ModelSource::runtime` with its `ModelCode`, so `explore::run` writes `model.code` and the session's `origin` and `code_hash` into the manifest, `--params --json` adds the two fields through `schema_json`, and a resume or a merge compares the code by M2's rule.
The crate doc names the flag, with one documented sweep line over `life_rule.toml`.

### Tests

`tests/model_file.rs` is a fourth `[[test]]` with `required-features = ["example-models"]`, and runs the binary on copies of `crates/henad-explore/specs/models/life_rule.toml`.
The edited copy adds a birth on six neighbours to the step, `alive == 3u || alive == 6u ||`.
Every test but the last needs a device to build a rule, takes it through `headless_test_device(&TestDeviceRequest::baseline())`, skips without one, and fails without one under `HENAD_REQUIRE_GPU`.

| Test | What it holds |
|---|---|
| `a_model_file_sweeps_and_records_its_code` | A sweep of two densities and two replicates on a 16 by 16 grid writes four runs; `model.code` holds format `henad-grid-rule/1`, the canonical text and its hash; the session's `model_source` reads `runtime`, that hash and `runtime:life_rule`; `argv` holds `--model-file` |
| `a_resume_with_an_edited_model_file_is_refused` | A resume with the edited file exits 1 naming both hashes, runs nothing and leaves the manifest byte for byte; a resume with the original file then adds the second replicates |
| `a_merge_of_shards_with_different_model_files_is_refused` | Two shards of one file merge; a shard of the edited file is rejected, naming its hash, and no output folder is made |
| `a_model_file_can_shadow_an_example_model` | A file with the id `gpu_game_of_life` prints the note under `--params`, and none under `--params --json`, which shows the rule's name, `runtime` and its hash, where the compiled model shows no origin; `--list` names the id once; a sweep of the id records runtime code |
| `model_file_is_allowed_with_spec_and_resume` | With a spec file of the rule: a dry run plans 2 by 2 runs and writes nothing, a sweep writes four, a resume skips four; a resume without the file is rejected because the build has no `life_rule` |
| `list_and_params_json_show_a_model_file` | `--list` adds `life_rule` beside the example models; `--params --json` with and without the id shows the model, `gpu`, `runtime`, the code hash, the three parameters and the columns `Dead` and `Alive`; `--params` prints the text form, with `--info` too |
| `a_model_file_error_points_at_its_line` | A loop at line 39 column 5 and an unknown key at its line print at `PATH:LINE:COLUMN`; a missing file and two files of one id exit 1; two files of two ids without `MODEL`, and `--merge` with `--model-file`, exit 2. No device is needed |

`tests/golden.rs`, `tests/json_lines.rs`, `tests/manifest.rs` and the unit tests pass unchanged.

### Docs

- `docs/reference/model-files.md` (new, in the Reference navigation after the command line): the header keys, `[grid]`, `[[state]]` with the constant names and the seeding, `[[param]]` in the `--params --json` vocabulary, `[step]`, code identity, the `step_cell` signature, what the template declares (`params` and its fields, `neighbor`, `moore_count`, `von_neumann_count`, `NUM_STATES`, the state constants), randomness through `henad::rng`, imports, what the engine owns, a table of what is refused with one reason each, and the limits.
  It includes `crates/henad-explore/specs/models/life_rule.toml` whole and opens with the "Henad 0.4" admonition, as `guide/hot-reload.md` does.
- `docs/reference/cli.md`: the 0.4 line in the page's admonition, `MODEL` optional with one file and required with two, `--model-file` in the flags table, a Model files section (the modes that take it, replacement, reading order, the error form, code identity and the refusals, which comments keep the code), one line beside `--spec`'s rejected flags, one line each under Resuming and Shards and merging, and a model file that does not load under exit status 1.
- `docs/guide/sweeps.md`: a Sweeping a model file section before Reading the results, and a pointer to it from Resuming a sweep.
- `AGENTS.md`: the henad-cli paragraph (four integration tests, the device sized with the files' models, a `--model-file` paragraph with its keep-list), and a line under henad-explore's `model_file.rs` naming the page and the tests that read `life_rule.toml`.
- `CHANGELOG.md`: one Added line and one Changed line under Unreleased.

### Review

Two reviews of the first version found eleven points, all taken.
`--info` with one file and no id printed its header and exited, and two files without an id exited 1, both now as above.
The replacement note no longer prints under `--json`, as the brief asked.
On `model-files.md`, a `[[param]]` table takes the `--params --json` vocabulary without `index`, `apply` and `default_index`, a problem with no place in the file is said to name the file alone, `--set` no longer appears among the things that name a model, and the unpinned cost of 1196 became "under 2048", the bound `runtime_grid.rs` asserts.
`cli.md` names model files in its 0.3 admonition and beside `--spec`'s rejected flags, and three sentences lost a trailing "which".

### Edited tree

```text
.
├── AGENTS.md, CHANGELOG.md, zensical.toml           ~ --model-file, nav for the page and #56
├── crates/henad-cli/
│   ├── Cargo.toml                                   ~ [[test]] model_file
│   ├── src/lib.rs                                   ~ --model-file, with_model_files, load_model_file,
│   │                                                  files and spec read before the device, crate doc
│   ├── src/explore.rs                               ~ --merge conflicts with --model-file
│   └── tests/model_file.rs                          + seven tests
└── docs/
    ├── reference/model-files.md                     + the model file format and the step's template
    ├── reference/cli.md                             ~ Model files section, flag, resume and merge lines
    ├── guide/sweeps.md                              ~ Sweeping a model file
    └── developing/agent-record/
        └── 20261010-56-cli-model-files.md           +
```

## State after

Everything is uncommitted on `49/m7-cli-model-files`, on top of `bf72686`, in the worktree `.claude/worktrees/49-m7`.
Nothing was committed, pushed or tagged.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes after the review fixes at the first attempt: the wasm32 typechecks, fmt, packaging, cargo-deny, clippy, 1320 Rust tests and doc tests over 42 test binaries with none failed and 6 doc tests ignored as before, the README program, the docs with `-D warnings`, the scripts' tests and the web build.
  The one warning is the future-incompatibility note on naga, wgpu, wgpu-core and wgpu-hal, as on `master`.
  `a_location_survives_a_panic_on_a_rayon_worker`, which flakes at the base commit, passed.
  One doc comment on `parse_args` changed after it, and fmt, strict clippy, `cargo doc` with `-D warnings` and henad-cli's tests passed again on it.
- `HENAD_REQUIRE_GPU=1 ./scripts/check_hot.sh` passes on the final tree, 12 checks `ok`: `hot-check` with every check `ok`, then `hot-check relink`.
- `HENAD_REQUIRE_GPU=1 cargo test -p henad-cli` passes: 32 unit tests, `golden` (2), `json_lines` (3), `manifest` (1), `model_file` (7) and the doc test.
- `uv run zensical build` reports no issues, with the page and this record linked from the navigation.
  The `site/` and `.venv/` it left in the worktree, both gitignored, are deleted.
- The scratch directory `/Users/micfong/.claude/jobs/3144150c/tmp/m7/` held the check logs alone, and is deleted after each round.
  The hand checks wrote a few files under `/tmp` (`m7*`), deleted as well.
  No template copy was built: the template's CLI is `henad::cli::run`, and the flag reaches it unchanged.
- The app was not run. Nothing in henad-app changed.

Proposed commit: `feat: --model-file in henad-cli and the model files reference`.

M7 has no benchmark gate, and no timing ran.
No per-tick path changed: the set is cloned once per command line, and only when a file is given.

## Issues found & future directions

- **A resume without the file says only that the build lacks the model.**
  The plan cut rebuilding a model from the manifest, and a resume of a `life_rule` folder without `--model-file` reads "this build does not include model 'life_rule' (try --list)".
  The folder holds the code inline as `model.code`, and the message could name `--model-file` when the manifest records runtime code.
  That needs the folder read before the model lookup, the reorder M2's record also names for the spec.
- **A shadowing file meets a compiled folder with "different model code".**
  A resume of a `gpu_game_of_life` folder with a file of that id is rejected, as M2's rule wants.
  The message names runtime code against compiled sources, and does not say that the cause is the file.
- **`--list` does not mark a file's model.**
  The golden output covers only the example set, so a marker such as the file's path after the name would break nothing.
  The note on stderr names a replaced model, and a model added under a new id has no note.

### Deviations from the plan and the brief

- **`MODEL` is optional with one `--model-file`.**
  The brief does not ask for it, and research-code-map.md §4.4 proposed it.
  With one file and no spec, the file's id is the model, so `henad-cli --model-file life_rule.toml --params` works.
  With two files and no id the command is refused.
  `existing_invocations_keep_their_mode` reads a documented line with no id as `--info`, so the documented non-sweep line names its id.
- **Two files of one id are refused.**
  The brief says nothing about it, and research-code-map.md's decision 5 recommended a refusal for a duplicate id.
  A later file replacing an earlier one would make the command line's order matter in a way nobody sees.
- **Two files without `MODEL` are a usage error.**
  The command line exits 2 from `parse_args`, as a missing flag does, where an error after parsing would exit 1.
- **`docs/reference/api.md` is unchanged.**
  The flag adds no API, and the page already lists `henad::rules`.
- **`a_model_file_error_points_at_its_line` checks more than one line.**
  It also checks an unknown key, a missing file, two files of one id and the conflict with `--merge`.

### Open questions for the maintainer

- Should a resume or a merge whose folder records runtime code, and whose model id the build lacks, name `--model-file` in its error?
- Should `--list` mark a model read from a file?

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
