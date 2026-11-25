---
date: 2026-11-11
title: "Open questions, restyle and cleanup"
description: "Round 6 of the #37 redesign. The open questions left after round 5 were settled at the session's discretion, about 30 of them by a fix. The Editor gained a second dock layout and a restyle of its card, Sheet pane, header, notices and dialogs, and cleanup packages refactored eight of the nine crates. 407 one-line unsigned commits came in 28 fast-forward merges and 5 commits of the session's own. An isolated full check passed 16 of 16 steps, and a snapshot of every model's output stayed identical through the cleanups outside the runs that do not replay."
icon: material/broom
status: ai-generated
model: "claude-fable-5-2 (Claude Code): design, analysis, decisions and review. Implementation, the cleanup findings and this record's draft by Opus subagents, one rewording job by a Sonnet subagent"
issue: "#37"
state: "37-node-editor at 7e845bb and this record: 407 unsigned one-line commits after bd62a9c, 660 after bad936b, nothing pushed. The final full check at 7e845bb passed 16 of 16 steps with 2,276 tests. The commit messages are rewritten after this record, and one commit then remaps the hashes the records cite"
baseline_commit: bd62a9c
delta_state: "committed on `37-node-editor`: 402 commits by fast-forward from 28 branches, 5 commits made on the branch itself, and this record from `docs/record-107`, all unsigned"
---

# Open questions, restyle and cleanup

> Round 6 of the #37 redesign ran on 2026-11-10 and 2026-11-11 from `bd62a9c`.
> The maintainer asked the session to settle the remaining open questions at its own discretion, to restyle the Editor, to clean up the codebase, and to keep commit messages to six words.
> Of 133 open questions, about 30 were fixed in code or docs, and each of the rest was left with a reason.
> The Editor gained an Editor layout, and its card, Sheet pane, header, notices, dialogs and egui's overlays were restyled as framed regions with hairlines inside.
> Cleanup packages over eight of the nine crates merged 341 commits, and a snapshot of every model's output stayed identical through them, outside the five runs that do not replay bit for bit.
> For some hours agents built into one shared cargo target directory, where cargo could reuse another worktree's compiled crates.
> An isolated full check with its own build directory then passed 16 of 16 steps with 2,257 tests.
> Nothing was pushed, and every commit is unsigned.
> A pixel comparison of 62 scenes of the Editor found no difference between the restyle and the tip after the app's cleanups.
> The final full check at `7e845bb` passed 16 of 16 steps with 2,276 tests, and the snapshot there is identical to its baseline outside the runs that do not replay.
> The commit messages are rewritten after this record is committed.

## State before

`37-node-editor` at `bd62a9c` held 253 unsigned `[draft]` commits after `bad936b`, the remote's head of the branch, and ended with record #106 and one docs commit after it.
At the end of round 5 the full check passed every step, with 2,219 tests passing and the two float probes ignored, and the device tests passed on lavapipe in a local container.
Record #106 left the push and the release work with the maintainer, the speed gaps and budget refusals of node models, three checks by eye, and the smaller calls of records #61 to #105 that had no options.
Its own list named one wording fault: the "otherwise" tooltip of a line below two competing lines inside one Repeat.

## What was done

### The requests

The maintainer's requests of this round, in their words:

1. "Fill all the rest of the open questions with your own discretion."
2. On commit messages: "aim for 6 word max, 4 word avg. Rewrite all the commit messages and drop the [draft]s."
3. A switchable Editor layout that rearranges the workspace, a redesign of the card, in their words "poorly designed with almost zero sense of 'blocks' or 'line type' that were shown in artifacts", and a shorter artifact: "Trim or separate things and reorganise it totally."
4. "you own the codebase now. Clean up duplicate/unreadable/obscure/redundant/nonidiomatic code and refactor if necessary."
5. "summarize breaking changes/things that might have deviated from my expectations in the artifact."
6. On readability: "no separators on sheet, no highlight and poorly separated custom selection menus, no table borders nor shades, poor alignments, unclear focus, unclear attention, and a lot more", and "A lot of the custom components are poorly designed as well."
7. On style: "minimalistic functional and practical FUI (futuristic UI), expert oriented, and no need to heavily modify egui styles", with the palette's consistency ahead of WCAG ratios.
   After three mockups: "Some sort of a middle ground would be good, since cards are intuitively separated but FUI is informative and functional".

Mockup C was built: framed regions, with hairlines, small mono labels and dense rows inside them.
Opus subagents wrote the code, and the session's main model did design, analysis and review, as the maintainer asked.
The artifact of requests 3 and 5 sits outside the repository, and this record does not repeat it.

### How the round ran

Each package ran in its own git worktree on its own branch from `37-node-editor`, from a brief the session wrote, and the session fast-forwarded the branch after reading the package's report.
28 branches merged that way.
The session made 5 commits on `37-node-editor` itself: `bca71d3`, `d4a96cb`, `d4a96cb`, `d4a96cb` and `6662055`.
Every commit of the round is one line and unsigned, by the maintainer's instruction, and none carries `[draft]`.
New messages were held to six words counting the type.
406 of the 407 keep to it, and `6662055` "fix: run to tick drives sheet preview" has seven.
The 407 messages average 5.34 words, against the 4 the maintainer asked for.

Read-only finders wrote 238 cleanup findings over 12 areas on 2026-11-10, and stopped early at the maintainer's request to save usage.
Six more areas followed on 2026-11-11 with 129 findings: henad-cli and the example models, the test code outside henad-graph and henad-app, and four areas of the app.
Each cleanup package was briefed to read its findings, apply or skip each one with a reason, and keep every test failing for the reason it failed before.

A change meant to keep every model's results was checked with a snapshot.
`henad-cli` runs 39 models and model files at a fixed seed for 64 steps, two sweeps and six error runs, and writes every parameter schema, lowered sheet, compiled shader, statistics file, sweep output and error text to a folder.
The folder is compared with a baseline, file by file.
Five runs do not replay bit for bit on this machine: stock `gpu_boids`, the boids graph fixture and starter, and the boids and bounded confidence sheets, from GPU float sums over neighbours.
Their statistics are compared loosely, and every other file exactly.

### The open questions

The session's working notes listed 133 open questions, drawn from the records, the notes of earlier rounds and the earlier artifact.
Each got a call by one rule: fix what loses a researcher's work or tells them something false, fix wording that is wrong, leave what guards against a mistake, leave what needs a design nobody has reviewed, and write up what is large.
About 30 were fixed by the packages below.
Each of the rest was left with a reason: published history stays as written, the error is the guard, latent until a feature exists, kept on purpose, adopted rows stand, needs an idle machine or other hardware, or large and written up.
The calls are in the session's working notes, and this record adds no decision rows.

### The packages, in merge order

| Package | Commits | First and last |
| --- | --- | --- |
| Tests and docs | 4 | `bd62a9c` to `bd62a9c` |
| Editor layout | 2 | `28845d0`, `28845d0` |
| Cleanup wave 1: host evaluator and edit commands | 14 | `865cb39` to `865cb39` |
| Cleanup wave 1: WGSL compiler | 20 | `865cb39` to `865cb39` |
| Cleanup wave 1: GPU engines | 19 | `530e498` to `530e498` |
| Cleanup wave 1: henad-explore | 20 | `bca71d3` to `bca71d3` |
| CHANGELOG of wave 1 | 1 | `bca71d3` |
| Cleanup wave 1: CPU runners and entries, henad-core, henad-build | 26 | `b8ec032` to `b8ec032` |
| Cleanup wave 1: graph document, reader, checker, catalogue | 19 | `68ec09a` to `68ec09a` |
| Card pane | 11 | `9e29fd8` to `9e29fd8` |
| Wording fixes | 12 | `d4a96cb` to `d4a96cb` |
| A fixture rename, `AGENTS.md`, a doc comment | 3 | `d4a96cb`, `d4a96cb`, `d4a96cb` |
| Sheet starter ids | 3 | `6662055` to `6662055` |
| App fixes | 9 | `6662055` to `6662055` |
| CI's web clippy | 1 | `6662055` |
| Ids of new models | 1 | `6662055` |
| Sheet pane | 1 | `825eaf6` |
| Cleanup wave 2: the four sheet areas of henad-graph | 69 | `01eed55` to `0e64f19` |
| Test code outside henad-graph and henad-app | 23 | `bec8310` to `bec8310` |
| Edit refusals | 2 | `091c468`, `091c468` |
| Header, notices, dialogs and theme | 10 | `3102bbb` to `3102bbb` |
| henad-cli and the example models | 15 | `29d0d27` to `29d0d27` |
| No rate inside a Repeat | 2 | `d1edb54`, `d1edb54` |
| henad-graph's test code | 24 | `b03b32d` to `b03b32d` |
| Sweep and Results tabs | 16 | `43d1acb` to `43d1acb` |
| App shell | 24 | `79d3b4c` to `79d3b4c` |
| The guide | 3 | `a3ffa74` to `a3ffa74` |
| The Editor's code | 26 | `59096ee` to `59096ee` |
| CHANGELOG | 1 | `59096ee` |
| henad-app's test code | 26 | `7e845bb` to `7e845bb` |

The last two branches of wave 1 had no merge of their own.
The card package's branch was rebased onto them, and its fast-forward brought them in.

#### Tests and docs

- `bd62a9c` qualifies the replay contract in `docs/reference/model-files.md` and `agents/gpu-traps.md`.
  A sheet or graph on a grid replays exactly with one seed on one device.
  A model of agents does too, unless a look at the agents around adds up floats or depends on the order of its visits.
  The boids sheet and node graph starter and the bounded confidence sheet do not replay exactly.
  A test in `agent_wgsl.rs` holds `replays_exactly` false for those two sheets and true for every other agent sheet starter.
- `bd62a9c` adds `the_copies_of_the_test_sheets_equal_their_source`.
  It holds the copies of `sir_chance.toml` and `sird.toml` in henad-models and henad-app equal to henad-graph's, byte for byte.
- `bd62a9c` adds `a_competing_group_with_a_rate_for_each_steps_alike_on_the_host_and_the_device` in henad-models.
- `bd62a9c` fixes private-item doc links in henad-graph's `catalogue/state.rs` and `wgsl/agent_hooks.rs`.
  `d4a96cb` of the wording package fixes the rest, in `sheet/mod.rs` and `sheet/relations.rs`.

#### Editor layout

The View menu holds "Default layout" and "Editor layout", and a checkbox "Open models in the Editor layout", on by default.
In the Editor layout the Editor takes 62.5 percent of the width, in front of Sweep and Results, and a right column holds the Viewport, then Playback and Pacing, then Charts, Parameters, Statistics and Model.
Each layout keeps its own dock state for the session, and Reset layout resets the layout that shows.
Tests in `ui/dock.rs` and `ui/menu_bar.rs` cover the switch, the checkbox and the reset.
`28845d0` describes the layouts in `docs/guide/app.md` and `docs/guide/editor.md`.
The package's screenshots were taken in the live app on the built-in display.

#### Cleanup wave 1

Six branches, 118 commits: three over henad-graph outside its sheet folder, one over henad-compute's GPU engines, one over its CPU runners and entries with henad-core and henad-build, and one over henad-explore.
Among them, `530e498` and `530e498` move the pass building and the press path the two GPU engines shared into `gpu/built_pass.rs` and `gpu/press.rs`.
`865cb39` moves the host evaluator's program into `host/program.rs`, and `b8ec032` moves the CPU runner's tests out of `cpu/sim_thread.rs` into `cpu/tests/`.
`b8ec032` takes the FNV-1a hasher out of sweep planning into `henad-core/src/fnv.rs`, and `bca71d3` makes henad-explore's `sweep.rs` a folder.
`530e498` makes `GpuCommand` private, and `b8ec032` merges the two public `StatSample` types into one.
The CHANGELOG lists `GpuCommand` under Removed as a breaking change of the unreleased 0.4, and its `run_to_samples` line now names `henad::StatSample`.
A full check after the first four branches, in the main checkout with its own build directory, passed all 14 steps of `./check.sh` with 2,235 tests passing.
The snapshot after those four, and the snapshot at `d4a96cb` after all six, the card and the wording fixes, are identical to the snapshot at `bd62a9c` outside the loose runs.

#### Card pane

Each card is a framed region with a title row and a line count, and the law under it.
Its rows carry a 3 point kind bar: Look around blue, Chance purple, Yes or no yellow, Calculate gray and Change green, at step 500, and When and Repeat gray 100.
Hairlines split the rows, a guide line marks each block level, and lead words take a semibold face, Henad Sans SemiBold, embedded in the app with its licence listed.
A result's name is drawn as its chip, and the alternatives of Choose and Depending on sit on rows of their own.
The add buttons sit in a footer, each with its kind's colour square.
Chips, blanks, selects, menus and pickers share one palette, menus are opaque with hover, focus and current rows, and every control shows one focus outline.
Lines and Diagram became a segmented toggle.
henad-graph's `SentencePart` gained `Keyword` and `ResultName` in `9e29fd8`, and the sentences hold the words `LINES_VIEW`, `MOVE_UP_FIRST` and `MOVE_DOWN_LAST` (`9e29fd8`).
`agents/ui.md` gained the palette table and the style rules, and the sheet wording moved to its own topic file.
Every screenshot of this package was rendered offscreen from the app's own UI code.
The machine's screen was locked.

#### Wording fixes

Each fix came with a test in henad-graph or henad-app.

- `d4a96cb`: a third "otherwise" sentence, for higher lines that compete inside one Repeat, where the first time round that fires decides between them.
  Record #106 had listed the fault.
- `d4a96cb`: four faults of the sheet's sentences.
  A Count names its "only" test, a Set to a chance reads as a sentence, a kind's plural follows the rules of English with a short list of irregular plurals, and a formula names an unnamed Count by its automatic name.
  The sentence comments of `bounded_confidence.toml` and two test sheets changed with it.
- `d4a96cb`: the checker rejects a comparison of an arrow with a number, at its line.
- `d4a96cb`: a shortfall names the properties a packed buffer holds.
- `d4a96cb`: an average measure with its automatic name follows a rename of its property.
- `d4a96cb`: an edit that would keep a number that is not finite is rejected.
- `d4a96cb`: the fault of a look past its cap suggests a larger world or a shorter distance.
- `d4a96cb`: a parameter with no unit put into a rate takes the rate's unit, and one undo restores both.
- `d4a96cb`: the long-tick note reads "almost 1" where the chance rounds to 1.
- `d4a96cb`: Show changes guards on the hash of the sheet's code, so a note edit no longer blocks it.

#### A fixture rename, `AGENTS.md` and a doc comment

`d4a96cb` renames henad-explore's fixture `series-be338ec` to `series-without-time`, the results folder recorded before the `time` column that `result_set.rs` reads as history.
`d4a96cb` corrects `AGENTS.md`: henad-cli runs a graph on a grid or of agents.
`d4a96cb` corrects a module doc of `wgsl/agents.rs` and a sentence of `agents/henad-graph.md`.

#### Sheet starter ids

The four sheet starters whose id equalled an example model's take `sheet_game_of_life`, `sheet_sir`, `sheet_boids` and `sheet_ants`, as the graph starters are `node_<name>`.
Applying one no longer replaces the example model in the session.
The app's model list shows the id after the name where two models share a name.
Tests: `applying_the_life_sheet_keeps_the_example_model_of_that_name`, `no_starter_takes_the_id_of_an_example_model` and `the_model_list_names_the_ids_of_models_that_share_a_name`.
The snapshot differs from the one before in 66 files, each the id or a hash that follows from it: the four sheets' code and schema hashes, their shader file names, and the run keys of the SIR sheet's sweep.
No number of any run moved, and the snapshot after this package became the new baseline.
The package checked the change in the live app, on the built-in display.

#### App fixes

Each fix came with a test in `crates/henad-app/src/tests/` or the CLI's tests.

- `6662055`: the Export tab writes the `time` column for a model with a time scale (`the_export_tab_writes_the_time_of_a_timed_model`).
- `6662055`: a sweep of a sheet past a work budget is rejected in the sheet's lines, in the CLI and in the Sweep tab.
- `6662055`: the button form keeps the steps that set a property everywhere.
- `6662055`: "Model applied." clears on the next edit.
- `6662055`: the world panel keeps a value a file holds outside its range.
- `6662055`: Run to tick drives a sheet's preview.
- `6662055`: the Model tab counts the parameters the Parameters tab shows.
- `6662055`: the Editor asks before New model, New grid rule, New node graph, Open model, Open in Editor or Edit model replaces a model with unsaved edits, with Cancel, Discard changes and Save model.
  Apply saves nothing, and only Save model clears the unsaved state.
  There is no autosave.

The package checked four of the fixes in the live app, with a screenshot of each.

#### CI's web clippy and the ids of new models

`6662055` fixes the two failures of CI's web clippy step, described under [CI's web clippy step](#cis-web-clippy-step).
`6662055` keeps an id the Editor mints for a new model off every compiled model of the set.
A Blank model named "Game of Life" gets `sheet_game_of_life`, and a new node graph `node_<name>`, with a number if that id is held.
A file opened with an example's id still replaces the example on Apply, since the id is the file's.
Four tests in the new `tests/model_ids.rs` cover it.

#### Sheet pane

Sections became framed regions with small upper-case titles, one label column and one field column each.
Tables take header labels, hairlines and a hover row, add buttons are dashed, and selects and pickers are the card's overlays.
Nothing draws past the pane's edge: a table's columns give up width in an order `agents/henad-app-sheet-pane.md` states, long values end in an ellipsis with a tooltip, and the last resort is a scroll inside that table.
The pane was checked in the live app and offscreen, at 1,500 by 950 and 1,280 by 800 and with the pane dragged to 320 and 600 points.
The implementer's commit was rejected by the permission system, and `825eaf6` is the session's commit of that work, made after the maintainer's answer "commit it unsigned like the rest".
It holds the whole package in one commit of 13 files.

#### Cleanup wave 2

Four branches over henad-graph's sheet folder, 69 commits: the wording, the document and checker, the lowering, and the edit commands with the conversion.
`01eed55` makes `sheet/sentence.rs`, 5,291 lines, a folder of `mod.rs` and eleven files, with every public path kept.
`01eed55` moves the fault words of a look into the WGSL compiler.
The compiler no longer imports the sheet layer.
`d7a5310` splits the lowering's `ground.rs` into `paint.rs`, `place.rs` and `shades.rs`, and `0e64f19` splits the edit commands into five files.
`01eed55` drops four unused wording entries, and `d7a5310` two unreachable errors.
The checker's and the agent lowering's notes became topic files of their own.
The integration step reported 2,262 tests passing, and the snapshot after wave 2 is identical to the baseline.

#### Test code outside henad-graph and henad-app

23 commits over the tests of henad-models, henad-explore, henad-compute and henad-cli, with 2,091 lines added and 3,023 removed.
Shared helpers went to henad-models' `src/tests/support.rs` and `tests/support/mod.rs`, the cross-engine gates of ants, boids and SIR to `tests/gates/`, and henad-cli's to `tests/support/mod.rs`.
The package reported the set of passing test names unchanged, 848 tests in the four crates.

#### Edit refusals and no rate inside a Repeat

The cleanup of the edit commands reported two faults and left them for this package.
`091c468`: a new id for an item the sheet does not hold is rejected, where it changed nothing and returned success.
`091c468`: Change to a rate is rejected on a card that runs once, as the card already does not offer it.
`d1edb54` and `d1edb54` do the same for a line inside a Repeat: the card no longer offers Change to a rate there, and the command rejects it with "a rate cannot stand inside a Repeat".

#### Header, notices, dialogs and theme

The header's first row holds New model, New grid rule, New node graph, a hairline, Open model and Save model, with Apply at the right as the one filled button.
The second row holds the model's name, a dot with a tooltip while there are unsaved changes, the file name, and for a sheet Undo, Redo and the model actions menu.
Every message under the panes is a notice with a bar and an icon, in blue, orange or red.
New model, New parameter, Rebuild model, Convert to node graph and the unsaved changes dialog share one frame: a small title over a hairline, buttons at the bottom right with the primary last, Enter and Escape.
New model's starters are full-width rows, the help popover is an overlay like the pickers, and both segments of the Lines and Diagram toggle take the keyboard focus.
The Sheet pane's width is stored as a share of the Editor's width per layout.
Five theme values changed in `crates/henad-app/src/init.rs`: the window stroke to gray 600, no popup shadow, the selection fill to blue 900 under a blue 200 stroke, the active widget's outline to blue 500, and no trailing fill on sliders.
`agents/ui.md` lists each with its value before.
The graph editor's notes moved from `agents/henad-app-editor.md` to `agents/henad-app-graph-editor.md`.
The header, notices, dialogs, menus and the other dock tabs were checked in the live app on the virtual display, and offscreen for the states the live app could not reach.

#### henad-cli and the example models

`29d0d27` parses `--set`, `--vary`, `--stop` and `--reduce` once.
`29d0d27` moves the seeding and packing helpers the GPU ports share into `henad-models/src/port.rs`.
`29d0d27` splits henad-cli's `lib.rs` and `explore.rs` by concern, into `single.rs`, `model_file.rs` and an `explore/` folder.
`29d0d27` adds `henad_core::action::parse_entry`, and `Schedule::parse` and the sweep's `--act` call it.
The package reported 68 CLI commands byte for byte equal with the build before, and its snapshot is identical to the baseline.

#### henad-graph's test code

24 commits, with 4,063 lines added and 4,970 removed.
Shared helpers went to `src/tests/support.rs` and the new `src/tests/sheet_support.rs`.
`b03b32d` splits the two longest test files, `sheet_edit.rs` and `sheet_lower.rs`, into five new files.
The package reported no test deleted and the set of test names unchanged.

#### Sweep and Results tabs

`43d1acb` renames `DraftMode` to `ExploreMode` and replaces the "sweep" and "search" strings passed around with it.
`43d1acb` makes the four Results views share one rule for when to recompute.
`43d1acb` and `43d1acb` make `ui/results/store.rs` and `ui/sweep/draft.rs` folders, as pure moves, and `43d1acb` gathers the text helpers the tabs share in `ui/text.rs`.
The package compared every user-visible string, old against new, in a test it then removed.

#### App shell

`79d3b4c` shares one run teardown, `79d3b4c` holds the GPU batch settings as one value, and `79d3b4c` moves the snapshot intake from the frame loop into `AppState`, where a test calls it.
`79d3b4c` and `79d3b4c` split the Parameters panel and `AppState`'s methods by concern, into `ui/params/` and `state/*.rs`.
Three fixes came with it, each with a test.
Density mode panicked on an empty agent population (`79d3b4c`).
A whole-number slider wrapped negative above 2,147,483,647 (`79d3b4c`).
A chart of one sample, at tick 0, spread its x axis as wide as its largest stat, and now shows one tick (`79d3b4c`).
`79d3b4c` makes the app ask before its window closes over unsaved edits, on native, with the dialog that guards a model's replacement.

#### The guide

`docs/guide/app.md` and `docs/guide/editor.md` follow the restyled app.
13 screenshots under `docs/assets/app/` were retaken and 4 added, from the live app on the virtual display.
Eight Sweep and Results screenshots were kept: they differ only in the fill of selected toggles.
`a3ffa74` adds the wording table's missing row for `LOOK_IN_REPEAT`.

#### The Editor's code

`59096ee` fixes a fault the cleanup found.
A typed number in an agent line's times, strength, minimum, maximum, amount, length or distance, or inside a Choose row, opened the picker's Number box empty.
The app's copy of the slot paths knew some of them, and henad-graph's new public `sheet::edit::slot_value` now reads every path.
Eight tests in `sheet_agent_lines.rs`, one per kind of slot, hold the typed number in the Number box.
`59096ee` makes the Sheet pane's and the card's pickers share their parts, and `59096ee` moves the kind colour into the component.
No call site passes one.
`59096ee` makes the graph editor's quick-add use henad-graph's rules instead of its own copies, with `NodeDef::sink` and `ContextSet::allows` made public.
`59096ee` does the same for the card with `LineKind::works_out_result` and `LineKind::is_block`.
`59096ee` moves the line data out of `card_pane.rs` into `card_lines.rs`, and `59096ee` splits `sheet_pane.rs` from 3,503 lines to 798 and six new files beside `sheet_pane/agents.rs` and `sheet_pane/layout.rs`.
Both splits move code and change only imports, paths and visibility.

#### CHANGELOG

`59096ee` writes the round's lines into `CHANGELOG.md` from the packages' notes.

#### henad-app's test code

26 commits over `crates/henad-app/src/tests/`, with 2,243 lines added and 2,737 removed.
Shared helpers went to `support.rs`, from 858 lines to 1,279, and the helpers that drive the Sheet pane and the Card pane to the new `panes.rs`.

Of the 25 findings, 23 were applied, one in part, and one was skipped: it needs a private function of the Editor made visible to the tests.
The rebuild-from-Blank tests share builders that return the text after each step, so each test still walks the steps it asserts on.
One `TALL_SCREEN` and `Harness::tall()` replace the set-up of 49 sites, and the three rules by which a test finds a sentence on screen have a name each.
Two checks became stricter, the label-in-name rule and the preview's forbidden words, and found no violation.
One test was deleted, `apply_installs_and_builds_the_sheet`, after its four own assertions moved into `applying_the_life_sheet_keeps_the_example_model_of_that_name`.
Two tests moved to the files of their subjects.
henad-app's tests pass 549 after it against 550 before, and the test phase took 8.94 s against 9.53 s.

### How it was checked

#### The isolated full check and the shared build directory

For some hours on 2026-11-11 every agent built into one shared cargo target directory.
Cargo names a workspace crate's artifacts the same in every worktree and judges freshness by file times, so in that directory one agent's tests could run on crates compiled from another worktree.
The session had set this up, and it was the session's mistake.
An agent found it, every agent went back to a target directory inside its own worktree, and the briefs now forbid a shared one.

The isolated full check then ran from 18:13 to 18:34 UTC, in one worktree with its own build directory.
Its tree was `bec8310` with the henad-cli package's 15 commits on top, before those commits were rebased once more and merged as `29d0d27` to `29d0d27`.
It ran every step of `./check.sh` and CI's two web clippy lines, 16 steps, with `HENAD_REQUIRE_GPU=1`.
All 16 exit 0.
The workspace tests pass 2,257 and ignore 2, the float probes, and none fail.
The doc tests pass 42 and ignore 4, the README program's pass 2, and `cargo deny` reports no advisory.
Everything merged in the window of the shared directory sits in that tree.
The packages merged after it come into the final full check.

#### The snapshot of every model's output

| Snapshot | Against | Result |
| --- | --- | --- |
| After the first four branches of wave 1 | `bd62a9c` | Identical outside the loose runs |
| `d4a96cb`, after wave 1, the card and the wording fixes | `bd62a9c` | Identical outside the loose runs |
| The sheet starter ids | `bd62a9c` | 66 files differ, each the id or a hash that follows from it. Became the baseline |
| After wave 2 | the baseline | Identical outside the loose runs |
| `bec8310` with the henad-cli package | the baseline | Identical outside the loose runs |

The test cleanups change no code the snapshot runs.
The app's cleanups change henad-app, and henad-graph only by the public items named under [The Editor's code](#the-editors-code).

#### On screen and offscreen

The Editor layout, the app fixes, the sheet starter ids, the Sheet pane, the header and dialogs, and the guide's screenshots were checked in the live app, driven through the egui MCP server.
The Editor layout's and the ids' screenshots were taken on the built-in display.
The switch to the virtual display came during the header package, whose first two screenshots were taken on the built-in display.
The card was rendered offscreen only, while the screen was locked.
States the live app could not reach were rendered offscreen too.
The offscreen driver is a scratch test module that runs the Editor's own UI code, with the app's fonts and theme, in the test harness, and draws each frame through `egui_wgpu` at 2 pixels per point.
It was never committed.
Keyboard focus was seen offscreen only: the live app on the virtual display never holds the system's focus, and egui draws no focus without it.
The three app cleanups were told to change nothing the app draws, and ran no app on screen.

A pixel comparison then tested that claim.
It rendered 62 scenes of the Editor offscreen at `3102bbb`, with the restyle complete and no app cleanup merged, and at `59096ee`, after the three app cleanups.
The scenes are the whole Editor for five sheets, the Card pane at 420 and 640 points, the Sheet pane at 320 and 600, pickers, selects, menus, dialogs, notices, keyboard focus, the graph editor's panels and a grid rule.
All 62 are identical, pixel for pixel.
The method was checked first: three renders of one tree were identical, and a planted change of one outline colour was caught, at 1,340 pixels.
The node canvas of the graph editor is masked in its scene, since egui-snarl draws overlapping nodes and wires in an order that changes with every process.
Fire counts on a playing preview, the Charts tab and the Sweep and Results tabs are not among the scenes.

#### CI's web clippy step

CI's web clippy step, clippy on the pinned nightly with atomics for henad-app and the facade, ran locally for the first time in this round.
It failed in two places the branch had introduced.
`SimRunner` in `sim_runner.rs` met `clippy::large_enum_variant` on wasm32, and now carries an `expect` with its reason.
`agent_hooks.rs` built an array from a tuple by hand, and now converts it with `into()`.
`6662055` fixes both, and both lines of the step pass in the isolated full check.

### Edited tree

```
AGENTS.md                                      cpu/tests/ among the test folders, henad-cli's model-file line, new topic files
CHANGELOG.md                                   round 6's lines (bca71d3, 59096ee)
agents/
├── henad-app-graph-editor.md                  new, from henad-app-editor.md
├── henad-app-sheet-pane.md                    new: sections, form rows, tables, column order
├── henad-graph-sheet-check.md                 new, from henad-graph-sheet.md
├── henad-graph-sheet-lower-agents.md          new, from henad-graph-sheet-lower.md
├── henad-graph-sheet-wording.md               new, from henad-graph-sheet.md
├── ui.md                                      palette table, style rules, theme values of overlays and focus
└── ...                                        the topic file of every area a package changed
crates/
├── henad-app/
│   ├── assets/fonts/Henad Sans SemiBold.ttf   new: the card's lead words
│   └── src/
│       ├── init.rs                            five theme values
│       ├── state/                             fault.rs, files.rs, open.rs, preview.rs new: AppState's methods by concern
│       ├── tests/
│       │   ├── model_ids.rs                   new: minted ids avoid compiled models
│       │   ├── panes.rs                       new: helpers that drive the Sheet pane and the Card pane
│       │   ├── support.rs                     the helpers the app's tests share
│       │   └── unsaved.rs                     new: the unsaved changes dialog and the close request
│       └── ui/
│           ├── dock.rs, menu_bar.rs           the Default and Editor layouts
│           ├── params/                        new folder, from params.rs: mod, schedule, seed, trial, values
│           ├── results/
│           │   ├── labels.rs                  new
│           │   └── store/                     new folder, from store.rs: axes, recorded, replay, series_cache, statistics
│           ├── sweep/draft/                   new folder, from draft.rs: check, levels, messages, spec, ExploreMode
│           ├── text.rs                        new: text helpers the tabs share
│           └── editor/
│               ├── history.rs                 moved from graph/history.rs
│               ├── program.rs                 new: compiled programs both editors share
│               └── sheet/
│                   ├── card_lines.rs          new: line data out of card_pane.rs
│                   ├── card_pane.rs           framed cards, kind bars, chips
│                   ├── sheet_pane.rs          798 lines, from 3,154 at bd62a9c
│                   ├── sheet_pane/            buttons, colors, layout, measures, parameters, property_form, starts new
│                   └── widgets.rs             regions, hairlines, overlays, notices, kind_hue, kind_bar, add_row_button
├── henad-cli/
│   ├── src/
│   │   ├── explore/                           new folder, from explore.rs: args, mod, report
│   │   └── model_file.rs, single.rs           new, from lib.rs
│   └── tests/support/mod.rs                   new
├── henad-compute/src/
│   ├── cpu/tests/                             new: runner tests out of sim_thread.rs, support.rs
│   └── gpu/built_pass.rs, press.rs            new: parts both GPU engines share
├── henad-core/src/fnv.rs                      new: the FNV-1a hasher
├── henad-explore/
│   ├── src/sweep/                             new folder, from sweep.rs: error, manifest, prepare
│   └── tests/fixtures/series-without-time/    renamed from series-be338ec
├── henad-graph/
│   ├── sheets/                                game_of_life, sir, boids and ants take sheet_<name> ids
│   └── src/
│       ├── host/program.rs                    new
│       ├── sheet/
│       │   ├── edit/                          colors, command, declarations, ids, line_commands new, slot_value
│       │   ├── lower/                         paint, place, shades new, from ground.rs
│       │   └── sentence/                      new folder, from sentence.rs: mod.rs and eleven files
│       ├── wgsl/                              the look's fault words, no import of the sheet layer
│       └── tests/
│           ├── sheet_support.rs               new: shared sheet test helpers
│           └── sheet_*.rs                     five new files, split from sheet_edit.rs and sheet_lower.rs
└── henad-models/
    ├── src/port.rs                            new: helpers the GPU ports share
    ├── src/tests/support.rs                   new
    └── tests/
        ├── gates/                             new: ants.rs, boids.rs, sir.rs, the cross-engine gates
        └── support/mod.rs                     new
docs/
├── assets/app/                                13 retaken, editor-{layout,new-model,picker,unsaved}.png new
├── developing/agent-record/
│   └── 20261111-107-open-questions-restyle-cleanup.md   this record
├── guide/app.md, editor.md                    layouts, header, notices, Sheet pane, card, dialogs, unsaved changes
└── reference/model-files.md                   replay of agent models
zensical.toml                                  nav entry for this record
```

## State after

- `37-node-editor` at `7e845bb` holds 407 commits after `bd62a9c` and 660 after `bad936b`, and this record follows them.
- Every commit is unsigned.
  The 407 of this round are one line with no `[draft]`, and the 253 before them still carry `[draft]`.
- Nothing was pushed, tagged or opened as a PR.
  The remote's `37-node-editor` is at `bad936b`.
- The isolated full check passed 16 of 16 steps on the tree of `bec8310` with the henad-cli package, with 2,257 tests passing, 2 ignored and none failing.
  The final full check passed the same 16 steps at `7e845bb`, with 2,276.
- This record changes only the record and its nav entry.
  `uv run zensical build` builds the site with it, and the scripts' tests pass.

The final full check ran at `7e845bb`, in a worktree with its own build directory and nothing else building into it.
All 16 steps exit 0: every step of `./check.sh` and CI's two web clippy lines, with `HENAD_REQUIRE_GPU=1`.
The workspace tests pass 2,276 and ignore 2, the float probes, and none fail.
The doc tests pass 43 and ignore 4, the README program's pass 2, and `cargo deny` reports no advisory.
The snapshot at `7e845bb` is identical to the baseline outside the five loose runs.

The commit messages of the branch are rewritten after this record is committed, as the maintainer asked.
A replay keeps each commit's tree, author, committer and dates and changes its subject alone.
After it no subject has more than six words, the subjects average about 4.2 words, and none carries `[draft]`.
The history before the rewrite stays under the local ref `refs/backup/37-node-editor/20261111-pre-rewrite`.
One commit follows the rewrite: it replaces every hash of the branch that a record or a topic file cites with the hash the commit has after the rewrite, in this record too.
A folder named after a hash, `series-be338ec` in record #77, keeps its name.

## Issues found & future directions

### Faults left in

- **Nothing autosaves.**
  The Editor asks before a model with unsaved edits is replaced or, on native, before the window closes.
  Apply saves nothing.
- **A model file with an example model's id** replaces that example on Apply, for the session.
  New models and starters take ids no compiled model holds.
- **The sheet checker** words about 100 of its errors as string literals in `crates/henad-graph/src/sheet/check/`, outside the wording module.
  Most are errors of a hand-edited file, such as an id that breaks the grammar or a line listed twice, that the editor cannot produce.
- **Focus.**
  A focused text field is outlined in blue 200, where every other control's focus is blue 500: egui outlines it in the selection stroke.
  A focused slider shows only its handle.
- **The model actions button** of a sheet's name row shows no open state while its menu is open.
- **A short tail of a card sentence**, such as ", always" or "per day", can wrap onto a row alone in a narrow Card pane.
- **A narrow Editor** in the Default layout leaves the Sheet pane about 180 points wide, where tables wrap their headers and end values in ellipses.
- **Playback's "Press Play to run the preview."** keeps the old style, weak text with no notice bar.
- **An expert copy's banner** dates the conversion in UTC, and the copy is written on the UI thread.
- **The pass names `colours` and `colours_ground`** and `SheetSection::Colours` break the American spelling rule for code.
  Renaming the passes changes every sheet's code hash.
- **The CPU agent engine** falls back to 10,000 agents in a 1000 by 1000 world for a parameter list too short to hold them, whatever the model declares (`crates/henad-compute/src/cpu/agent_engine.rs`).
  Only `ModelEntry::build` or `from_params` with a short list reaches it.
  The cleanup left it: a fix changes behaviour.
- **Records #92 and #105** still describe the any-time rule inside a Repeat.
  Round 5 replaced it.

### Not checked on a screen, in a browser or on other hardware

- **Device tests on DX12 or a hardware Vulkan adapter, and CI on the branch.**
  The tests ran on an Apple M4 Pro, and on lavapipe in a local container in round 5.
- **The restyled Editor, seen by a person.**
  Screenshots, offscreen renders and the live app on the virtual display were judged by the session.
- **Keyboard focus in the Editor**, seen offscreen only.
- **Fire counts arriving without moving a row** rest on two app tests (record #104).
- **Game of Life built from Blank by hand** rests on a test that rebuilds it by accessible names (record #98).
- **The legend order, Charts after Run to tick, and the Measures rows** rest on tests (record #105).
- **The editors in a browser**: after this record's other checks, the sheet editor was driven once in headless Chrome on the web build of the final full check.
  The page loaded cross-origin isolated with a WebGPU adapter and 14 workers and logged no error.
  New model with the SIR starter opened the Editor layout, the preview played, a state chosen in a chip's picker changed line 3 and restarted the preview, and Undo restored it.
  The node graph editor, keyboard input, Apply and Build at full size, and every other browser were not tried.
- **Save model, Open model and Import function** through the native file dialogs rest on tests that record the saves.
  Import function never ran in the app.
- **A float Repeat body on another GPU.**
  The two ignored probes fail on Metal and lavapipe.
- **Expert canvas cases** rest on reading the source: a dropped wire's menu after its node goes, a socket lost under a live wire, a floated Editor tab, a region dragged whole, a touch drag.
- **The sheet and the card with people.**
  Simulated participants and one rater met none of the agent sheet, the agent card, expert mode or a Repeat.

### Below a target

Every speed here was measured on one machine under load, in record #106 unless another record is named.

- **Game of Life as a sheet or a graph** runs 541 steps a second at 4096 by 4096, against 25,559 for stock `gpu_game_of_life`.
  Bit-packed lanes for node models would close the gap.
- **The SIR graph starter** takes 1.344 times the stock time at 4096 by 4096.
  The cause is not traced.
- **The ants graph at small populations** runs 60 to 90 percent slower than stock at 2,000 ants (record #68).
- **Boids as a node model** is rejected above 201,012 birds in its default world.
  Capping the steps of one submission by the budget would admit more, at single steps of seconds.
- **The walk charge**: 64 steps at 200,000 birds took 3.5 s, against the budget's 1.7 s.
- **The ants files' world slider** is rejected past about 7,200, 6,900 and 5,100 a side (D117).
- **A look at agents** faults past 65,536 candidates around one agent, and the budget reads the walk at tick 0 with an even spread.
- **Chances** resolve to 2^-24 on most paths, and a rate per element below about 3e-8 per tick never fires.

Never measured: every set on a cooled, idle machine with reps over about 0.5 s, SIR at parameters that keep the epidemic alive, stock `gpu_boids` at several densities, the old agent engine against the spec-built one with a stats sample every tick and in a GPU sweep (record #63), fire counts at batches of 1 and 64 steps (record #74), the sheet library's own costs, the wasm32 stack at the nesting limit of 24, the agent preview's cap of 16,384 on a slower GPU, the stock models' streams under PractRand, and the barrier that zeroes the tallies.

### Written up and not built

Sizes: S under 50 lines, M a few hundred, L a subsystem, XL research or a new engine feature.

- **Bit-packed lanes for node models** (XL): states packed into bits through the compiler's lane layout and stencil reads.
- **A fixed visit order** (XL), so boids and bounded confidence replay.
- **Edits that take effect without a restart** (XL): an engine feature that carries a running world across a rebuild.
- **Rates on agent cards** (M to L): rate items in the agent Chance menu, a device run of a rate over agents, and a starter with a time scale.
- **The missing editor controls** (L).
  No control writes Velocity's start or a measure over an Arrow, paints the ground, or writes "only those whose" on a look.
  Boids, Ant foraging and Bounded confidence cannot be rebuilt from Blank for that reason, and Majority vote has no rebuild test.
- **A host-against-device test of agent sheets** (M), with bands on a measure such as mean speed.
  An exact lockstep fails on rounding.
- **Fire counts on the kinds of line that show none** (M).
- **Run to tick samples in a browser** (L): a stats readback per sample that does not block.
- **Left out of the plan from the start**, each needing a design of its own: explaining (hover to tint, Explain this one), the world's further parts (a ground card, walls, births, deaths, a second kind of agent), values carried between times round a Repeat and rates inside one, an expert line inside a card, a code hash over less than the whole lowered text, the state diagram view and presets, painting cells, and networks as node models.

### The round's process

- **The shared build directory.**
  For some hours agents built into one cargo target directory, the session's own mistake, described under [How it was checked](#how-it-was-checked).
  The tree as merged up to `bec8310` passed the isolated check, and the final full check at `7e845bb` covers the packages merged after it.
- **A rejected commit.**
  One agent's `git commit --no-gpg-sign` was rejected by the permission system.
  The session did not make the commit for it, asked the maintainer, and committed `825eaf6` after the answer.
  That package's work is one commit of 13 files, against one concern per commit.
- **The usage limit.**
  The session hit the usage limit once on 2026-11-11 and resumed.
- **The maintainer's display.**
  Until the maintainer's instruction of 2026-11-11, "run your app on the virtual screen instead", agents launched the app where the maintainer's stored window put it, full screen on the built-in display.
  From then on every launch went to the virtual display.
- **The finders.**
  The first finders stopped early at the maintainer's request to save usage.
  Six more areas were read on 2026-11-11.
- **Commit messages.**
  One message has seven words, and the round's average is 5.34 words against the 4 asked for.
  The rewrite of all messages is the last step of the round.
- **The push, the signing and the release** stay with the maintainer.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
