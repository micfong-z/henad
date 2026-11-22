---
date: 2026-11-22
title: "Strings and audit fixes"
description: "The maintainer requested every string a user reads in standard UI wording, and an audit of the branch with its findings fixed. Six research reports fed a style guide, and about 4,500 strings passed through several generators, a blind judge, a consistency pass and the maintainer's own picks, before six packages applied 898 picks in 29 commits. The audit confirmed 80 findings and checked 421 plan items, and nine fix packages fixed 73 of the findings in whole or in part, and 12 of the 14 plan items they took up. 166 one-line commits, 26 of them signed, none pushed. A snapshot of every model's output at the tip keeps every deterministic model's statistics identical to the baseline."
icon: material/comment-text-outline
status: ai-generated
model: "Claude Code. Fable generated string candidates in 20 of 37 batches until the maintainer asked to stop, and every other agent ran on Opus, Sonnet or Haiku. The fix packages' implementers and reviewers and this record's draft were Opus subagents"
issue: "#37"
state: "37-node-editor at b525bb2f and this record: 166 one-line commits after 68447db5 and 829 after e816b444, nothing pushed. 26 commits of the string work are signed and the rest unsigned. The isolated full check passed at b7804e51 with 2,276 tests, at 82f6ca1c with 2,360 and at b525bb2f with 2,385. The snapshot at b525bb2f keeps every deterministic model's statistics identical to the baseline"
baseline_commit: 68447db5
delta_state: "committed on `37-node-editor`: 29 commits of the string work, 137 of nine fix packages, and this record"
---

# Strings and audit fixes

> The maintainer made two requests: every string a user reads, rewritten in standard UI wording, and an audit of the branch followed by fixes for what it found.
> The work ran from 2026-11-20 to 2026-11-22 from `68447db5`.
> Six research reports and 337 pairs of the maintainer's own earlier edits fed a style guide.
> About 4,500 strings and card sentences passed through several generators, a blind judge and a consistency pass, and the maintainer chose among the options on a review page.
> Six packages applied the 898 picks, 892 of them the judge's defaults and 6 the maintainer's own choices, in 29 commits.
> The audit's 15 finders and their verifiers confirmed 80 findings and checked 421 items of the redesign's plans.
> Nine fix packages, each reviewed before its merge, fixed 73 of the findings in whole or in part, and 12 of the 14 plan items they took up.
> The isolated full check passed at the end of the string work with 2,276 tests, after the first seven fix packages with 2,360, and at the tip with 2,385.
> A snapshot of every model's output at the tip keeps every deterministic model's statistics identical to the baseline, and each of its 44 other differences has a cause.
> The branch holds 166 new one-line commits, 26 of them signed by a rebase, and nothing was pushed.

## State before

`37-node-editor` at `68447db5` held 663 unsigned commits after `e816b444`, the remote's head of the branch.
They ended with record #107, the commit that remapped the hashes the records cite after the rewrite of the commit messages, and the record of a check in a browser.
At record #107 the final full check passed 16 of 16 steps with 2,276 tests, and the snapshot of every model's output was identical to its baseline outside the five runs that do not replay.
The maintainer had edited a set of the app's strings by hand on 2026-09-29, in the parameter sweep work of #14.
Record #105 held the last audit of the redesign, over records #71 to #104.

## What was done

### The requests

The maintainer made two requests.
They are paraphrased here, since the sources of this record do not quote them.

1. Rewrite every user-facing string in standard UI wording.
2. Audit the branch, then fix the audit's findings.

### How the work ran

The string work came first: research, a style guide, candidate rewrites, a judge, a consistency pass, the maintainer's picks, and six packages that applied the picks.
The audit ran beside it and only reported.
Nine fix packages then worked from the audit's report.

Each package ran in its own git worktree on its own branch from `37-node-editor`, from a brief the session wrote, and the session merged the branch after reading the package's summary.
Every fix package had an Opus implementer and then an Opus reviewer, who checked each item against its finding, fixed what it found in commits of its own and ran the checks again.
Each package ran the tests of every crate it touched with `HENAD_REQUIRE_GPU=1`, clippy, rustfmt, the wasm32 typecheck, `cargo doc` where a string or a doc comment changed, and the snapshot of every model's output.
The briefs required each agent to build into a target directory inside its own worktree.

No virtual display was connected, and the briefs forbade launching the app on screen.
A change to the UI was rendered offscreen by a scratch test module that runs the app's own UI code, and the module was never committed.

All 166 commit subjects keep to the six-word limit, and they average 5.12 words against the 4 the maintainer requested.

### The strings

#### Research

Six reports, each on one question, were written first.

- The official style guides: Microsoft, Apple, Material Design, GNOME, KDE, Nielsen Norman Group, Atlassian, Shopify Polaris, IBM Carbon and Mailchimp.
- The string files of nine mature desktop applications, among them GIMP, Blender, Qt Creator, Audacity and Nautilus, at tags dated April 2021 to September 2022.
- The strings of 13 scientific, modelling and block-language tools.
- The sentence shapes and word choices that mark text as machine-written, restated for labels, tooltips, errors and help.
- Error messages and command-line wording.
- The standard terms of NetLogo, Mesa, Repast Simphony, GAMA, AnyLogic, Vensim, Stella and epidemiological modelling, and of the desktop toolkits Qt, GTK and LibreOffice.

Every project string came from a file fetched at a tag or commit dated before November 2022, before machine-written text was common.
Fifteen quotes of each report that leans on quotes were fetched again, and all matched.
A seventh file held 337 pairs of agent-written text and the maintainer's edit of it from 2026-09-29, read as the target taste.

#### The style guide

A synthesis of the reports was critiqued by a second agent and revised.
Its quick rules keep a string that already reads as standard UI text, keep every mark (placeholders, non-breaking spaces, line breaks, edge spaces, quote marks), keep every fact, and name the value, control, file, line or action at hand.
A string tells the user what to do with an imperative and the control's exact label.
In app text a control is never the subject of a verb.
The verbs are Press for a button, Select for an option and Enter for a value, and in sheet mode Choose in a picker, Type in a field and Turn on for a checkbox.

Where the sources disagreed, the guide made a house choice that the maintainer can overturn.

- The maintainer's own edits outrank the published guides: no contractions, "failed" and "cannot", "Press" for a button, the future tense for a consequence, no "you" or "we", and no ellipsis on progress text.
- A tooltip of one phrase takes no final stop.
- "chance" in the sheet's words, node names and the node library's help, and "probability" in the Sweep and Results tabs and the CLI.
- Diagnostics start in lowercase and take no final stop, as rustc, git and the Rust API Guidelines write them.
- Each string keeps its own spelling.
  `AGENTS.md` requires American spelling in string literals, and the sheets ship "neighbours" and "colour".
- The rate note keeps its formula.
- The card notes write "independent trial", "converted to a chance by" and "the line nearest the top of the card".
- The checkbox verb stays open: "select" outside sheet mode, and "Turn on" in it, since "tick" is the sheet's unit of time.

#### Candidates, the judge and the consistency pass

A script extracted 4,685 distinct string literals.
Sonnet agents annotated each one: whether a user reads it, its kind, where it shows, its placeholders and its constraints.
An Opus check of 160 exclusions found two recurring mistakes.
Both were corrected.

The strings a user reads went into 29 batches of 3,993 strings, six batches of card sentences as the sentence tests render them, and one batch of 9 clauses that several card notes share.
261 pieces that the sentence builders join, and 431 literals no user reads, were listed and not rewritten.
The web build's loading page and 53 builder words that no rendered sentence shows took one more batch.

For each batch several generators wrote candidates, and scripts checked that every candidate kept its placeholders, non-breaking spaces and line breaks.
The candidates were then blinded, and an Opus judge picked one for each string or kept the current text.
Fable generated in 20 of the 37 batches, until the maintainer asked to stop using it, and the rest ran on Opus, Sonnet and Haiku.
The shared clauses took two wider wordings and a second judgement.
Two Opus consistency reviewers then read the picks across batches and made 47 switches, to make one term or one form read the same everywhere.

The final set held 4,043 strings and 466 card sentences and shared clauses.
The judge's defaults rewrote 897 of them and kept 3,612.
Every option kept its placeholders, non-breaking spaces and line breaks, no fragment lost an edge space or changed the case of its first letter, and the 73 changed defaults with two or more anonymous `{}` placeholders kept their order, checked by hand against their arguments.

#### The maintainer's picks

A review page outside the repository showed each string with its options and the default, and saved the maintainer's choices.
Six choices differ from the default: five replace the judge's rewrite with another option, and one rewrites a string the judge kept.
The export holds 898 changes, 854 strings and 44 card sentences and shared clauses: 892 defaults of the judge and 6 choices of the maintainer.
Two of the maintainer's own choices:

- "one tick must last more than 0 units", where the default read "one tick must last a positive number of units".
- "the kind of agent '{id}' cannot be removed, as it is the world's only kind of agent", where the string read "the kind of agent '{id}' is the world's only one and stays".

#### Applying the picks

Six packages applied the picks, in merge order.

| Package | Picks | Commits |
| --- | --- | --- |
| Sheet text: the card law, rate notes, card notes, Sheet pane help, the conversion dialog, Compare runs and the starters' purposes | 120 | 6, `7b3c0fa2` to `5cfd00be` |
| Sheet diagnostics: the checker, reader, writer, edit commands, lowering and conversion | 195 | 4, `43fda84a` to `02efb17e` |
| The graph checker | 135 | 3, `e106d6a4` to `7dadd159` |
| The node library's help, the graph's edit commands, reader, host evaluator and WGSL compiler | 176 | 5, `4f86a254` to `4b94f437` |
| henad-build, henad-core, henad-compute, henad-explore with its testing kit, and henad-cli | 163 | 5, `fc610c9e` to `ed443a8b` |
| henad-app and the web page | 109 | 4, `95494a6f` to `b7804e51` |

Two test commits came between the last two: `f4fd6ad8` wraps an assert past 120 columns, and `2d0b19c1` updates the fault phases two app tests build.

Every package applied each chosen text as given, in the source form of its literal, with its placeholders in their order.
Two card sentences of the sheet text package are built by templates of the WGSL compiler, and the node library package changed them there.
Tests that assert a changed text, and the topic files, guide pages and doc comments that quote one, changed with it.
The session records keep the texts of their time.

The families of change:

- The sheet's card law reads "All changes are applied together at the end of the tick", and the Lines help says "the line nearest the top of the card wins".
  Rate notes read "The rate is converted to a chance by 1 − e^(…)", competing lines "split between the lines in proportion to each rate", and a long tick "Use a shorter tick for a more accurate result."
  The line menu reads Convert to a rate, Convert to a chance and Exclude me from line N, and each sheet starter's purpose says what to "Look for".
  The public constant `sentence::RATE_IS_NOT_A_CHANCE` became `sentence::RATE_CONVERSION`.
- Diagnostics write "is not a" for "is no", "expected …" and "got …" for a limit or a mismatch, "must", "cannot" and "can only" for a rule, and "unknown parameter" or "invalid … id" for a name.
- henad-core, henad-compute, henad-explore and henad-cli write "rejects" for "refuses", "failed" for "faulted", and "exceeding the …" for a device or budget shortfall.
  A rejected action's note in `runs.csv` reads `model rejected action 'ID' at tick TICK`, and 14 entries of the CLI help are reworded.
- The node library's help writes "returns" for "gives" and "integer" for "whole number".
  `library.lock` took 26 new semantics hashes, and nothing else in it moved.
- The app writes "less than" and "greater than" in a sweep's stop rule, "Enter values", "Select a CSV file", "Concurrent runs", "Select model", the dialog title "Rebuild model?", and "−1" with a minus sign.

The four golden sheets of henad-graph's reference writer hold the starters' purposes and the ants starter's description of its trail, and the new words went into them by hand.
`5cfd00be` records in the golden files' README that the procedure's three commands, run on a copy of each edited file, return the file unchanged.

The commits of the app package and of the node library package were rejected by the permission system, and their work was left staged.
The session committed it, under the maintainer's choice of local unsigned commits.
The isolated full check then passed 16 of 16 steps at `b7804e51`, with 2,276 tests.

### The audit

The audit read `37-node-editor` at `68447db5` against `e816b444`.
Fifteen finders ran: seven on correctness, two on the user's experience, and six that checked the redesign's plans and design documents item by item against the code.
An adversarial verifier followed each finder, and one writer assembled the report.
The finders were told not to read record #107 or its artifact, to check that work on their own.
One UX finder rendered the Editor offscreen in its own worktree.
The report changed no code.

Of 85 findings reported, 83 were confirmed and 2 refuted, and three pairs of duplicates were merged.
That leaves 80 findings: 8 high, 27 medium and 45 low.
58 are of correctness (8, 19 and 31) and 22 of the user's experience (0, 8 and 14).

The plans held 421 items: 339 done, 48 superseded, 26 partial, 5 missing and 3 unclear.
The verifiers ruled on 91 of them, every partial, missing and unclear item among them, and corrected 13 of the checkers' statuses.
The other 330 rest on the checker alone.
Of the 31 partial or missing items, one was rated high: a Calculate that divided by a zero count wrote infinity or NaN into cell state with no warning, where the sheet's design says "division by 0 gives 0".
Two were rated medium: the Model tab's word on runs that do not replay, and a reference for graph model files with the limits that bound a step.

The five findings the report put first:

1. At the app's default GPU pacing of 30 ticks a second, 59 of 60 snapshots carried the stats of another tick, 1 or 2 ticks old, and every other readback was dropped.
   The Record CSV and the Charts tab of a default GPU play were wrong at every row.
2. A sheet parameter's value changed on the sheet or by Keep, then applied again, ran at the old value, while the Editor read "Model applied."
3. The Sheet pane kept one pending edit per frame, so a field's commit on losing the focus was dropped when another control fired in the same frame.
   A typed purpose was lost, and Undo could not bring it back.
4. Looks at agents in a world with walls counted the agents across the walls, while the Sheet pane said an agent near an edge has fewer agents around it.
5. Converting a sheet a second time dropped the edited copy beside it, with no unsaved changes dialog.

The other high findings: AddLine inside a block that lists itself looped forever on the UI thread, "Add up" marks on the ground wrapped at 65,536 per cell per tick with no warning, and Cmd+Q on macOS quit past the unsaved changes dialog.

### The fixes

The 80 findings went to eight packages by area, and the accessibility package took plan items only.
One more item came from the string work, the messages that read "1 components" for a count of 1, and 14 of the 31 partial or missing plan items went to packages too.
The other 17 partial or missing items went to no package.
Most need people, a screen, an idle machine or the maintainer, and the rest are features or housekeeping that no fix of a fault covers, such as a `henad-cli` diff of graph files.

73 of the 80 findings were fixed in whole or in part, and the 7 left whole are under [Left, with the options for each](#left-with-the-options-for-each).
Of the 14 plan items, 12 were done in whole or in part.
The count takes the 0.4 migration lines as done: they are in `CHANGELOG.md` under "Migrating to 0.4".
It takes as left the toast for notices, and the rule that no item's name contains the name of another item.
`f7a1bed6` tests that rule on two menus only.

No fix implementer wrote its report, and several left their work staged or as patch files.
Their reviewers rebuilt the lists of what was fixed and what was left from the patches, the changelog notes and the snapshots.
Where the permission system rejected a commit, the session committed the reviewed work: `5147d1f3` of the sheet edits package, and `b58ba1b2`, `6f25b042` and `b525bb2f` of the accessibility package.

| Package | Commits | First and last |
| --- | --- | --- |
| Sheet edits and the sheet checker | 20 | `3d38cf9d` to `e7a43a27` |
| Sheet lowering | 15 | `d5becccd` to `a98e4d84` |
| The Editor's panes and the replacement of a model | 16 | `cb54b126` to `3bb216fd` |
| App state | 13 | `4473111d` to `c9d56a8f` |
| henad-explore, henad-cli and the docs | 12 | `64f47f1f` to `fba97bb9` |
| The runners | 10 | `20779bcf` to `0f1737fd` |
| Graph models | 13 | `02a7cfdf` to `82f6ca1c` |
| Editor UX | 27 | `39440ad8` to `893f1064` |
| Accessibility | 11 | `b31710f7` to `b525bb2f` |

The isolated full check passed at `82f6ca1c`, after the first seven packages, with 2,360 tests.

#### Sheet edits and the sheet checker

- `bbde2dc6`: AddLine inside a block that a file lists inside itself, or inside two blocks that list each other, returns.
  The walk up the blocks stops after as many blocks as the card has lines.
- `8f60e55e`: an edit is rejected when its text would nest past the reader's 24 levels, with line tables counted at depth 5, or pass the sheet size limit.
  Every accepted edit reads back.
- `3d38cf9d`: renaming a kind of agent is rejected when its tick card id would pass 48 bytes.
- `95529378`: a world past 16,384 cells a side, or 10,000 for agents, and a population past 10,000,000 are errors at `[world]` and `[[agents]]` in the sheet's words, quoting the file's integer.
  They were errors at `[sheet]` in the graph's words.
- `831270b1` and `8116f587`: the checker checks the value types of Set, Keep between and comparisons, at the line.
  The review's `8116f587` lets an Arrow property take a place.
  The first version rejected it, though the lowering builds it.
- `819e299e` and `59a083c7`: a missing value of a Depending on skips the line, a value keyed by no state is an error at the line, and a property with no states reports one error.
- `0284a719`: a Calculate past 16 values in one chain of operations is an error at its line, "Group some of them in parentheses".
- `cbbe340d`: a delete that empties a one of or none of list blanks its slot, and a hand-written `[]` reads as a blank.
  A `none of []` line no longer fires every tick.
- `5147d1f3`: a delete that empties a formula inside a slot, such as Trail falloff in the ants' Leave lines, blanks the slot for the card's picker.
- `6a861dcd`, `44b34018` and `dbc52450`: a line that reads its own result gets its own note, "Uses its own result. Choose another value.", and its chip takes a tooltip of its own.
  Notes name an unnamed result as its chip does: an unnamed Count by its automatic name, any other by its line.
  The review found the implementer's version still quoting a whole sentence, "Uses line 4 is yes, which is below this line".
- `eaee283b`: Blank reads "An empty model to build on." in both worlds, and `54ac7aa3` adds an article to the skip notes of a select, "Not run yet: choose a unit of time".

#### Sheet lowering

- `a9807a6d`: a division in a Calculate whose divisor is 0 returns 0, per component for an arrow over an arrow.
- `3c08005e`: in a world of agents with walls, a look at agents sees no one across an edge, and its offset and distance to them do not wrap.
- `812adc36` and `c44ed383`: a look at agents that reads an earlier look, through other lines or through the When around a line it reads, takes a later fold of its own.
  Such a sheet builds, where it failed with a node cycle before.
  The review found the When case missing from the first fix.
- `6584aec7`: a competing group of rates of parameters and numbers, with no "for each", keeps each share to 2^-32, through `rng.threshold` and one `rand.bits` draw.
- `f531b734` and `93482704`: a Steer towards a value that a Calculate works out from an average, largest or smallest of no one adds nothing.
  The first fix also stopped a Choose with an explicit fallback, such as "average when count > 0, otherwise 0.5", from steering, and the review narrowed it.
- `ea5e5c3a`: the checker warns at a Leave of a number or a parameter on a ground property that adds its marks up, when the kind's largest population on one cell can leave 65,536 or more in a tick.
  The docs state the 2^-16 rounding and the wrap.
- `d8c2d7e4`: a Move to below another Move to that can apply to the same agent reads "otherwise (when line N does not apply)".
- `6139e37d`: a rate note under an "otherwise" line ends "This chance holds only when line N does not apply.", and the mean stay is left out where another Become of the property leaves the same state.
- `d0884b19`: a note under a Steer towards inside a Repeat says that each time through the Repeat steers from the value at the start of the tick, and the steers add up.
- `c319cedf`: a Steer's value takes the type of the property it steers, and a largest or smallest summary's value must be a number, both checked at the line.
- `d5becccd`: a rate worked out per element with no "for each" takes the chance 0 at a hazard of 0 on every device.

#### The Editor's panes and the replacement of a model

- `cb54b126`: a sheet field left by a click on another control commits in the same frame as that control, and both edits are kept as one Undo step each.
- `853d4ee5`: a parameter slider that only takes the keyboard focus edits nothing.
- `e42ea147`: Escape in a one-line sheet field puts back the sheet's value.
- `50e4e467` and `53dd3acc`: the share of the ground kept each tick stays from 0 to 1 when typed in its chip, and a share outside 0 to 1, as a number or through its parameter's range, is an error.
  The review added the error on finding two paths the first fix left open.
- `4615287a` and `0aeef831`: the state picker of a Become and of a count or look at agents on an agent card offers the agent's own states.
  The first fix dropped the ground's states from a count over the cells around, and the review's `0aeef831` restores them.
- `fbf4810d`: two state names swapped, or renamed in a chain, in the property form are kept.
- `d13bcb1b`: a node graph copy whose sheet beside it holds an error reads as changed since the conversion, and its Open button opens that sheet.
- `2e151fda`: converting a sheet again asks before it drops an edited copy beside it.
- `c7094ec7` and `f19175c2`: a model edited while the Open model dialog is open asks before the picked file replaces it, and a save that reports after its model was replaced names no model.
- `0a806b30` and `74a8b840`: Apply builds nothing while the Seed field holds no seed, and a run to a tick behind the current one that Build rejects leaves the run where it is.
  The review's `74a8b840` stops the run to that tick from asking the runner again on every snapshot.

#### App state

- `4473111d`: at the install of replaced model code, a value still at the old default takes the new default, and any other value is carried by id.
- `c95cdfec`: a replacement whose parameters, stats or actions changed keeps the Sweep tab's draft, each row and each scheduled action by id.
- `a8a198b2`: Apply over a run whose Rebuild or Build is disabled reads "Model build failed. Fix the problem below."
- `4e955aa4`: Save recording names the file after the model recorded and the tick the recording stopped at.
- `140c3c37`: Run beside a paused node graph preview says to press Apply in the Editor.
- `c7fc3954`: model names that differ only in letter case count as the same name, and each shows its id.
- `cf042a8a`: the Model tab's Identity rows gain Exact replay, with a cross and "Two runs with the same seed can differ." for a model that does not replay exactly.
- `c1668c18`: in a browser, closing or reloading the tab over unsaved Editor edits makes the browser ask before it leaves the page.
- `1abb36c0` and `c1e401ae`: a playing run, or a run to a tick, pauses while a modal dialog is open and goes on once it closes.
- `cdc20f40`: Cmd+Q on macOS asks about unsaved Editor edits, as closing the window does.
  The app menu's Quit item, which quit past the dialog, is removed through objc2-app-kit, at versions the lock already held.

#### henad-explore, henad-cli and the docs

- `64f47f1f`: a search candidate the device cannot hold is never built.
  Its runs are recorded `refused`, with the reasons in the note in the sheet's words.
  Before, the candidate was built and ended `panicked`, with pass names and a Rust source path in its note.
  `henad_explore::probe::ShortfallWords` words the reasons, and `f9b5c360` re-exports it from the facade.
- `e5f2433c`: `--export` or `--export-stats` naming one of the `--model-file` paths is rejected before anything runs.
  Before, the export overwrote the sheet.
- `47f4f4db`: a `note:` on stderr names each model file whose model the command does not run.
- `63d2f4f9` and `456f9e31`: a resume with no author text but the recorded code keeps the recorded sheet text, and a resume refusal reads "cannot resume the runs in the directory".
- `2a47d824`, `41544555`, `5d12b4ce`, `eea60a99` and `fba97bb9`: the CLI reference on exports of model files, the Edit model line of the Editor guide, the private doc links that `cargo doc --document-private-items` reported, and a section "Limits of a graph model" in the model file reference.

#### The runners

- `fdef518c` and `ce295214`: a GPU snapshot goes out once its stats readback has landed, and its tick, stats, fire counts and display agree, capped or uncapped.
  A capped batch that holds a whole 16 ms interval always ends in a snapshot, and at 1 to 60 ticks a second every tick is published.
  The adaptive batch size of a capped run times each batch to the end of its work.
- `20779bcf`: the CPU runner's run-to samples prepare the view first, as a publish does.
  Before, the component stats of `team_assembly` in the Charts tab were stale.
- `e96a7b47` and `ab771024`: a resume, and a merge of shards, reject a folder whose manifest records another time unit or tick-length parameter.
  The schema hash leaves both out.
- `8e8d5b28`: the refusal names a "tick-length parameter".

#### Graph models

- `02a7cfdf` and `06b773df`: messages with a count of 1 read "1 byte", "1 component", "1 argument", "1 error" and "1 value".
- `2cab6396`: an integer division or remainder by a constant 0 builds and returns the documented result, where naga rejected it as a fault of the compiler.
- `a4a0fcc7` and `31e565bd`: a field's `finish` is checked as the update runs it, its cost included, and each fault is reported at the file's tables.
  The update calls `finish` with a key of the field's own, and two fields that share a `finish` with a draw now draw different numbers.
- `0887b471` and `362ba322`: a `carry` accumulator of a cell, a layer or an agent builds.
- `5527be4c`: `space.delta` and `space.dist_sq` on the GPU reduce a difference between points more than a world and a half apart as the host does.
  `agent.radius_fold` still corrects a difference once on the GPU, and its semantics now say so.
  The review's `dc1f795d` words the agents it misses as a world and a half or more away on an axis, where the first text said more than a world.
- `e46fb633`: a chance that is a parameter or a typed number, with no count, draws through a 32-bit threshold, and a chance of 1 or more always holds.
- `c95926a7`: every node of every graph starter has a place in the `[editor]` region, and no node lies over another.

#### Editor UX

- `39440ad8`: the add menus of the Card pane and a block's "+ Add inside" menu scroll inside the window.
- `426ab077`: inside a Repeat the "+ Add inside" menu shows Count, Look around and Pick a cell greyed out, with the reason as the tooltip.
- `afca0840` and `ca149b42`: a start's chance offers no rate parameter, and the parameter pickers of both panes leave out a parameter whose range the check rejects for that use.
- `57678205`: a state picker with no state to offer names the section to add one under, Agents, Agents or Ground, or Cells.
- `e608a064` and `c73a9495`: text that is no number in a Size field reads "Type a size from 1 to 16384 cells.", and a tick length of 0 or less is rejected with a message.
- `bbbb7035`: a changed Starts as reads "Press Build in the Playback tab to see the change.", naming a control that exists.
- `d98c9d79` and `3251ec11`: the New model dialog's list scrolls in a short window, and a long model name ends in an ellipsis with its full name as the tooltip.
- `d10fa5ed` and `7edaaeae`: the preview line past tick 0 drops "Press Play", and while the sheet or graph holds an error, it warns that the world shows the model before the last change.
  The review took "your" out of both warnings, as the style guide asks.
- `f3c21199` and `5dd742e7`: the Sweep tab warns while the Editor holds a model no Apply has installed and the swept model was not built from it.
- `6d826566`: Frame all, and a new canvas, fit a graph down to zoom 0.1.

#### Accessibility

- `a6a014e2`: the pickers and menus of the sheet editor read as menus: a dialog named by its title, the title a heading, and each item a menu item in a group labelled by its heading.
- `b31710f7`: the New model, New parameter, Rebuild model, conversion and unsaved changes dialogs are marked modal, with their title as a heading.
- `364e1467`, `c8f7b640` and `b58ba1b2`: ↓ and ↑ move through an open overlay's items and wrap at both ends, and Escape returns the focus to the control that opened it.
  The review found the keys escaping an open picker from its Use button, and only a text field now keeps them.
- `59d9bf33`: an overlay opens under its control, above it where it does not fit, and otherwise on the side with more room, scrolling at that height.
- `4802a51c`, `b58ba1b2` and `6f25b042`: a tooltip in sheet mode, and on the Editor's header buttons, also shows on keyboard focus, never while an overlay is open.
  The review found the focus tooltip catching clicks meant for the control under it.
- `61ff86a5`: a sheet's button beside the preview shows what it does as its tooltip and accessible description.
- `b525bb2f` trims `agents/henad-app-sheet.md` back under 40,000 characters.

### Results that changed on purpose

The snapshot runs 39 models and model files at a fixed seed for 64 steps, two sweeps and six error runs through `henad-cli`.
At `b525bb2f` it was compared with the baseline in use since record #107, taken before this work.
No file of a deterministic model's statistics differs.
The five runs that never replay exactly, stock `gpu_boids`, the boids graph fixture and starter, and the boids and bounded confidence sheets, differ as always.

Each of the 44 strict differences has a cause.

- Three error texts, reworded by the string work: a graph type error, a parameter out of range and a sheet's missing state.
- The hash of `Cargo.lock`, which gained two macOS-only dependency lines of henad-app, objc2 and objc2-app-kit, at versions the lock already held.
- `library.lock`: 27 kinds.
  26 take new semantics hashes for the node library's reworded help, `agent.radius_fold` among them, whose text the graph package changed again.
  `rand.chance` changed for its stated resolution.
- The lowered graphs, parameter schemas and shaders of nine sheets whose chances or competing rates now draw through exact thresholds: the ants, forest fire and SIR starters, the SIR chance and SIRD fixtures, and four test sheets.
  The SIR starter's change is its New outbreak button.
- The shaders of the ants starter and one test sheet also take the full reduction of `space.delta`.
- The `[editor]` layouts of six graph starters, whose semantic text and code hashes are unchanged.
- The SIR sheet sweep's manifest, for the starter's new purpose and its code hash.

Results change on purpose in these cases, and no starter's statistics moved in the snapshot runs.

- A division by 0 in a Calculate returns 0, where it returned infinity or NaN.
- A look at agents in a walled world sees no one across an edge.
- A Steer towards a Calculate built from an empty look adds nothing.
- A competing group of rates of parameters and numbers, and a chance with no count, change where a draw falls within 2^-24 of the share or the chance.
  A chance below about 1e-6 per tick now runs at its own value, and the forest fire's Lightning chance of 1e-8 fired about six times too often before.
- A rate worked out per element at a hazard of 0 takes the chance 0 on a device where e^0 rounds below 1.
- The draws of a user's `finish` that draws change: each field now calls it with a key of its own.
- A `none of []` line in a hand-written sheet no longer fires every tick, and a share kept outside 0 to 1 is an error.
- A search candidate past the device is recorded `refused` instead of `panicked`, with another note.
- In the app, a GPU model played at a capped rate records each tick's own stats in the Charts tab and the Record CSV.

A sheet whose lowering changes takes a new code hash.
A results folder written before the change will not resume.

### How it was checked

#### The isolated full checks

The isolated full check runs every step of `./check.sh` and CI's two web clippy lines, 16 steps, with `HENAD_REQUIRE_GPU=1`, in a worktree with its own build directory.
It passed 16 of 16 steps at `b7804e51`, the end of the string work, with 2,276 tests.
It passed again at `82f6ca1c`, after the first seven fix packages, with 2,360 tests.
The Editor UX and accessibility packages merged after it, each with its crate checks and a snapshot identical outside the loose runs.
It passed 16 of 16 steps a third time at `b525bb2f`, after every package, with 2,385 tests.

#### Each package

Every package ran the tests of each crate it touched with `HENAD_REQUIRE_GPU=1`, clippy with `-D warnings`, rustfmt, and the wasm32 typecheck of each crate with web support.
A package that changed a string or a doc comment ran `cargo doc` with `-D warnings`.
Each ran the snapshot and compared it with the baseline or with a base snapshot of its own branch point, and reported every strict difference with its cause.
The reviewers ran the checks again on the final tree of their package.

The reviewers checked that a fix's test fails without the fix, and made several tests stricter that passed without it.
The card's rename test, the New model list's scroll test and the menu names test were among them.

#### On screen

No change was seen in the live app.
The UI packages rendered their changes offscreen, from the Editor's own UI code.
The audit's UX finder did the same.

### Edited tree

```
Cargo.toml, Cargo.lock                         objc2 and objc2-app-kit for henad-app on macOS, at versions the lock held
CHANGELOG.md                                   the changelog lines of the fixes, the rewording and the 0.4 migration
index.html                                     the loading page's text
agents/                                        the topic file of every area changed, 20 files
crates/
├── henad/                                     the facade re-exports ShortfallWords
├── henad-app/
│   ├── Cargo.toml                             objc2 and objc2-app-kit on macOS, the web-sys events of the leave prompt
│   └── src/
│       ├── native.rs                          Cmd+Q asks over unsaved edits, the app menu's Quit item removed
│       ├── web.rs                             the browser asks before it leaves a page with unsaved edits
│       ├── state.rs, state/                   new defaults at Apply, rejected builds, the modal hold, wording
│       ├── tests/                             a test of each fix, in the existing files
│       └── ui/
│           ├── editor/mod.rs, expert.rs       reconversion, late opens and saves, the seed at Apply, header tooltips
│           ├── editor/graph/                  Frame all to zoom 0.1, wording
│           ├── editor/sheet/                  same-frame edits, slider focus, Escape, pickers, overlays, menu roles, keys
│           ├── export/mod.rs                  a recording named after its own run
│           ├── model.rs                       the Exact replay row
│           ├── params/                        the world buttons' tooltips, wording
│           ├── sweep/                         the draft kept by id, the unapplied Editor warning, wording
│           └── results/, files/, ...          wording
├── henad-build/src/                           shader build errors
├── henad-cli/src/                             exports onto a model file, the note on a model file not run, help
├── henad-compute/src/
│   ├── cpu/sim_thread.rs                      run-to samples prepare the view
│   ├── gpu/sim_thread.rs                      snapshots wait for their readback, the timing of capped batches
│   └── gpu/, entry/, simulation.rs            engine and capacity errors
├── henad-core/src/                            parameter, action and plan errors
├── henad-explore/src/                         candidates past the device, the time scale at resume and merge, author text
├── henad-graph/
│   ├── library.lock                           27 semantics hashes
│   ├── sheets/                                purposes and help of the sheet starters
│   ├── starters/                              [editor] layouts with no node over another
│   └── src/
│       ├── catalogue/                         node help, the rand.chance and agent.radius_fold semantics
│       ├── check/                             a field's finish and its cost, wording
│       ├── edit/, host/, reader.rs, ...       count nouns, wording
│       ├── sheet/check/                       value types, formula depth, world limits, Depending on, kept shares
│       ├── sheet/edit/                        cycles, nesting and size, emptied lists and formulas, long kind ids
│       ├── sheet/lower/                       division by 0, walls, thresholds, looks that read looks, the add-up warning
│       ├── sheet/sentence/                    the card law, rate notes, new notes and tooltips
│       ├── wgsl/                              the full space.delta reduction, integer division by 0, wording
│       └── tests/                             a test of each fix, in the existing files
└── henad-models/src/tests/                    budget and look fault wording
docs/
├── developing/
│   ├── agent-record/
│   │   └── 20261122-108-strings-and-audit-fixes.md   this record
│   └── gpu-backend.md                         capped snapshot batches
├── guide/app.md, editor.md, search.md, sweeps.md
└── reference/cli.md, model-files.md           limits of a graph model, exports and model files, wording
zensical.toml                                  nav entry for this record
```

No file was added, removed or renamed by the 166 commits.

## State after

- `37-node-editor` at `b525bb2f` holds 166 commits after `68447db5` and 829 after `e816b444`, and this record follows them.
- 26 commits of the string work, `22036047` to `b7804e51` without `5cfd00be`, carry the maintainer's signature from a rebase.
  Every other commit is unsigned.
- Nothing was pushed, tagged or opened as a PR.
  The remote's `37-node-editor` is at `e816b444`.
- The isolated full check passed 16 of 16 steps at `b7804e51` with 2,276 tests, at `82f6ca1c` with 2,360, and at `b525bb2f` with 2,385.
- The snapshot at `b525bb2f` keeps every deterministic model's statistics identical to the baseline, and its 44 other differences are the ones listed above.
- The packages' changelog lines are folded into `CHANGELOG.md` in `74d340eb` and `49c9f0c9`, after `b525bb2f`: a fix to a feature new since 0.3.0 changes that feature's line, a fix to what 0.3.0 shipped goes under Fixed, and the 0.4 migration lines go under "Migrating to 0.4".
- This record changes only the record and its nav entry, and `uv run zensical build` builds the site with it.

## Issues found & future directions

### Left, with the options for each

#### Findings left whole

- **A whole-number sheet parameter with a negative minimum** takes fractional values from `--set`, `--sample` and searches.
  `ParamKind` has no signed integer, and henad-core's `check_value` ignores a float step.
  (a) Add a signed integer kind, a schema change.
  (b) Mark a sheet's whole numbers as integer in the descriptor, and have `check_value` and the samplers reject or round a value off the step.
  (c) Round at read in the lowering.
  It hides the symptom while `runs.csv` still shows fractions.
  The reviewer picked (b).
  It needs the maintainer and the owners of henad-core and henad-explore.
- **A stat named `time` or `tick`** writes a second column of that name in `--export-stats` and `series.csv`.
  (a) The checkers reject a measure of that name, and sheets that build today stop building.
  (b) A clashing stat column takes a new header, for those models only.
  (c) The clock column is renamed, such as `time_<unit>`, for every model with a time scale.
  The reviewer picked (a), with a debug assertion for compiled models, to keep the CSV format.
- **Counters that run the whole run are 32-bit** and wrap at 2^32, while `stat.counter` reports a u64.
  The wrap is in the documented semantics of `counter.add`.
  A fix changes a kind of node library 1 and the GPU counter buffer, and the maintainer decides.
  (a) A carry word per counter, a 64-bit workgroup tally and a u64 on the host.
  (b) Fold the device word into a u64 on the host at each readback.
  It fails when one step adds more than 2^32, as 10M agents adding 5,000 each would.
  (c) Document the wrap where the sheets' Record lines and event measures are described.
  The reviewer picked (a), with (c) meanwhile.
- **The work charge of a radius fold** assumes the agents spread evenly, so a clustered start passes the budget at many times the charged work.
  (a) Charge each fold at its visit cap when the start writes one position for every agent.
  Many valid models would then be rejected.
  (b) Measure the largest bucket of the index after the first step, and size later submissions from it.
  (c) Keep the documented approximation.
  The reviewer picked (b).
- **State names are cut off** in the property tables at the default 1280 by 800 Editor.
  States could wrap, move to a row of their own, or the columns could give up width in another order.
  The app's style rule requires a mock-up first.
- **Two sheets can share one model id**, and Apply of the second replaces the first in the model list, as the Editor guide documents.
  The choice is between a confirmation at Apply and a numbered id.
- **The Sweep tab shows a percent parameter as "1.0%"** and rejects "1%" in its values field.
  The field holds the `--vary` level text in fractions.
  A fix chooses where to convert, in that field only or in henad-explore's level grammar, and how a range like `1%:5%:1%` reads.

#### Partly fixed

- **"Add up" marks on the ground.**
  The warning covers a Leave of a number or a parameter.
  A worked-out amount, as in the ants starter set to add up, still wraps with no warning, a mark below 2^-17 adds nothing, and the Add up tooltip does not state the bound.
  (a) A saturating sum merge in henad-compute, a compare-and-swap loop that runs slower.
  (b) A resolution per sheet chosen from the largest population times the largest amount.
  It needs a bound on worked-out amounts.
  (c) Also warn for a worked-out amount when the largest population reaches 65,536, and state the bound in the tooltip.
  The reviewer picked (c) now, and (a) if exact sums matter.
- **A Steer towards a Choose or Depending on** whose chosen value is an average of no one, with no count test in its rows, still steers towards 0.
  An emptiness flag per row through the select chain would fix it.
  The reviewer kept the current rule, that the author tests the count in a row, until the maintainer decides.
- **An any or all summary of a value that is not yes or no**, such as any of their Velocity, passes the checker at its own line and fails only at the lines that read it.
  Expecting yes or no for Any and All would close it.
- **Chances at 32 bits** cover a chance with no count.
  A chance with tries, or one worked out per cell, still draws at 2^-24 through `rand.chance`, as its semantics and the model file reference now state.
- **`agent.radius_fold` on the GPU** corrects a difference once, so an agent a world and a half or more away on an axis is missed on the GPU and found on the host.
  Its semantics state it.
  The new `space.delta` reduction evaluates both arms of its `select`, two float remainders a call.
- **An emptied formula.**
  A file saved with a Calculate with no formula in a slot still shows a blank that cannot be filled, and nothing asks before a delete empties a formula.
- **The formula depth message**, "Group some of them in parentheses", is untrue for deeply nested negations or calls, where parentheses do not help.
- **Two saves pending under one target** are not told apart, where the save dialog leaves the window usable (Windows and Linux) and Save is pressed again while the first dialog is open.
  Numbering each save, as the save of sweep results already does, would fix it.
- **The Apply line after a rejected build.**
  "Model build failed. Fix the problem below." stays after the seed is fixed, until a build runs the applied code.
  A separate outcome for a rejected build would fix it, and changes behaviour the app's tests hold.
- **A tuned slider against a changed sheet value.**
  A Parameters slider moved off its default wins over a value changed on the sheet at the next Apply.
  Making the sheet win would drop tuned values at every reload of the code, and the maintainer decides.
- **Quitting on macOS.**
  Quit in the Dock, logout and a quit by Apple Event end the app without the dialog, and the app menu has no Quit item.
  Retargeting Quit needs calls that objc2-app-kit marks `unsafe`, and the workspace denies unsafe code.
- **A resume refusal** reads "A resume cannot change the code of a model composed at run time", with no word on the model file.
  henad-explore does not know a model came from a file, and the CLI would have to wrap the error.
  `ExploreError::Capacity` and `ExploreError::Execution` still say "sweep" for a search, and a search still spends evaluations on candidates past the device.
- **In a browser**, a Step, action or schedule that arrives before a playing batch's readback lands still skips its copy, as before this work.
- **Editor UX.**
  A converted copy's node labels are unreadable at about 0.13 zoom, Tab does not enter the add menus, the Plan panel shows no warning of an unapplied Editor, and the tabs still use different words for Build.
  The pickers hide a parameter whose range the check rejects, and greying it out with the reason is the other way.
- **Accessibility.**
  No toast was built: notices stay inline until the next edit, and a toast is a new visual feature.
  No shadow marks the hidden items of a scrolling overlay, and one needs a mock-up first.
  Item names that contain another item's name stay, as the design sets them: "Become, with chance" beside "With chance", Grey beside Dark grey, and "Alive neighbours 2".
  The About, fault, open results, replace results and abort sweep dialogs have no dialog role, modal flag or heading.
  Tooltips in the other dock tabs show on hover only, the world's buttons have no tooltips after Apply, and the help popover and the colour picker move the focus by egui's geometry.

#### Smaller items

- `c1668c18`, `1abb36c0` and `cdc20f40` warn in the wasm32 typecheck, until `68357827` drops a stale expectation.
  It matters only when bisecting.
- The add menus of a button card were not checked for Count, Look around and Pick a cell.
  The checker rejects them on a card that runs once.
- The test of the starter layouts checks nodes and their outputs, and not the frames of regions or groups.
- A world button's tooltip over a stale preview can describe steps the world does not run, as the descriptions read the Editor's live sheet.
- A control with the keyboard focus that the mouse wheel scrolls out of view still shows its tooltip, held at the window's edge.
- A run to a tick paused by a dialog and resumed starts a new run to that tick, so the Charts tab spaces its samples differently on each side of the dialog.
  No result changes.

#### For the maintainer

- A share kept outside 0 to 1 in a sheet file was accepted and is now an error, and a warning is the other choice.
  Its message, "Type a share from 0 to 1, such as 0.99.", now also prints as a diagnostic in the CLI.
- The hand-written reference of a competing group of rates in henad-models' lowered fixtures still shows a float draw, where the lowering now writes a threshold.
  No test compares them.
- The new user-facing strings of the fix packages were not put before the maintainer as picks.
  The sheet lowering package's reviewer notes five that follow the existing note forms and were checked against no pick.

#### Strings outside the picks

- Lookalikes outside the inventory keep the old forms: "refuses" in the testing kit's live edit message, the log line "Model refused action", and "The view pass cannot be made".
- `templates/model-project/index.html` still reads "Loading…", where the root page now reads "Loading".
- "Another model is open in the Editor. Press Offload to stop the preview." also shows when the Editor holds nothing, where it is untrue.
- A Plan row's tooltip reads "Recorded by sweep" with a bare lowercase noun, and the Plan line "no table loaded" differs from "Select a CSV file".
- "{} files to {}" in the app's save message reads "1 files", and henad-explore's "has a colour of {} bytes" reads "1 bytes".
- The Sheet pane's side field takes up to 16,384 in a world of agents, whose limit is 10,000, and the sheet then fails at `[world]`.

### Not checked on a screen, in a browser or on other hardware

- **The live app.**
  No change of this work was seen in the running app, and the session judged the offscreen renders alone.
- **Cmd+Q** in a real window with an unsaved edit needs one press by the maintainer.
  A test of the change passes without the menu change.
- **The browser's leave prompt** never ran in a browser.
- **Tab into the add menus** has no test.
- **CI on the branch.**
  Nothing was pushed, and CI never ran.
- **Device tests on another GPU.**
  The packages report device tests on the machine of this work only, and none on another adapter.

### Process

- **Fable.**
  Fable generated string candidates in 20 of 37 batches, until the maintainer asked to stop using it, and every other agent ran on Opus, Sonnet or Haiku.
- **Rejected commits.**
  The permission system rejected the commits of several subagents.
  None tried another way round, and the session committed the reviewed work itself, as the maintainer had chosen local unsigned commits.
- **Missing reports.**
  No fix implementer wrote its report, and several committed little or nothing.
  Their reviewers rebuilt what each package fixed and left.
- **Signing.**
  The briefs' first rebase line did not turn signing off.
  One rebase signed 26 commits of the string work with the maintainer's key, cached at the time, and in another package a rebase stopped at its first commit when signing failed.
  Later rebases ran with signing off.
- **Commit messages** average 5.12 words, against the 4 the maintainer requested, and none passes six.
- **Topic files near their limit.**
  `agents/henad-app-sheet.md` held 40,020 characters before `b525bb2f` trimmed it to 39,943, and needs a split by subsystem before anything more goes in.
  `agents/henad-app.md` holds 35,349.
- **The push, the signing and the release** stay with the maintainer.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
