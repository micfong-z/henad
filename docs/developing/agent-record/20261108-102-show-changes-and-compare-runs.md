---
date: 2026-11-08
title: "Show changes and Compare runs"
description: "S30 of the #37 redesign. The banner of an edited node graph copy offers Show changes, which lists every node added, removed or changed since the conversion under its frame's title and badges them on the canvas, and Compare runs, which runs the sheet and its copy for 200 ticks off the UI thread and names the first tick a measure differs. The side panel shows the sheet's purpose and parameter meanings, and the graph editor's Categories gain colour buttons, a next unused colour for a new state and Add category."
icon: material/compare-horizontal
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s30-show-changes-compare, which was already on its tip d574226: four unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: d574226
delta_state: "committed on `37-node-editor` by fast-forward: `feat: show changes, notes and categories in the expert mode [draft]`, `feat: compare runs of a sheet and its copy [draft]`, `docs: editor guide for show changes and compare runs [draft]`, `fix: s30 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Show changes and Compare runs

> S30 builds the second half of the expert mode in the app.
> The banner of an edited copy gains Show changes, a toggle that lists the nodes added, removed and changed since the conversion at the top of the side panel, each under the title of its frame or under "In no frame", and marks the added and changed nodes on the canvas with a green or yellow badge and stroke.
> Compare runs, offered on an unedited or edited copy, runs the original sheet and the copy as two in-memory sweeps of one run through henad-explore, at the Parameters tab's seed and the preview's size for 200 ticks, then names the first tick at which a measure differs, or says they match, and plots both runs' measures in one plot.
> The side panel's "From the author model" section shows the sheet's purpose and each parameter's meaning from `[[editor.note]]`.
> The graph editor's Categories section gains a colour button per variant, the next unused palette colour for a new variant, and Add category with its first variant.

## State before

`37-node-editor` at `d574226`, after S27 (the first half of the expert mode).
A sheet converted to a node graph copy, and the copy's banner read unedited, edited with node counts, original changed or original not found, with Open <sheet> alone.
henad-graph's `changes_since_conversion` already grouped a copy's changes by frame, and the copy's `[editor]` region already held notes of the purpose and each parameter's meaning, but the app called the one only for the banner's counts and drew none of the notes.
The graph editor's Categories section showed each variant's colour as a coloured square, added a variant in grey, and could not add a category.

## What was done

**Show changes** (C8, D20).
`CopyBanner` keeps the `ChangeGroup`s it worked out in the `Edited` state, and the banner offers a "Show changes" toggle in that state alone (`EditorRequest::ShowChanges`, `EditorPanel::show_changes`, cleared by every load and swap).
`editor_ui` hands the shown groups to `GraphEditor::shown_changes` each frame.
The new `graph/changes.rs` draws "Changes since conversion" at the top of the side panel: each group under its frame title or "In no frame", the pass under it, a link per added or changed node ("Changed: Is (n2001)", by label or display name) that opens and frames the node on its canvas, and a line per removed node.
`marks_of` gives the open canvas a `ChangeMark` per added or changed node, which `node_frame` strokes and `show_header` badges, green for Added and yellow for Changed, an issue keeping the stroke first.

**Notes** (C10, D24).
`origin_ui` takes the copy's `[[editor.note]]`s and draws each title and text under "From the author model", after the model, file and date.

**Categories** (C11).
`categories_ui` draws each variant's colour with the sheet's `color_button` (the twelve palette colours, named "Colour of waned: Purple"), which runs `SetVariantColor`.
Add variant takes `next_variant_color`, the first of `STATE_COLORS` its category lacks.
`add_category_ui` adds a category with its id and first variant through `AddCategory`, since the checker refuses a category with no variant.
The three text fields carry accessible names through `set_name`, which the tests and the egui MCP server find them by.

**Compare runs** (C9, D20).
The new `ui/editor/compare.rs` holds `Comparison`, two `Side`s, each a `SweepRun` of one run in memory with the Sweep tab's provenance, build pool and a repaint waker.
`start_comparison` reads the original's canonical text (`expert::original_text`) and the copy's, builds both entries through `ModelFile`, and runs each for `COMPARE_TICKS` (200) ticks at the Parameters tab's seed or `DEFAULT_COMPARE_SEED` (1), with the sizes the preview caps and no reducers.
`header_ui` polls it each frame and draws `comparison_ui` under the banner while the Editor shows the graph: the line, "Close comparison" and one `show_plot` of every measure, the original solid and the copy dashed in the same data colour, with a grey vertical line at the first difference.
`first_difference` compares the measures both runs hold by name, tick by tick, two values that are not numbers counting as equal.
`Side`'s drop stops its run with `SweepRun::stop(STOP_WAIT)`, so closing, replacing or quitting never joins a run's thread without a bound, and `STOP_WAIT` is exported outside tests on native for it.
The comparison survives a swap to the sheet, hidden there, and is cleared by every load.
In a browser the button is disabled with "Compare runs is unavailable in a browser.", since a GPU model runs in no sweep there.

**Words.** henad-graph's `sheet::sentence` gains `SHOW_CHANGES`, `CHANGES_HEADING`, `IN_NO_FRAME`, `NODE_ADDED`, `NODE_REMOVED`, `NODE_CHANGED`, `COMPARE_RUNS`, `COMPARE_NEEDS_NATIVE`, `CLOSE_COMPARISON`, `compare_running`, `runs_match`, `runs_differ`, `runs_share_no_measure`, `compare_failed` and `compare_series`, with `a_comparison_of_a_copy_reads_in_full_sentences`.

**Tests** (`crates/henad-app/src/tests/expert.rs`): `show_changes_lists_a_highlife_edit_under_the_frame_of_line_2_and_marks_it_on_the_canvas`, `an_unedited_copy_offers_no_show_changes`, `the_notes_of_the_author_model_show_under_its_section`, `add_category_on_a_converted_sir_adds_a_state_with_its_colour_and_the_copy_still_checks`, `an_unedited_copy_matches_its_sheet_at_every_tick_of_compare_runs` and `compare_runs_names_the_first_tick_at_which_a_highlife_edit_differs`, and in `compare.rs` the unit tests of `first_difference`.
The HighLife tests rebuild the HighLife edit of henad-graph's `a_highlife_edit_is_found_by_changes_since_conversion` (line 2's comparison becomes an either of "is 3" and "is 6") in a test helper, and apply it to the converted copy in place, keeping the sheet as its counterpart.

**Docs and topic files.** `docs/guide/editor.md` gains Show changes, Compare runs, the notes and the category edits under "Converting to a node graph".
`agents/henad-app-editor.md` gains the Show changes, Compare runs and Categories paragraphs and the tests, and `agents/sim-runner.md` a sentence on Compare runs' bounded stop.
One CHANGELOG line under the unreleased Added section.

```text
crates/
├── henad-graph/src/
│   ├── sheet/sentence.rs           Show changes and Compare runs words
│   └── tests/sheet_sentence.rs     the comparison's sentences
└── henad-app/src/
    ├── state.rs                    STOP_WAIT exported on native
    ├── tests/expert.rs             the tests above
    └── ui/
        ├── sweep/session.rs        provenance shared with Compare runs
        └── editor/
            ├── compare.rs          new: Compare runs
            ├── expert.rs           banner buttons, changes, original text, notes
            ├── mod.rs              show_changes, comparison, requests, polling
            └── graph/
                ├── canvas.rs       change marks: stroke and badge
                ├── changes.rs      new: the changes list and marks
                ├── mod.rs          shown_changes, list at the top, notes
                └── world.rs        colour buttons, next colour, Add category
agents/ henad-app-editor.md, sim-runner.md
docs/guide/editor.md
CHANGELOG.md
```

**UI check.** Under the lock, with `app.ron` pointing the window at G15F1 (1600 by 960 at -1800, 60), the app ran from the worktree with inspection, the Editor opened from the View menu and widened by dragging the dock separators.
The screenshots stay outside the repository.

- New model, Game of Life, Model actions, Convert to node graph, Convert: the banner read "Node graph converted from Game of Life on 8 Nov 2026. Frames show the line each node came from." with Open Game of Life and Compare runs, and no Show changes.
- Compare runs on the unedited copy: "Game of Life (expert) matches Game of Life at every tick from 0 to 200." over a plot of "Alive, Game of Life" and "Alive, Game of Life (expert)", one line over the other.
- The HighLife edit on the canvas, by raw-position drags: line 2's constant 3 typed as 6, an Either and a Not added from quick-add, "Is not 3" of line 3 wired into the Not, line 2's comparison and the Not into the Either, and the Either into the Become's "Line 2 when", which is B36/S23.
  The banner read "Edited since: 2 nodes added, 2 changed." and the problems list emptied.
- Show changes listed "Line 2: Become Alive from Dead, when Alive neighbours is 3" with "Changed: Is (n2001)", "Lines 2 and 3: the new Status (the first change that applies wins)" with "Changed: Become (n5)", and "In no frame" with "Added: Either (n3004)" and "Added: Not (n3006)", and the canvas showed the yellow Changed badges and strokes on the comparison and the Become and the green Added ones on the two new nodes.
- Compare runs: "Game of Life (expert) first differs from Game of Life at tick 1: Alive is 22915 against 22455." (before the review, which groups the digits), the dashed copy above the solid sheet over the 200 ticks, and the vertical line at tick 1.
- The side panel's "From the author model" showed Model, File and Converted, then "Purpose" with the sheet's purpose and "Initial density" with "Share of cells alive at the start".
- Categories: the colour button of alive opened "Choose a colour", and Purple turned the alive cells of the preview purple.
- The app was quit, `app.ron` restored from its backup and the lock released.

## Deviations

- **The HighLife edit in the tests is applied to the document.** The acceptance's HighLife edit is henad-graph's own (one changed node under line 2's frame, two added), which changes line 2's comparison into an either, and no edit command changes a node's kind.
  The app tests build that file from the converted copy and put it in the Editor in place of the copy, keeping the sheet as its counterpart, so the banner, Show changes and Compare runs read it as an edited copy.
  The UI check made a HighLife edit by hand on the canvas instead, which changes two nodes and adds two.
- **The banner reads "2 nodes added, 1 changed", not "1 changed".** The acceptance's "Edited since: 1 changed" predates S27's review, which puts the noun on the first part of `node_edits`, and HighLife adds two nodes to the one it changes.
- **Add category takes a first variant.** The checker refuses a category with no variant, so the form asks for the category's id and its first variant, and the variant starts in the first palette colour.
- **A new variant takes the next unused palette colour** in place of the fixed grey it took, so "Add category ... adds a state with its colour" holds without a second step.
- **Compare runs runs at the preview's size**, the sizes `preview_values` caps (`grid_width`, `grid_height`, `num_agents`), and at the Parameters tab's seed or 1 when the tab sets none.
  The expert design names one seed and size and no value.
- **Each model runs as its own sweep**, two `SweepRun`s, since a sweep runs one model entry. Each acquires its own device, as the Sweep tab's runs do.
  The expert design (section 6) names one device for Compare runs.
  Both devices come from the same adapter, so the two runs share the hardware the reproducibility contract names, but they are two `wgpu::Device`s.
- **Compare runs needs an original that matches the record.** The banner offers it on an unedited or edited copy, as the expert design's banner table does, and never once the original changed or is not found.

## State after

The expert mode is complete in the app: a sheet converts to a copy, the copy lists and marks its changes since the conversion, runs beside its sheet to show whether and where an edit moved the results, and shows the sheet's prose, and its categories take new states and colours.
`37-node-editor` holds the four unsigned commits `139a57e`, `815a3ac`, `17c168f` and `de02ce5`, fast-forwarded with no merge-check fix.

### Merge check

- The branch already sat on the tip of `37-node-editor` (`d574226`), so the rebase replayed nothing and raised no conflict.
- The full check then ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name.
- No baseline test passes now.
- The rebase brought in no app change, so the UI check was not repeated.

## Issues found & future directions

- **The node menu offers no constant for an input of a generic kind.** A new Is (`cmp.eq`) with `a` wired showed only "Bind to parameter" and "Clear" for `b`, so a comparison with a typed number cannot be added from the canvas, and the UI check's HighLife edit used a Not of line 3's "Is not 3" instead.
  This predates S30.
- **Two devices for one comparison.** Each run acquires its own device, as a Sweep tab run does. Sharing the app's device would halve the memory, at the cost of sharing its fault sink.
- **The plot repeats one colour per measure.** A model with many measures draws as many pairs, and the legend lists each twice.
- **Values compare exactly.** A copy edited in a way that reorders floating-point sums would differ in the last bits and report a first difference, which is the right answer for "same model, same seed", but can surprise.
- **The comparison is not saved.** Closing the Editor's model or quitting drops it, and Compare runs runs both models again.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D20 | Ways back | the expert design, section 8, question 4 | (a) The original kept, Compare runs and Show changes | S23, S30 |
| D24 | Sheet prose with no place in the IR | the expert design, section 8, question 8 | (a) `[[editor.note]]` | S23, S30 |

**Words added.** Outside the wording table and the expert design's section 5: "Changes since conversion", "In no frame", "Added", "Removed", "Changed", the link form "Changed: Is (n2001)", "Close comparison", the comparison's lines ("Running Game of Life and Game of Life (expert) for 200 ticks at seed 1.", "Game of Life (expert) matches Game of Life at every tick from 0 to 200.", "Game of Life (expert) first differs from Game of Life at tick 1: Alive is 22,915 against 22,455.", "Game of Life (expert) and Game of Life share no measure to compare.", "Comparison failed: ..."), the legend form "Alive, Game of Life", "Compare runs is unavailable in a browser.", the colour buttons' names "Colour of alive: Purple" (`color_button_name`), "Removed: Is (n2001)", the fields "New variant of status", "New category id" and "First variant", and "Add category" with its tooltips.

## Benchmark commands left for the maintainer

None: S30 changes no engine, no shader and no hot loop.

## Review

A fresh-context review found two should-fix findings and six nits, and `fix: s30 review findings [draft]` fixes all eight.
An edit that changes no node, such as Add category, a variant colour or a parameter default, left the copy `Edited` with an empty change list, and the banner still offered Show changes, which then drew nothing.
`CopyBanner::shows_changes` now also needs a change to list, and the SIR category test checks the banner offers no Show changes after Add category.
A finished comparison stayed on screen after the copy or the sheet was edited, describing a model the Editor no longer held.
`Comparison` now records the copy's load and code revision and the hash of the original's text (the banner's `BannerKey`, through the new `expert::current_copy_key`), and `header_ui` drops it once the Editor shows the copy with another key.
A placement keeps the code revision, so moving a node keeps the comparison, and `an_edit_of_the_copy_drops_the_comparison_of_the_copy_before_it` checks both.
The yellow Changed and green Added hues are now named in the state mapping of the `mcs.rs` module doc and in `agents/ui.md`.
Four doc comments that ended on a trailing "which" clause are split into two sentences.
The variant colour button takes its name from the new `sentence::color_button_name`, which `measure_color_name` now calls too, so the words live in henad-graph alone.
The two values of a first difference print through the Results tab's `format_precise` ("22,915 against 22,455"), and in every digit when six significant digits print both the same (`difference_texts`, with a unit test).
A removed node is listed by the display name of its kind in the original's lowered graph ("Removed: Is (n2001)"), through the new `CopyChanges`, which holds the groups and those names, with `a_removed_node_is_listed_by_the_name_of_its_kind`.
Its label is not resolved, since the conversion's labels are private to henad-graph's `convert` module and a removed node keeps no layout in the copy, so a labelled node reads by its kind.
The topic file now names the test's HighLife edit as the one of henad-graph's `a_highlife_edit_is_found_by_changes_since_conversion`, rebuilt in the app test, and the deviation above says the expert design names one device.

Fmt, clippy with `-D warnings` over henad-app and henad-graph (native, and henad-app on wasm32), the wasm32 typecheck of both, rustdoc with `-D warnings`, and their tests with `HENAD_REQUIRE_GPU=1` (henad-app 483 passed, henad-graph 457 passed and 1 ignored) pass.

The UI check ran again under the lock on G15F1, with `app.ron` restored and the lock released after it.
On a fresh conversion of Game of Life, Compare runs read "Game of Life (expert) matches Game of Life at every tick from 0 to 200.".
Add category "mood" with first variant "calm" turned the banner to "Edited since: no node changed." with Open Game of Life and Compare runs and no Show changes, and the comparison and its plot were gone.
Deleting line 2's comparison on the canvas read "Edited since: 1 node removed, 1 changed.", and Show changes listed "Removed: Is (n2001)" under line 2's frame and "Changed: Become (n5)" under the frame of lines 2 and 3.
After an undo, an initial density default of 0.31 in the copy, then Compare runs, read "Game of Life (expert) first differs from Game of Life at tick 0: Alive is 20,265 against 19,594.".
Four screenshots (`review-1-match.png` to `review-4-differ.png`) sit beside the earlier ones, outside the repository.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
