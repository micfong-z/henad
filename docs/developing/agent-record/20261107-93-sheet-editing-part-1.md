---
date: 2026-11-07
title: "Sheet editing, part 1"
description: "S16a of the #37 redesign. The Sheet pane edits World (size up to the engine's grid limit, edges with their sentence, the length of one tick) and Cells (a property form with States, Number and Integer, the twelve-colour palette, Starts as with by-chance rows, values and random numbers, number fields that take percents, Use a parameter and the New parameter dialog). New model offers Blank. Until the first tick an edit redraws the start, and after it a changed start waits for Start again while any other edit restarts the world from tick 0 with a notice."
icon: material/table-edit
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s16a-sheet-editing-1 after its merge rebase onto f8e92ce (S20 and S21 merged): three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 4cf2a76
delta_state: "committed on `37-node-editor` by fast-forward: `feat: edit world, time and cells, and start from blank [draft]`, `fix: s16a review findings [draft]`, `fix: s16a merge check [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet editing, part 1

> S16a makes the first sections of the Sheet pane editable (row R5), with the display half of T1, the Time line, and decisions D23, D52, D72 and D75.
> World edits the size of the grid up to the engine's 16384 cells a side (D72), the edges with their grey sentence and tooltip (no torus mark, D93), and "One tick is [number] [unit]" with "one step (no unit)".
> Cells adds, changes and deletes properties through a form with States, Number and Integer (D23), the twelve palette colours and usual names taking usual colours, and sets Starts as: All [state], By chance rows with "otherwise", a value or a random number.
> Starts as numbers are plain fields that commit on Enter or on losing focus, take a percent and refuse a chance outside 0 to 1 (D75, D110, D111), with "Use a parameter" opening a picker and the New parameter dialog.
> The New model dialog offers Blank, a 100 by 100 grid that wraps around, and a blank model shows the empty texts of the sheet, the card and the world.
> Until the preview's first tick or a button press, every edit redraws the start at tick 0 (D52). After that, a changed Starts as reads "Start again to see it" and any other edit restarts the world from tick 0 with "Changes restart the world from tick 0.".
> Every edit runs `SheetEdit` commands as one edit of the sheet, and henad-graph gains `SetPropertyType` so a changed kind keeps the property's place.

## State before

- `37-node-editor` at `4cf2a76`, S18 merged, with S20 and S21 open beside S16a.
- The Sheet and Card panes drew a sheet read-only (S13a, S13b), and the edit commands of S12 had no caller in the app.
- The New model dialog offered the three grid starters, and Blank was missing.
- The words S16a shows were in `henad_graph::sheet::sentence` from S4, so the milestone adds only the refusal for a grid side past the limit.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### henad-graph

- `SheetEdit::SetPropertyType { property, property_type }` clears every reference to the property and its states as a delete does, drops a colour rule over the property, empties its states, its start and its marks, and keeps its place and its id.
  A property that becomes a States property colours its thing by state again while the thing has no colour rule, and the type the property already has changes nothing.
  The random-edit test of S12 now draws it too.
- `edit::PALETTE_COLORS` holds the twelve colours of `STATE_COLORS` in the order of `sentence::COLOR_NAMES`, for the colour palette.
- `sentence::read_typed_number` reads a typed number, a trailing "%" making a percent, and refuses "inf", "nan" and anything else that is not one number.
  `sentence::literal_words` is public for a field that shows a literal, and `sentence::world_side_out_of_range` words the refusal of a grid side.

### The edit path in the app

`SheetEditor` holds a `SheetBuilder` in place of the bare document, so new draws take fresh keys.
`SheetEditor::edit(past_start, starts_only, run)` runs a closure of commands on a clone of the builder and keeps it only when every command is accepted, so a form's Done or a field's commit is one edit of the sheet.
It writes the canonical text, the report and the lowering again through `refresh`.
Every edit takes a fresh preview key, so the debounced preview builds again paused at tick 0, except an edit of starts alone while `past_start` holds, which keeps the key and leaves the running world alone.
`built_starts` records each property's start when the preview is built (`mark_built`, called from `preview::build_preview`), and `start_changed` compares against it for "Start again to see it".
A restart while `past_start` holds sets the notice, listed under the preview line.
`AppState::sheet_preview_past_start` holds while the sheet's preview has passed tick 0, runs, or had a button pressed in it (`Preview::acted`, set by the Parameters tab's action buttons and cleared by every preview build).
Build, which is Start again for a sheet's preview, rebuilds the preview from the current sheet and so draws a changed start.

### The empty world

`SheetEditor::shows_nothing` holds for a world of cells with no property.
The preview source then reports errors and `empty`, `preview::poll_at` offloads a running Editor preview so the Viewport draws nothing, and the list under the panes reads "Add a property on the sheet to see the world." in place of the preview line, leaving the checker's own warning about the empty sheet out.
The Cells section reads "Every cell has the properties listed here. Add the first one to start." with "+ Add property" as the blue 700 primary action, and the Card pane, with no card, reads "Each tick, every cell" and "Nothing changes yet. Add a line below.".

### The Sheet pane

`sheet_pane.rs` collects at most one pending edit per frame and runs it once the pane is drawn.
A refusal of a command shows as the form's message, or under the Cells section for any other edit.

- **World.** Width and height are number fields named "Width in cells" and "Height in cells", taking a whole number of cells from 1 to `henad_graph::limits::MAX_GRID_SIDE` and refusing anything else, a fraction or a percent included, with "Type a size from 1 to 16384 cells." or "Type one number, such as 3 or 0.25.".
  Edges is a select named "Edges" with the wording table's tooltip and the grey sentence under it.
- **Time.** "One tick is" is followed by the length field "Length of one tick" while a unit is set (a length of 0 or less becomes 1) and the unit select "Unit of one tick", whose first choice, "one step (no unit)", writes no `[time]`.
- **Cells.** The table's name is a button named "Edit property Status", the kind shows each state's swatch and name, or "Number", or "Integer" with its tooltip, Starts as is a select named "Status starts as", and Changed by is unchanged.
  By chance draws each row on a line of its own, then "+ Add row" (named "Add row to Status starts as") and "otherwise" with its select. A new by-chance start draws the second state with an empty chance and gives every other cell the first.
  A Number property starts at a value or at a random number between two values, and an Integer property at a value only, since the lowering refuses a random whole number.
- **Starts as numbers** (D111). Each is a field named as the wording table gives it, showing a percent as typed, with "Use a parameter" beside it opening "Choose a parameter".
  A chosen parameter replaces the field with its chip, which reopens "Choose a chance" or "Choose a number" with the Number box.
  An empty field empties the value, text that does not read keeps the old value and shows the red message under the row, and a chance outside 0 to 1 shows "A chance is from 0 to 1, or from 0% to 100%.".
- **The property form.** "New property" or "Edit property Status", Name, the three kinds, and for States the rows of a colour swatch, a name field and a remove button, then "+ Add state".
  A new property starts with two empty rows, Enter in a state name moves to the next row or adds one, and a row whose colour is not chosen by hand takes the usual colour of a usual name (B16).
  Done checks in the page's order, then adds the property with its states and a start in its first state (or at 0 for a number), or renames, recolours, adds and deletes states of an edited one.
  A changed kind runs `SetPropertyType` and starts the property afresh. "Delete property" deletes it.

### Widgets and the New parameter dialog

`ui/editor/sheet/widgets.rs` holds what the panes share: the chip and the dashed blank (moved from the card pane), `set_name` for the accessible name of a field or a select, `commit_field`, the `Picker`, `NumberUse`, the colour swatch with "Choose a colour", `color_name` (the nearest palette name, for a starter's own colours) and the New parameter dialog.
`commit_field` keeps the typed or refused text in egui temp data and calls the caller's closure on Enter or on focus loss, unless the text equals the value and no message shows.
The picker leaves out an empty group, offers "New parameter..." and a Number box named by its hint, and says what is missing when it has nothing to offer.
The New parameter dialog starts from 0.1 in 0 to 1 for a chance, from 1 in 0 to 10 for any other number, or from a typed number with Min at twice a negative one and Max a round number about ten times it, at most 1 below 1 or for a chance, as NOTES.md gives.
Max and a negative Min follow Default until typed, every field takes a percent, the parameter keeps the decimal, and Create parameter runs the wording table's checks in order.
Created from a Starts as number, the parameter and its use in the start are one edit.

### The New model dialog

Blank comes first, named "Untitled model", with "What is the world made of?" under it while it is chosen: "Cells on a grid" chosen and "Agents in open space" disabled until S26.
A blank model is 100 by 100 cells that wrap around, with an id minted from its name and no property, parameter or card.

### Tests

`crates/henad-app/src/tests/sheet_pane.rs`:

- `a_blank_model_builds_the_life_start_by_accessible_names`: New model, Blank, Create, Status with Dead and Alive, By chance, "Use a parameter", "New parameter...", Initial density at 0.3, every control found by its accessible name.
  A count of Alive is then added by command, and the preview's tick-0 Alive share is 0.3 +/- 0.02 on 100 by 100 cells.
- `a_blank_world_shows_nothing_until_a_property_is_added`: a running preview is taken down, the preview line reads the empty text, and adding Status builds the preview.
- `after_play_a_start_edit_waits_and_a_size_edit_restarts_with_the_notice`: at tick 0 a start edit redraws, after Play it keeps the preview key and reads "Start again to see it", a parameter made from a start's field waits the same way, and a width of 200 restarts at tick 0 with the notice and the new start drawn.
- `the_world_takes_the_engine_grid_limit_and_refuses_past_it`: 1024 by 1024 is accepted, 16385, "abc", 2.5 and "50%" are refused in the field with the text kept, and Edges and the Time line edit as the wording table says.
- `a_refused_text_stays_with_the_sheet_and_the_row_it_was_typed_in`: a width refused in Game of Life and a chance refused in one blank model do not show in the next blank model, and a by-chance row that moves up after a removal shows its own chance.
- `a_field_keeps_typed_text_while_the_window_lacks_the_system_focus`: with the harness's window holding no system focus, typed text stays in the width field and Enter commits it.
- `a_starts_as_chance_takes_a_percent_and_refuses_what_is_no_chance`: 1.5 and "abc" are refused, "30%" is stored as a percent and shows as typed, and a Number start's value and random number commit.
- `editing_a_property_renames_recolors_and_changes_its_kind`, `the_property_form_checks_in_the_order_of_the_page` and `the_new_parameter_dialog_starts_from_the_values_of_the_page`.
- `the_edited_sections_show_no_forbidden_word_in_any_state`: 29 states of the World, Time and Cells sections, the property form in each kind, the colour palette, the pickers, the New parameter dialog and its message, the refused fields and an edited property, with the label-in-name check of D112 over the controls that have words of their own.

henad-graph: `a_new_property_type_clears_every_reference_and_keeps_the_place` and `a_typed_number_reads_as_its_known_answers`.
The S13a test of the New model dialog now expects Blank first with "Untitled model", and leaves the world question's radio buttons out of the starters.
The `Harness`, its `Node`, the words rule's helpers and the snapshot helpers moved from `sheet_editor.rs` into `src/tests/support.rs`, so both files share them.

### UI check on G15F1

The UI lock of plan section 2.4 was taken, `app.ron` backed up and its window set to (-1800, 60) at 1600 by 960 on G15F1, and the app run from the worktree with `EGUI_INSPECTION=1 cargo run -p henad-app --features inspection`.
The screenshots stay outside the repository.

1. The New model dialog with Blank chosen, its description, the world question with "Agents in open space" greyed, the three starters and Name "Untitled model".
2. The blank model, with the sheet's empty text and the blue "+ Add property", the card's "Nothing changes yet. Add a line below.", the list's "Add a property on the sheet to see the world." and an empty Viewport.
3. The form with Status, the kinds, Dead in black (its usual colour) and Alive in blue.
4. After Done, the table reads Status, States with swatches, "All Dead" and "no line yet", "Colour cells by Status" appears, and the Viewport shows the 100 by 100 world all Dead at tick 0 without Play.
5. By chance with its row, "otherwise" Dead, and "Choose a parameter" open on "Use a parameter".
6. The New parameter dialog with Initial density, Default 0.3, Min 0 and Max 1.
7. The chip Initial density in the row, the parameter on the sheet, and the Viewport following at tick 0 with about 30% Alive in blue.
8. "One tick is" with the length field at 1 and the unit select on "days".
9. After Play, "All Dead" chosen, "Start again to see it" under the select, and the world running on with its old start.
10. The width set to 150 after Play, the preview rebuilt at 150 by 100 at tick 0 with the new start, and "Changes restart the world from tick 0." under the preview line.
11. A width past the limit kept in the field with "Type a size from 1 to 16384 cells." in red under the row.

In the first session, typing into the width field after Play left its text unchanged several times while the field held the focus.
The app was restarted with a log of the field's frames, and the same sequence then worked every time, the restart notice included, so the cause was not found.
The log showed that the app draws two passes in some frames, so a commit runs in both passes. The second sees the kept value and commits nothing.
The review session found the cause, recorded under "Review".
After the check the Kind column was changed to put each state's swatch and name on one line, as the fourth screenshot shows them wrapped onto two.
The review session's check saw the new layout.
The app was quit, `app.ron` restored from the milestone's backup and the backup deleted, and the lock released.

The review session repeated the affected checks under the same lock and window, with the review fixes built.

12. Game of Life with 2.5 typed as the width: the field keeps "2.5" with "Type a size from 1 to 16384 cells." in red, and the Kind column shows each state's swatch and name on one line.
13. New model, Blank, Create straight after: the width field reads 100 with no message.
14. A blank model with Status Alive by chance at 0.3, played to tick 49073, then a new parameter Seed share made from row 1: the chip in the row, "Start again to see it" under the start, no restart notice, and the world still running.
15. The width typed as 150 after that: the preview rebuilt at 150 by 100 at tick 0, with "Changes restart the world from tick 0." under the preview line.

### Docs and topic files

- `agents/henad-app-sheet.md`: the edit path and the restart rule, the empty world, the editable World, Time and Cells sections, the property form, the widgets and the New parameter dialog, Blank, and the new tests.
- `agents/ui.md`: the swatches, the palette as data colours, the primary "+ Add property" and the field message colour.
- `agents/henad-graph-sheet-edit.md`: `SetPropertyType`, `PALETTE_COLORS` and the new test.
- `agents/henad-graph-sheet.md`: `literal_words`, `read_typed_number` and `world_side_out_of_range`.
- `CHANGELOG.md`: one line under the unreleased Added.

The topic files stay under 40k characters: `agents/henad-app-sheet.md` is 23,633, `agents/henad-graph-sheet.md` 29,701, `agents/henad-graph-sheet-edit.md` 10,484 and `agents/ui.md` 6,298.

### Edited tree

```text
henad/
├── CHANGELOG.md                                   (S16a line)
├── agents/
│   ├── henad-app-sheet.md                         (edits, empty world, form, widgets, Blank)
│   ├── henad-graph-sheet-edit.md                  (SetPropertyType, PALETTE_COLORS)
│   ├── henad-graph-sheet.md                       (typed numbers)
│   └── ui.md                                      (swatches)
└── crates/
    ├── henad-graph/src/
    │   ├── sheet/
    │   │   ├── edit/mod.rs                        (SetPropertyType, PALETTE_COLORS)
    │   │   └── sentence.rs                        (read_typed_number, literal_words, world_side_out_of_range)
    │   └── tests/
    │       ├── sheet_edit.rs                      (property type test, random edits)
    │       └── sheet_sentence.rs                  (typed number test)
    └── henad-app/src/
        ├── state.rs                               (sheet_preview_past_start)
        ├── ui/
        │   ├── params.rs                          (a pressed button marks the preview)
        │   └── editor/
        │       ├── mod.rs                         (past_start, empty world, notice)
        │       ├── preview.rs                     (acted, mark_built, offload on empty)
        │       └── sheet/
        │           ├── mod.rs                     (SheetBuilder, edit, built starts, notice)
        │           ├── sheet_pane.rs              (World, Time and Cells editing, property form, ids per sheet)
        │           ├── widgets.rs                 (new: chip, blank, field, picker, palette, New parameter)
        │           ├── card_pane.rs               (widgets moved out, blank card text)
        │           └── new_model.rs               (Blank, world question)
        └── tests/
            ├── mod.rs                             (sheet_pane)
            ├── sheet_pane.rs                      (new)
            ├── sheet_editor.rs                    (Blank in the dialog test, helpers moved)
            └── support.rs                         (Harness and helpers, window_focused)
```

### Deviations from the plan

- **Starts as numbers are fields, not blanks.** The gate's D111 replaces the plan's Starts as built from the shared blank and picker. The blank is moved into `widgets.rs` for S19, and Starts as uses the field, "Use a parameter", the picker and the parameter chip.
- **A henad-graph command was added.** The plan lists no henad-graph change. The page clears every reference on a changed kind and keeps the property's place, and S12's commands could only delete and add it again at the end of the table, so `SetPropertyType` was added beside them with a test.
- **The acceptance share is read through a measure added by command.** Blank holds no measure until S16b, and a GPU preview publishes its cells as a texture. The test adds a count of Alive through `SheetEditor::edit`, which keeps the start's key, and reads the share from the preview's tick-0 snapshot.
- **No rate in the New parameter dialog yet.** `NumberUse` holds a chance and a number. Starts as offers chances only, and a rate variant with no caller would be an abstraction ahead of its use, so the rate's starting values and its "per [unit]" Unit wait for the first rate blank in S19.
- **A blank model holds no card.** The card pane draws "Each tick, every cell" and the blank text when the sheet has no card, and a cell card with no lines shows the same text. S19 adds the card with its first line through `AddCard`.
- **Integer starts at a value only.** The lowering refuses a random number for a whole-number property, so its Starts as select leaves "A random number" out.
- **One sentence the wording table lacks.** "Type a size from 1 to 16384 cells." words D72's refusal in the field, since the table has none for the engine's limit. No researcher met it.
- **The Time line does not reach the run yet.** A grid sheet's lowering writes no time scale before S21, so after "One tick is 1 day" the Playback counter still reads "(a tick is one step of time)". The Time line edits the sheet as the plan asks.
- **The form sits under the table.** The page opens the edit form under the property's row. egui's grid cannot span a row across its columns, so the form for a new and an edited property both sit under the table.

### Review

A reviewer with a fresh context listed three should-fix findings and six nits, and all nine held against the code.
They were fixed in `fix: s16a review findings [draft]`, with tests where a test can hold the fix.

- **Refused text crossed sheets.** A field's refused text and message sat in egui temp data under ids that every sheet shared, and a by-chance row's draft was keyed by its place.
  The Sheet pane now draws under an id made from the load number that opened the sheet (`PaneState::new`), and every field id and the picker's Number box derive from it.
  Removing a by-chance row forgets the drafts of that row and the rows after it.
  `a_refused_text_stays_with_the_sheet_and_the_row_it_was_typed_in` holds both.
- **The topic file contradicted itself.** `agents/henad-app-sheet.md` still said Blank was no starter, missed a comma and left the card pane paragraph unwrapped.
  All three are fixed.
- **The record cited its screenshot folder.** The UI check now says the screenshots stay outside the repository, and the list keeps their descriptions without file names.
- **The size field rounded.** "2.5" was kept as 3 and "50%" as 1.
  A side that is no whole number is now refused with "Type a size from 1 to 16384 cells.", as the topic file and this record describe.
- **A new parameter from a start restarted the world.** After Play, choosing a parameter waited for Start again, while making one from the same field restarted with the notice.
  The combined edit is now a changed start, since only the start uses the new parameter, and it waits as the other way does.
- **Doc comments, a test name, a long line and the order in `sentence.rs`.** The headless "what" phrases and trailing "which" clauses are rewritten, `recolours` in a test name is `recolors`, the random-number test moved into `holds_random_number`, and `read_typed_number` and `world_side_out_of_range` sit after the typed-number messages.

The repeated UI check reproduced the first session's typing failure at once: text typed into the width field never stayed.
A log of the field showed the text edit taking the typed text while `Response::has_focus` returned false.
That method also asks that the window holds the system's focus, and a window driven through the inspection port does not.
The draft was therefore never stored, and the next frame drew the old value again.
After the first session's restart the window most likely held the system's focus, and the failure did not show.
`commit_field` now asks `Memory::has_focus`, the same focus the text edit types under.
`Harness::window_focused` lets a test draw frames without the system's focus, and `a_field_keeps_typed_text_while_the_window_lacks_the_system_focus` fails with the old check.
This fix is outside the reviewer's list.
It answers the open item the implementer left on `commit_field`.

## State after

- The Sheet pane edits World, Time and Cells through `SheetEdit` commands, one edit per form or field commit, and Blank starts a model from nothing.
- The preview follows D52: edits redraw the start until the first tick or a button press, then a changed start waits for Start again and other edits restart with the notice.
- The tests of henad-graph and henad-app pass with `HENAD_REQUIRE_GPU=1`, clippy passes with `-D warnings`, and rustdoc and the wasm32 typecheck of both crates pass.
- `37-node-editor` holds the three unsigned commits, the feature, the review's fixes and the merge check's fix, fast-forwarded after the merge rebase onto `f8e92ce`.

### Merge check

- The merge rebase onto `37-node-editor` at `f8e92ce` brought in S21 (rates) and S20 (ground, picks and fused folds).
  The first commit conflicted in three files, and the second applied cleanly.
  - `CHANGELOG.md`: S21's two lines and S16a's line all kept.
  - `agents/henad-graph-sheet-edit.md`: the command list takes S16a's `SetPropertyType` and `PALETTE_COLORS` and S21's `ChangeToRate` and `ChangeToChance`, rewrapped to the file's width.
  - `crates/henad-app/src/tests/sheet_editor.rs`: the New model dialog test expects Blank first and S21's SIR starter second among the grid starters, as Blank, Game of Life, SIR epidemic, Majority vote and Forest fire.
- The full check then ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  `cargo fmt` failed on the merged line of the dialog test, which had grown past the line width, and `fix: s16a merge check [draft]` holds the formatter's wrap.
  The `fmt` step then exits 0.
  Every other step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.
- The rebase brought no henad-app change but the SIR starter in the New model dialog, which the dialog test covers, so no egui MCP check was repeated.
- S21 lowers `[time]` to the hidden parameter `tick_length`, so the deviation "The Time line does not reach the run yet" no longer holds at this tip.
  The Playback counter after "One tick is 1 day" was not checked in the app at the merge.

## Issues found & future directions

- **The editor pane is narrow in the default dock.** At 1600 by 960 the Sheet pane is about 270 points wide, so the by-chance rows wrap and the Changed by column is clipped while a row is open. The Starts as rows could move under the whole table row once a layout spans the columns.
- **S16b's undo.** One `SheetEditor::edit` is one edit of the sheet, so `History` can push the canonical text after each call. A refused field message is no step.
- **The notice stays until the next edit.** The page shows notices for seven seconds in a toast. The app has no toast, and the notice stays in the list under the panes until an edit clears it.
- **The tick counter and the Time line.** Once S21 lowers the time scale, the Playback counter reads the tick's length after "One tick is 1 day".

## Decisions adopted for review

From the plan's section 6 and the gate's sections 7 and 14.6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D23 | "Integer" or "whole number" | expert, section 8, question 7 | (a) "Integer" in the sheet and the expert mode, with the tooltip "A number with no fraction, such as 3 or -2." | S16a, S27 |
| D52 | Edits during a run (L2) | recon of the engine, section 2.10, and the prototype's "World" | L2 is not built. A model waits paused at tick 0, edits redraw the start until the first tick, and after that a valid edit restarts the world from tick 0 with a notice | S13a, S16a |
| D72 | World size bounds | the prototype's "Cells on a grid", 10 to 200 | **Departure** from the prototype's 10 to 200, a limit of its canvas. The sheet takes the engine's grid limits, since the Game of Life starter is 1024 by 1024 and Henad's purpose is large worlds. Blank keeps 100 by 100 | S16a |
| D75 | Input rules for chances and rates | the prototype's notes ("Typed chances must lie from 0 to 1 and rates may not be negative") | Built in the editor's fields and, for a hand-written file, as checker errors | S5, S16a, S19 |
| D93 | A torus mark under Edges | round C C14, the final prototype's section 17 | Not built. The grey sentence under Edges and the select's tooltip say what the mark would show | S16a |
| D110 | A number typed as a percent | round D D8 part 1 | A percent shows as typed in a Starts as field and is stored as `{ percent = 30 }`. The New parameter dialog keeps the decimal | S1a, S4, S5, S7, S12, S16a, S19 |
| D111 | Starts as numbers | round D D8 part 2 | Plain text fields with "Use a parameter" beside them, committing on Enter or on losing focus as one edit, with the red messages of the wording table, and a chosen parameter shown as its chip | S16a, S16b (the undo step) |

D74 (hand-set cells, not built) holds: the restart rule has two triggers, the first tick and a button press.

## Benchmark commands left for the maintainer

None: S16a changes no engine, no shader and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
