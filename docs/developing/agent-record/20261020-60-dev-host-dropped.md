---
date: 2026-10-20
title: "Dev host dropped, #49 carried onto the node editor branch"
description: "The maintainer redirected Henad to a code-free node editor whose models compile to WGSL alone, and dropped the dylib dev host. A new branch, 37-node-editor, takes #49 from master without the dev host, and a cleanup commit removes the plugin code origin and the dev host's traces from code, docs and the agent files."
icon: material/source-branch
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37, #49"
state: "37-node-editor at master plus #49 without the dev host, HENAD_REQUIRE_GPU=1 ./check.sh passing but for three new cargo-deny advisories that master fails as well, the cleanup and this record committed as placeholder commits"
baseline_commit: 4f1ea05
delta_state: "committed on `37-node-editor`: sixteen cherry-picked placeholder commits of #49 and the AGENTS.md split, one reduced review commit, and a cleanup commit with this record"
---

# Dev host dropped, #49 carried onto the node editor branch

> After reading a walkthrough of #49, the maintainer restated the goal: a node editor in which people who write no code build large agent-based models that run fast, with previews that never need a restart.
> Hot reload of hand-written Rust, which #49's dev host provided, is left to people who write Rust.
> The agreed direction compiles node models to WGSL alone, for grid and agent models first and networks right after, and keeps #49's runtime grid rules as expert use.
> A new branch, `37-node-editor`, starts from `master` and takes every #49 milestone but the three of the dev host (D1, D2 and M8), each as its original placeholder commit.
> A cleanup commit removes the plugin code origin, which only the dev host produced, and every remaining mention of the dev host in code, docs, `AGENTS.md` and `agents/`.

## State before

`49-dynamic-loading` held #49 complete: eleven milestones, seven review commits and record #58, all as placeholder commits on `4f1ea05`, with the split of `AGENTS.md` into `agents/` uncommitted (record #59).
The maintainer approved scope from an interactive walkthrough and a written proposal in the same session.
Every decision is listed below.

1. Node models require a GPU with compute.
2. A node model replays exactly on the same device.
3. Network models follow right after this effort.
4. The dev host is dropped entirely. Hand-written WGSL grid rules stay as expert use. The split is committed first.
5. v1 covers what the example grid and agent models use.
6. The editor tries egui-snarl first.
7. The example models also ship as node models.
8. Model files stay TOML.

## What was done

### The split, committed on the old branch

The `AGENTS.md` split was committed on `49-dynamic-loading` as `8623977`, since its text describes that tree, dev host included.

### The new branch

`37-node-editor` branches from `master` at `4f1ea05`.
These commits were cherry-picked in their original order, keeping their placeholder messages for the maintainer to reword:

- M1, M3, M2, M4, M5, M7, R and M6;
- the core, compute, explore-cli, app and docs review commits;
- the commit of record #58;
- the split.

Conflicts came only from the dev host:
- CHANGELOG lines, `zensical.toml` nav entries and feature lists in `ci.yml`, `check.sh`, the facade's manifest and README, `releasing.md` and `api.md` took the picked side without the dev host.
- Files the dev host owned were dropped.
- `docs/guide/hot-reload.md` was dropped.
- The app review commit lost its project-mode and plugin hunks, its two project-file tests and its two plugin tests.
- The dev-host review commit was reduced to its one change outside the dev host, the spelling `PHASE_CANCELED` in `state/build.rs`.

The records of D1, D2 and M8 (numbered 51, 54 and 60 there) describe dropped work, and stay on `49-dynamic-loading` alone.
The records on this branch are numbered without gaps, so a record's number here can differ from its number on `49-dynamic-loading`.

### The cleanup commit

- **The plugin code origin.**
  - `CodeOrigin` holds `Compiled` and `Runtime`.
  - `ModelSource::__into_plugin` and `ModelEntry::__into_plugin` are gone, with every arm, doc sentence and test that existed for plugin code.
  - Code identity is a runtime model's code hash, or a compiled crate's source hash.
  - When either build records an origin, two builds are the same only as runtime code of one known hash. A resume, a search resume and a merge refuse every other pair, and compiled against compiled still warns `BuildChanged`.
  - A manifest that records the origin `plugin`, written only on the unreleased branch, reads as an unknown origin through the existing path.
- **The same-code check.**
  - `same_code` in henad-app compares the `ModelSource` and the `schema_hash`.
  - `selection_is_loaded`, `code_pending` and `settle_loaded` now compare through it. Before, they compared the source alone, so two entries of one source with other parameters read as one code, and `equal_sources_with_other_declarations_are_other_code` failed.
- **Tests.**
  - Deleted: `a_marked_plugin_source_reads_as_plugin`, `a_resume_refuses_a_plugin_whose_sources_changed` and `a_plugin_with_unchanged_sources_resumes_a_compiled_folder`.
  - Lost their plugin halves: `a_merge_refuses_shards_of_two_codes`, `two_codes_under_one_label_are_listed_apart` and `schema_json_names_code_only_for_runtime_models`.
  - `runtime_and_plugin_models_name_their_code` became `a_runtime_model_names_its_code`.
- **Docs.** The guide, reference and authoring pages name no dev host or plugin, and the dead links to `hot-reload.md` are gone.
- **AGENTS.md and `agents/`.**
  - `agents/dev-host.md` is deleted.
  - `AGENTS.md` describes eight crates with master's graph, no unsafe exception and no dev host commands or environment variables.
  - Each topic file is checked against this branch. `agents/template.md` and the CI and release sections return to master's text, since the template and those jobs are unchanged since 0.3.0.
  - The facade's and the template's `inspection` features came with the dev host and are gone with it. henad-app's own `inspection` feature stays.
- **CHANGELOG.** The `[Unreleased]` bullets of the kept milestones name no dev host or plugin.

### Codebase structure

```
37-node-editor (new branch, from master 4f1ea05)
├── #49 M1, M3, M2, M4, M5, M7, R, M6       # cherry-picked placeholder commits
├── #49 review commits, record #58           # cherry-picked, dev host hunks left out
├── AGENTS.md split                          # cherry-picked
└── cleanup commit
    ├── crates/henad-core/src/provenance.rs  # CodeOrigin without Plugin
    ├── crates/henad-compute/src/entry/      # no __into_plugin
    ├── crates/henad-explore/src/            # code identity of runtime code alone, docs, tests
    ├── crates/henad-app/src/state*.rs       # same_code compares source and schema hash
    ├── crates/henad-app/src/ui/             # About and Results wording, test renamed
    ├── docs/{guide,reference,authoring}/    # no dev host, no plugin code
    ├── docs/developing/releasing.md         # hidden API list without __into_plugin
    ├── AGENTS.md, agents/                   # dev host removed, dev-host.md deleted
    ├── CHANGELOG.md                         # [Unreleased] without the dev host
    └── docs/developing/agent-record/
        └── 20261020-60-dev-host-dropped.md  # this record, and its nav entry
```

## State after

- `37-node-editor` holds master plus #49 without the dev host. Every commit since `master` is a placeholder for the maintainer to reword and sign.
- `HENAD_REQUIRE_GPU=1 ./check.sh` passes, apart from `cargo deny`.
  Three unmaintained-crate advisories published since 0.3.0 fail it, RUSTSEC-2026-0370, 0371 and 0372 for `zvariant`, `zbus_names` and `zvariant_derive`, and `master` fails them as well.
  Every step after it passed when run without it: 1334 tests, the wasm32 checks, the web build and the docs.
- `49-dynamic-loading` keeps all of #49 and the split commit, unchanged.
- The plan for the node editor is in the maintainer's planning folder. It narrows the earlier full design of #37 to GPU targets of grid and agent models, and is not part of the repository.

## Issues found & future directions

- **CI.** The branch has not been pushed, and no CI ran on it.
- **`cargo deny` advisories.** The zbus crates come in through eframe's Linux accessibility layer (`accesskit_unix`). Whether to raise them, update the stack or add `ignore` entries in `deny.toml` is the maintainer's call, and it affects `master` too.
- **The full #37 design assumed `49-dynamic-loading` as its base.** It includes the Editor's project mode and CPU targets. The new plan records what changes against it. Its sixteen open questions still need answers before the first round.
- **`merge.rs` compares every pair of shards.** Without plugin code, comparing each shard with the lowest one would be enough. The loop is correct and cheap, and was left as it is.
- **Leftovers on disk.** `examples/hot-host/target` (7.3 GB of ignored build output from the old branch) remains, and switching back to `49-dynamic-loading` reuses it. `crates/henad-dylib/Cargo.lock`, an untracked copy that broke the workspace glob, was moved out of the checkout.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
