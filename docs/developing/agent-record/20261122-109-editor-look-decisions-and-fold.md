---
date: 2026-11-22
title: "Editor look, decisions and the fold"
description: "The maintainer asked the session to take the recommended option of each decision record #108 left open, to bring the Editor back to egui's stock look, and to fold the branch's history into about 100 commits. Five packages took 15 decisions and nine points on the Editor's look in 74 commits, and five more commits bumped url for a new advisory, folded the changelog and retook 19 of the guide's 30 screenshots. A snapshot of every model's output keeps every deterministic model's statistics identical, and the full check passed 16 of 16 steps with 2,452 tests. After this record the branch's 933 commits since master are folded into 98 signed commits and one that remaps the hashes the records cite, and the branch is force-pushed."
icon: material/source-branch
status: ai-generated
model: "Claude Code. Each package had an implementing agent and a reviewing agent, and this record's draft was an Opus subagent"
issue: "#37"
state: "37-node-editor at d95771ad and this record: 81 commits after 6141c2e3, the remote's head of the branch, and 933 after master's 4f1ea055. The full check passed 16 of 16 steps at 13205c3c with 2,452 tests, and the snapshot there keeps every deterministic model's statistics identical. The history is folded into 98 signed commits after this record, one more commit remaps the hashes the records cite, and the branch is force-pushed"
baseline_commit: 6141c2e3
delta_state: "committed on `37-node-editor`: 79 unsigned commits of eight packages, the maintainer's signed commit `f309fd29`, and this record, all folded after it"
---

# Editor look, decisions and the fold

> The maintainer made five requests on 2026-11-22: take the recommended option of each decision record #108 left open, restyle the Editor in seven points, unframe the Sheet pane's sections, bring the Card pane's controls in line with the rest of the app, and fold the branch's commits into about 100.
> The work ran from `6141c2e3`, the remote's head of the branch.
> Five packages, each implemented by one agent and reviewed by another, took 15 decisions and nine points on the Editor's look in 74 commits.
> Five more commits bumped `url` for a new advisory, folded the changelog's lines and retook 19 of the guide's 30 screenshots.
> A snapshot of every model's output keeps every deterministic model's statistics identical.
> Its 15 strict differences are two lock files, eight shaders of the ants models and five new shaders.
> The full check passed 16 of 16 steps at `13205c3c` with 2,452 tests.
> A user meets egui's own frames and shadows again, help on hover, new dialogs before a replace and before a delete that empties formulas, the Editor on 70 percent of the window, percent values typed as "2.5%", and files rejected for names that clash with columns of the exported tables.
> After this record, the 933 commits since master are folded into 98 signed commits and one that remaps the hashes the records cite, and the branch is force-pushed.

## State before

`37-node-editor` at `6141c2e3` held 852 commits after master's `4f1ea055`, 44 of them signed.
The maintainer had pushed it after record #108, and the remote's head of the branch was `6141c2e3`.
At record #108 the full check passed 16 of 16 steps with 2,385 tests, and the snapshot of every model's output kept every deterministic model's statistics identical to its baseline.
Record #108 left seven findings whole and several in part, each with its options and the reviewers' picks.

## What was done

### The requests

The maintainer's requests, in their words where the sources quote them:

1. "Take recommended choices of decisions."
   The decisions are the items record #108 left with options, together with the recommendations of its reviewers.
2. Seven points on the Editor's look, quoted below.
3. "on sheet, the rectangular outlines for each section seems redundent given that the headings are separated with a line decoration. remove the rectangular boxes"
4. "consider unifying component styles. Right now the card editor styles (especially buttons) seems to stick out from the rest of the app."
5. "fold the commits into around 100 commits with rewritten commit messages. 800+ commits is just crazy", and then "they are all pushed, but you can force-push".
   Every commit of the fold is signed, once the maintainer unlocks the key.

The seven points of the second request:

1. "are all dropdowns now custom? I don't like the new brighter border around dropdowns/hover cards/etc. Revert back to what it used to look like (it used to have shadows i think, just don't touch it plz)"
2. No callout bar at the left of the notices, "The colored icons themselves are clear enough", and no bar anywhere else but beside the card's lines, the text of the tick rule included.
3. "don't use monospace text for table headings or small texts, and don't use all caps."
4. "question marks should be hovered helps instead of click-to-open help panels."
5. "restyle dashed-border buttons to faded solid outline instead of current bright dashed look."
6. "use proper symbols instead of things like '->' in the editor."
7. "default editor split layout should have a larger fraction for the left and a even smaller fraction for the right."

### How the work ran

Each package ran in its own git worktree on its own branch from `37-node-editor`, from a brief the session wrote.
One agent implemented it, a second agent reviewed it, and the session merged the branch by fast-forward.
The briefs required each agent to build into a target directory inside its own worktree.
No virtual display was connected, and the briefs forbade launching the app on screen.
The packages that changed the UI rendered the Editor offscreen through a scratch test module that runs the app's own UI code, and the module was never committed.

| Package | Commits | Merged at |
| --- | --- | --- |
| Sheet checker: names that clash with columns, add-up marks, any and all, kept shares | 7 | `fe7e8329` |
| Editor look | 18 | `8694d2a1` |
| GPU counters and submission sizes | 13 | `783bba2a` |
| Whole numbers, percent values, clock columns and the resume note | 19 | `85c9e948` |
| The `url` advisory | 1 | `4b06fcc2` |
| Editor behaviour: replacing, saving, deleting, the Apply line, parameter ids | 17 | `13205c3c` |
| The changelog | 1 | `f3851adb` |
| The guide's screenshots | 3 | `d95771ad` |

`f309fd29`, the maintainer's own signed commit before the packages, rewrites the AI usage section of the contributing page.
The 79 commits of the packages are unsigned, by the maintainer's choice of 2026-11-21.
Their subjects keep to six words and average 4.9.

### The decisions

The 15 decisions and the option taken for each:

| Decision | Option taken | Package |
| --- | --- | --- |
| A whole-number sheet parameter with a negative minimum | Marked integer in its descriptor, typed fractions rejected, samplers draw whole numbers | Whole numbers and percent values |
| A stat named `time` or `tick` | The checkers reject it, and `run_id` too, with debug assertions for compiled models | Sheet checker, and whole numbers and percent values |
| Counters that wrap at 2^32 | 64-bit counters with a carry | GPU |
| The work charge of a radius fold | The largest bucket of the index, measured after the first step, sizes later submissions | GPU |
| State names cut off in the property tables | Names wrap under their swatch | Editor look |
| Two sheets sharing one model id | Apply asks before it replaces | Editor behaviour |
| Percent values in the Sweep tab | "2.5%" read wherever a value is read | Whole numbers and percent values |
| The add-up wrap of worked-out amounts | A warning from a population of 65,536, and the bound in the tooltip | Sheet checker |
| A share kept outside 0 to 1 | A warning instead of an error | Sheet checker |
| Any or all of a value that is not yes or no | An error at the summary's own line | Sheet checker |
| Two Editor saves pending under one target | Each save numbered | Editor behaviour |
| A delete that empties a formula | A question first, naming the lines and measures | Editor behaviour |
| The Apply line after a rejected build | An outcome of its own that follows the reason | Editor behaviour |
| A converted copy's zoom | Zoom 0.5 or more on its first frame | Editor look |
| A resume rejected over a changed model file | The CLI names the model file | Whole numbers and percent values |

Record #108's other items stand as it leaves them.

### The packages

#### Sheet checker

- A sheet measure or a graph's `[[stat]]` named `tick`, `time` or `run_id` is an error.
  `--export-stats`, `series.csv` and the Export tab wrote two columns of that name.
  The name is compared exactly, and `Time` passes.
  The sheet's message reads "The measure name tick is reserved for the tick column of exported stats or sweep results."
- A grid rule's `[[state]]` labelled `tick` or `run_id` is rejected at its table.
  A rule writes no `time` column, and `time` stays allowed.
- A Leave of a worked-out amount on a ground property that adds its marks up is a warning when the kind's `max_count` reaches 65,536.
  The Marks tooltip states the bound of Add up: at most 65,536 on one cell in a tick, in steps of 2^-16.
- An any or all summary of a value that is not yes or no is an error at the summary's own line.
  Before, only the lines that read it failed, in the lowering's internal words.
- A share kept each tick outside 0 to 1, as a number or through a parameter whose Max passes 1 or whose Min falls below 0, is a warning at its ground property.
  It was an error.
  The lowering does not clamp the share.
  The warning says that each tick multiplies the property by the share, and above 1 that its marks grow every tick.
  The Sheet pane still rejects such a share typed in its field.

#### Editor look

- `crates/henad-app/src/init.rs` drops the theme overrides of the restyle of record #107, and now differs from 0.3.0 only by the semibold face.
  Popups, menus, tooltips, combo lists and windows take egui's frame again: a 1 point gray 800 border and egui's popup shadow.
  The selection, the slider's trailing fill and the focus outline, 1 point of white, are egui's again.
- The pickers, add menus, the line menu, select lists and Choose a colour draw egui's popup frame.
  The New model, New parameter, Rebuild model, conversion and unsaved changes dialogs draw egui's window frame.
- To the maintainer's question in the first point: not every dropdown is custom.
  The selects of the Sheet and Card panes, `select_within`, `select_list` and `select_control`, stay custom and now draw egui's combo and popup frames.
  A `ComboBox` would lose their arrow keys, focus ids, swatches, disabled items with their reason, and placement.
  The graph editor and the tabs outside the Editor use egui's `ComboBox`.
- A notice is its coloured icon and its text, with no bar at its left.
  Neither the card's tick rule nor "What is the world made of?" in the New model dialog has a bar beside it.
  The kind bar at the left of each card line stays.
- Titles of sections, overlays and dialogs are set in the semibold face, in sentence case.
  Counts, table headers, group labels, line numbers and fire counts are proportional at the theme's small size.
  They were mono 11, and titles and counts were upper case.
  The mono face stays for a value shown as typed and for code.
- A "?" shows its help as a tooltip under the pointer and on keyboard focus, wrapping at 320 points, and its text is the button's accessible description.
  The help popover and its Close help button are gone, and with them henad-graph's `sheet::sentence::CLOSE_HELP`, `help_dialog_name` and `LINES_HELP_HEADING`.
- Add buttons, blanks, empty selects and an empty card draw weak words inside a faded solid gray 800 outline, gray 600 on hover.
  They were dashed gray 500.
  The Diagram's "Inside line N" frame is solid gray 700.
- The Editor's text writes "…", "→", "×" and the like: "+ Repeat…", "New parameter…", "when … is …", and "Searching → Food way" for a Depending on.
  A blank reads "…" (`BLANK_TEXT`) in sentences, notes, a saved sheet's comments and automatic names.
  A name stored with "..." still follows its line, and the next edit writes "…".
  File formats, typed input and code views keep their characters.
- In the Editor layout the Editor takes 70 percent of the width, where it took 62.5.
  The Model tab joins Playback and Pacing, and Charts holds Parameters and Statistics.
  At 1280 points wide, the tab bar of the old stack cut Model off.
- The Sheet pane's sections lose their frame, and each is a title over a rule to its "?".
- Chips, selects, the condition arrow, "always", "for each", "+", "×", the line menu's dots, a note's buttons and the Lines and Diagram toggle take egui's stock button look.
  A chip keeps its fill, and a red chip stays red inside the selection's outline.
- A state's name in a property table wraps under its swatch before the Kind column gives up width.
  At 1280 by 800 it was cut to "Susce…".
- A converted copy, from Convert to node graph or Edit as node graph, opens at zoom 0.5 or more on its first frame, at the top left corner of a pass too large to fit.
  Frame all is unchanged.

#### GPU counters and submission sizes

- `counter.add` and `stat.counter` hold 64-bit totals.
  Each counter takes two words in the engine's counter buffer, low then high, and two in each workgroup's tally.
  An add that wraps a low word carries into its high word, in `counter.add` and at the workgroup flush.
  The host evaluator's `HostLanes` counters are `u64`.
  A sheet's Record line or event measure no longer wraps at 2^32 events.
- A new pass, `crates/henad-compute/src/gpu/primitives/hash_largest.wgsl`, reads the largest bucket of the spatial index into one word (`LargestBucket`).
  A graph of agents with a radius fold sends the first step after a build or a press alone.
  Each later submission then holds at most the steps that fit the submission budget with every cell a walk covers as full as that bucket.
  The bucket is measured again with a stats readback, at most once every 64 ticks, and after a press.
  The build is still charged for an even spread.
- `GpuSimState::submission_steps`, 64 by default, bounds each command buffer in the GPU runner, `stepping`, `run_batched` and the sweep's GPU tracks.
  A model without a fold keeps 64 steps from the start and never waits for a measurement, stock `gpu_boids` among them.
- `crates/henad-graph/src/tests/engine_runs.rs` runs a counter past 2^32 through the engine, a crowd in one cell, a press, and a stats readback that carries the measurement.
  Its last test shows that the submission sizes change no result.

#### Whole numbers, percent values, clock columns and the resume note

- `ParamDescriptor::integer` marks an `f32` parameter as taking whole numbers only.
  A sheet's Whole number with a negative minimum lowers to such a parameter, with whole `min`, `max`, `default` and `step`.
  A Whole number from 0 up stays a `u32`, as before.
  A graph file's or grid rule's `[[param]]` takes `integer = true` on an `f32`, and `--params`, `--params --json` and the manifest's schema mark it.
- `parse_param` and `check_param` read and check a value against its whole descriptor.
  `--set`, `--vary` levels, spec files, design tables, `RunSetup::set` and `set_text`, `Simulation::set_param` and the Sweep tab use them, and reject a fraction for an integer parameter.
  A range takes whole ends and steps, and Latin hypercube designs, random designs and searches draw whole numbers.
  The Parameters tab's slider, the Sheet pane's slider and the Sweep tab's fills step by whole numbers.
- A parameter in the percent format takes "2.5%" wherever it takes 0.025: `--set`, `--vary` lists and ranges such as `1%:5%:1%`, spec files and the Sweep tab.
  `runs.csv`, the manifest's `spec` and a search's hash hold the fractions.
  A `%` on another number is rejected, naming the parameter.
- Debug builds panic in `StatColumns::plan` on a stat column named `run_id` or `tick`, and in `StatsWriter` and `series.csv` on one named `time` where they write a `time` column, naming the stat.
  The assertions cover compiled models.
  The checkers reject such names in model files first.
- A resume rejected over changed code of a model from `--model-file` opens with "model file '<path>' holds different code from the runs in the directory".
  A rejected merge names the model files each shard's command line gave.
  The library's messages follow unchanged.

#### The `url` advisory

RUSTSEC-2026-0592 appeared against `url` 2.5.8 during the work, and a full check at `85c9e948` found it.
`4b06fcc2` moves `Cargo.lock` to `url` 2.5.9 and `form_urlencoded` 1.2.3, and regenerates `docs/license.html`.

#### Editor behaviour

- Apply asks "Replace applied model?" before it replaces a model applied from a sheet of another name under the same id.
  The question reads, for example, "Replace the applied model Chance SIR? It has the same id as this model, sir."
  Nothing changes before Replace.
  A sheet of the same name, and a rename of the sheet the Editor applied last, replace the model without asking.
- Each Editor save carries a number, in `SaveTarget::Model(u64)` and `SaveTarget::Counterpart(u64)`.
  Each save's report names and marks the model it saved, also with two saves pending under one target, and a save of a replaced model names no model.
- An action waiting on the unsaved changes dialog's save, such as New model or closing the window, goes with its model when another action replaces the model first.
  Before, the save's report ran it over the new model.
  The fault predates this work.
- Deleting a parameter, a property or a line, or removing a summary of a look at agents, asks first when it would empty a formula the card cannot write again.
  The question names the lines and measures, as in "Trail falloff is used by lines 3 and 4. Delete it?".
  A delete the card can rebuild runs at once.
  `sheet::edit::lines_losing_formulas` and `measures_losing_formulas` find the formulas, and `docs/guide/editor.md` states the exception.
- When Build or Rebuild rejects the build after an Apply, such as for a Seed field with no seed, the Apply line takes an outcome of its own, `ApplyOutcome::BuildRefused`.
  The line names the reason Build or Rebuild is disabled, and once the reason is gone it says what Build or Rebuild will do.
  Before, "Model build failed. Fix the problem below." stayed until a build ran the applied code.
- A parameter id equal to one of the 14 fixed columns of `runs.csv`, such as `seed` or `status`, is an error in the graph checker and the sheet checker.
  `AddParameter` mints past them, Seed becoming `seed_2`, and the Sheet pane never mints one.
- A picker or menu draws one rule under its title, where it drew two.
- A test of `crates/henad-graph/src/tests/sheet_summaries.rs` is named for what it checks.
- `agents/henad-app-sheet.md` is split, and the new `agents/henad-app-sheet-open.md` holds opening a sheet, Apply of a sheet, its preview and the New model dialog.

#### The changelog

`f3851adb` folds the `[Unreleased]` section of `CHANGELOG.md` as one change against 0.3.0, and a change to a feature new since 0.3.0 changes that feature's line.
The "App theme" line goes, since the theme is back at the values of 0.3.0.

#### The guide's screenshots

The screenshots were rendered offscreen from the app's own UI code at 2 pixels per point.
19 of the 30 under `docs/assets/app/` were replaced, `viewport-toolbar.png` last in `d95771ad`.
11 were kept where only noise differed, such as another random draw or a widget moved by a pixel.
The overview's caption reads about 40,000 ticks per second, where it read 30,000.

### Results that changed on purpose

#### The snapshot

The snapshot runs 39 models and model files at a fixed seed for 64 steps, two sweeps and six error runs through `henad-cli`.
At `13205c3c` it was compared with the snapshot at `b525bb2f`, the tip of record #108's work.
Every deterministic model's statistics, parameter schemas and lowered sheets are identical.
The five runs that never replay exactly, stock `gpu_boids`, the boids graph fixture and starter, and the boids and bounded confidence sheets, differ loosely as always.

The 15 strict differences:

- The hash of `Cargo.lock`, for `url` 2.5.9.
- `library.lock`: the semantics and cases hashes of `counter.add` and `stat.counter`.
- Eight shaders of the ants graph fixture, the ants sheet and the ants graph starter, with their counter words, workgroup tally and carry.
- Five new largest-bucket shaders, of the boids graph starter, the boids sheet, the bounded confidence sheet, the `gpu_boids` graph fixture and the `mobile_sir` test sheet.

#### Input accepted before and rejected now

- A sheet measure or a graph `[[stat]]` named `tick`, `time` or `run_id`.
- A grid rule `[[state]]` labelled `tick` or `run_id`.
- A sheet or graph parameter whose id is a fixed column of `runs.csv`.
- A value with a fraction for a Whole number with a negative minimum, from `--set`, a listed level or a design table.
  Its runs reported ok before.

A sheet with a Whole number whose minimum is negative takes a new schema hash and code hash, and a results folder written before will not resume.
No shipped sheet has one.
A compiled model with a stat named `run_id` or `tick`, or `time` with a time scale, panics in a debug build.

#### What a user meets

- egui's own frames, shadows and focus outline, help on hover, faded solid outlines, "…" and "→" in the Editor, and unframed Sheet pane sections.
- In the Editor layout, the Editor on 70 percent of the width and the Model tab beside Playback.
- The dialogs Replace applied model?, Delete parameter?, Delete property?, Delete line? and Remove summary?.
- An Apply line after a rejected build that follows the reason.
- A share kept outside 0 to 1 accepted with a warning.
  Record #108 calls it an error in two places.
  That was true when the record was written, and the record stays as it is.
- Percent values typed as "2.5%", and whole-number sliders and fills for an integer parameter.
- Counters past 2^32 that read their whole total.
- A graph of agents with a radius fold sends one step alone after a build or a press, and a crowded run takes smaller submissions with the same results.
- A saved sheet's comments write "…" and "→" where they wrote "..." and "->", and a comment line of the ants sheet changed.
  A sheet's comments are author text outside the code hash, and no code hash or results folder changes for them.

### How it was checked

#### Each package

The briefs required each package to run the tests of every crate it touched with `HENAD_REQUIRE_GPU=1`, clippy with `-D warnings`, rustfmt, the wasm32 typecheck of each crate with web support, `cargo doc` with `-D warnings` where a string or a doc comment changed, and the snapshot against its branch point.
The snapshot at `13205c3c` above covers every package that changed code.
The GPU package timed its models before and after: the boids sheet's 200 steps took 603 ms before and 605 ms after, ants at 1M agents moved by less than 1 percent, and no timing moved by more than about 3 percent.

#### The full check

The full check ran every step of `./check.sh`.
A run at `85c9e948` found the `url` advisory, fixed in `4b06fcc2`.
At `13205c3c`, after every package that changed code, it passed 16 of 16 steps with 2,452 tests.
The four commits after it change `CHANGELOG.md`, the guide's screenshots and one caption.

### Edited tree

```
AGENTS.md                                      the row of the new sheet topic file
CHANGELOG.md                                   [Unreleased] folded against 0.3.0, the App theme line removed
Cargo.lock                                     url 2.5.9, form_urlencoded 1.2.3
agents/
├── henad-app-sheet-open.md                    new, from henad-app-sheet.md: opening a sheet, Apply, preview, New model
├── ui.md                                      egui's frames and theme, labels, help on hover, faded outlines, symbols
└── ...                                        the topic file of every area changed, 21 files in all
crates/
├── henad-app/src/
│   ├── init.rs                                the theme values of 0.3.0, the semibold face kept
│   ├── state/files.rs, install.rs             numbered saves, integer parameters at install
│   ├── tests/                                 a test of each change, in the existing files
│   └── ui/
│       ├── dock.rs                            the Editor on 70%, Model beside Playback and Pacing
│       ├── editor/mod.rs, expert.rs           Replace applied model?, BuildRefused, numbered saves, waiting actions
│       ├── editor/graph/                      a converted copy's zoom, whole-number fields, egui's frames
│       ├── editor/sheet/
│       │   ├── delete_question.rs             new: the question before a delete empties formulas
│       │   ├── widgets.rs                     egui's frames, no bars, faded outlines, stock buttons, one title rule
│       │   ├── help.rs                        help as a tooltip
│       │   ├── card_pane.rs, picker.rs, ...   labels, symbols, stock controls
│       │   └── sheet_pane.rs, sheet_pane/     unframed sections, wrapped state names, whole-number sliders
│       ├── files/mod.rs                       numbered saves
│       ├── params/values.rs                   whole-number sliders
│       └── sweep/                             percent values, whole-number fills
├── henad-cli/                                 percent values, integer marks, the model file in a rejected resume or merge
├── henad-compute/
│   ├── build.rs                               the largest-bucket shader
│   └── src/gpu/
│       ├── primitives/hash_largest.wgsl       new: the largest bucket of the spatial index
│       ├── primitives/spatial_hash.rs         LargestBucket
│       ├── agent_engine.rs, agent_spec.rs     measured submission sizes
│       ├── capacity.rs, sim_thread.rs, ...    submission_steps
│       └── runtime_grid/checks.rs             rule states labelled tick or run_id
├── henad-core/src/
│   ├── params.rs                              ParamDescriptor::integer
│   ├── explore/                               parse_param, check_param, percent levels, whole-number designs
│   └── export/stats_csv.rs                    debug assertions on clock column names
├── henad-explore/src/
│   ├── sweep/percent.rs                       new: percent levels written as fractions
│   └── output/, schema.rs, spec_file.rs, ...  series.csv assertion, integer schemas, manifests, search hashes
├── henad-graph/
│   ├── library.lock                           counter.add and stat.counter
│   ├── sheets/ants.toml                       → in a comment
│   └── src/
│       ├── catalogue/, host/                  64-bit counters
│       ├── check/                             reserved stat labels and runs.csv columns
│       ├── model_text.rs                      integer = true
│       ├── sheet/check/                       clashing names, any and all, kept shares, add-up warnings
│       ├── sheet/edit/formulas.rs             new: lines and measures that lose formulas
│       ├── sheet/edit/                        reserved parameter ids, names with …
│       ├── sheet/lower/world.rs               whole numbers with a negative minimum marked integer
│       ├── sheet/sentence/                    new messages and dialogs, …, →, no help popover
│       ├── wgsl/                              counter words, carry, tally
│       └── tests/engine_runs.rs               new: a counter past 2^32, a crowd in one cell, submission sizes
└── henad-models/                              stock gpu_boids keeps 64 steps, submitters keep the cap
docs/
├── assets/app/                                19 of 30 screenshots retaken
├── authoring/parameters.md                    whole numbers and percent values
├── developing/
│   ├── agent-record/
│   │   └── 20261122-109-editor-look-decisions-and-fold.md   this record
│   ├── architecture.md, gpu-backend.md        measured submission sizes
│   └── contributing.md                        the AI usage section, by the maintainer
├── guide/app.md, editor.md, sweeps.md         the look, the new dialogs, percent values, whole numbers, the caption
├── license.html                               url 2.5.9
└── reference/cli.md, model-files.md           integer parameters, percent values, reserved names, kept shares
zensical.toml                                  nav entry for this record
```

The 80 commits add six files and remove none.

## State after

- `37-node-editor` at `d95771ad` holds 80 commits after `6141c2e3` and 932 after `4f1ea055`, and this record follows them.
- `f309fd29` carries the maintainer's signature, and the 79 commits of the packages are unsigned.
- The remote's `37-node-editor` is at `6141c2e3`.
  Nothing after it was pushed before the fold.
- The full check passed 16 of 16 steps at `13205c3c` with 2,452 tests, and the snapshot there keeps every deterministic model's statistics identical to the snapshot at `b525bb2f`.
- This record changes only the record and its nav entry, and `uv run zensical build` builds the site with it.

### The fold of the history

The history of the branch is folded after this record is committed, as the maintainer asked.

- The 933 commits since master's `4f1ea055`, the 932 above and this record, become 98 commits.
  Each holds the work of one milestone, session or package under a new one-line message.
- One more commit after them replaces each hash that a record or another tracked file cites with the hash of the commit that holds that work.
  Several citations in records #61 to #108, and in this record, then name the same commit.
- The final tree is unchanged.
- Every new commit is signed.
- The history before the fold stays under the local ref `refs/backup/37-node-editor/20261122-pre-fold`.
- The branch is force-pushed, as the maintainer allowed.

The counts of commits, the hashes and the signing states in this record describe the history before the fold.

## Issues found & future directions

### Left, with the options for each

- **Parameter ids beside the fixed columns of other tables.**
  A search's `evaluations.csv`, `best.csv` and `archive.csv`, and a sweep's `summary.csv`, write parameter ids beside fixed columns that no checker reserves: `candidate_id`, `batch`, `failed`, `objective`, `x` and `y`, and `runs`, `ok`, `failed` and `ticks:mean`.
  A grid rule's and a compiled model's parameter ids are not checked against the columns of `runs.csv` either.
  (a) Reserve those columns too.
  Common ids such as `x` and `y` would then be rejected.
  (b) Write the parameter columns of those tables under a prefix, a change of format.
  (c) Check grid rules in `GridRule::check_declarations`, and compiled models with debug assertions, as for stat columns.
- **Edits that empty formulas without a delete.**
  Changing a property's kind (`SetPropertyType`) or deleting a state in the property form empties formulas, or drops a branch of a Depending on, without asking.
  The question before a delete could cover both edits.
- **Marks below the resolution of Add up.**
  A mark below 2^-17 on a ground property that adds its marks up adds nothing, with no warning.
  (a) Warn at a fixed amount below the resolution.
  (b) A resolution per sheet, chosen from the largest population and the largest amount.
- **A share below 0.**
  Keep the largest lifts a negative cell to 0 at the next merge, and a worked-out deposit of 0 or less issues no atomic.
  The warning states neither effect, and its words could name both.
- **The largest bucket without stats samples.**
  `run_steps` and the CLI bench without stats keep the size measured after the first step, and agents that gather later go unmeasured.
  Between measurements a stale size can last up to 64 ticks plus the readback's delay.
  In a browser, the first batch after a build or a press of a model with a fold goes one step per submission.
  Measuring every few submissions without a stats readback would close the first case.
- **Frame all on the ants copy.**
  The tick pass of the ants starter's converted copy fits only at a zoom of about 0.085, below the canvas's minimum of 0.1.
  (a) Lower the minimum.
  (b) Let Frame all stop at the minimum and show the pass from its top left corner, as a converted copy opens.
- **The Model tab beside Playback.**
  At 1280 by 800 its content scrolls in a stack about 140 points tall.
  A focus request to the Model tab, as from the Sweep tab's model name, hides Playback.
- **The Charts x axis.**
  In the Editor layout at 1440 points wide, the right column is about 430 points, and the x axis shows only its first label, "0".
  egui_plot hides x labels closer than 60 points.
- **Percent values and whole numbers in the app.**
  The Sweep tab's hint and examples for a percent parameter still read as fractions, "0:1", while its baseline shows "1.0%".
  The Sheet pane does not say why a typed fraction for a Whole number is rejected, and the lowering's message reads "holds a number its type does not support".
  The graph editor's World table rounds an integer `f32` with no test of its own.
  `with_fractions` reads " 1%", with spaces, as 0.01, where `--set` rejects it.
- **Saves and swaps.**
  `AfterSave.remaining` is positional, and a swap with the converted copy while the dialog's save is pending can save one model twice.
  Nothing is lost, and no test covers it.
  A swap clears the applied mark, and renaming the applied sheet after a swap to its copy and back asks to replace its own applied model.
  "Model build failed." also covers a build that never ran.
  "Seed" shows in sheet mode's status area with a bad seed, as before this work.
- **Kept as they were.**
  The card's blue 2 point focus mark in its gutter, the collapsing headers of the graph editor's World panel, the Diagram boxes' gray 600 stroke, and the canvas nodes' window frame without a shadow.
- **The screenshots.**
  The old files had been cut to about 252 colours, and the new ones are lossless.
  12 of the 19 new files are 2 to 2.7 times the size of the old ones.
  The View menu's button shows no highlight while its menu is open in the offscreen render, and the fault may lie in the offscreen render alone.
- **Topic files.**
  20 topic files hold lines past 100 columns.
  Most predate this work, and `henad-explore.md`, `henad-graph.md` and `ui.md` gained four of them.

### Not checked on a screen, in a browser or on other hardware

- **The live app.**
  No change of this work was seen in the running app, and the session judged the offscreen renders alone.
- **The browser.**
  The single steps of a fold model after a build or a press never ran in a browser.
- **Lavapipe and CI.**
  The GPU package's lavapipe timings are estimates, since the Docker daemon was not running, and its crowd test was shrunk to stay well under 1 s.
  CI never ran on the commits of this work before the fold.
- **Device tests on another GPU.**
  The packages ran their device tests on the machine of this work only.

### Process

- **Signing.**
  The packages' commits are unsigned, and the fold signs every commit it writes.
- **Commit messages.**
  The 79 subjects average 4.9 words against the 4 the maintainer requested, and none passes six.
  The fold writes new messages.
- **The release** stays with the maintainer.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
