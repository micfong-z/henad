---
date: 2026-11-07
title: "Sheet editing, part 2"
description: "S16b of the #37 redesign. The Sheet pane edits its remaining sections: Purpose, Parameters (name, value slider and field, Min, Max, a free-text Unit with suggestions, + Add parameter), Measures (Plot boxes, + Add measure for a count or an average, the Plotted list with colours), Colours (a States property by its states, a Number property shaded in an MCS hue between two values) and Buttons (Start again or Run once, with the button's card read-only). A toolbar holds the model name and Undo and Redo, which also take Cmd+Z, Shift+Cmd+Z and Cmd+Y. A blank model rebuilds the Game of Life starter without its card lines."
icon: material/table-edit
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s16b-sheet-editing-2 after the merge rebase onto 16fdc61: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: cc9aa0b
delta_state: "committed on `37-node-editor` by fast-forward: `feat: edit parameters, measures, colours and buttons [draft]`, `feat: model name, undo and redo in the sheet editor [draft]`, `fix: s16b review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet editing, part 2

> S16b finishes the Sheet pane of the sheet editor: every section of a grid sheet is now edited on the sheet, with the words of the final `SPEC.md`.
> Purpose is free text, each parameter has its name, a slider and a field for its value, Min, Max and a free-text Unit with "per second" to "per year" offered under it, measures are ticked as Plot beside a state or added as a count or an average, the cells are coloured by a States property or shaded by a Number property in one of seven MCS hues between two values, and buttons start properties again or run their own card once.
> A toolbar above the panes holds the model name, which a click turns into a field, and Undo and Redo over the sheet's canonical text, also on Cmd or Ctrl+Z, Shift+Cmd+Z and Cmd+Y outside text fields.
> The shading needed three new keys on a `shaded` colour rule (`hue`, `darkest`, `lightest`), the one change of the author format here.
> A test builds the Game of Life starter from Blank by accessible names alone, and after the alignment of plan section 4.4 its canonical text equals the starter's without its card lines.

## State before

`37-node-editor` at `cc9aa0b`, after S16a (World, Time and Cells edited on the sheet, Blank in the New model dialog) and S24 (tried parameter values in the preview, the sheet's trying line, Keep).
The Sheet pane drew Purpose, Parameters, Measures, Colours and Buttons read-only, the parameters as a `kv_grid` with the trying line between grids.
`SheetEditor::edit` made one edit per form or field commit, but nothing recorded it for undo, and the Editor had no model name control in sheet mode.
A `shaded` colour rule over a Number property lowered to a fixed blue-to-orange ramp over 0 to 1, with no way to store a hue or a range.
The Charts tab drew nothing for a sheet with no measure.

## What was done

**Shading in a hue (henad-graph).**
`ColorRule` gains `hue` (`ShadeHue`, the seven MCS hues stored `gray` to `purple`), `darkest` and `lightest`.
The reader takes them (a hue outside the seven or a number that is not finite is refused at its key), the writer writes them after `decades`, and `ShadeHue::ends` returns the hue's MCS steps 900 and 200, the colours the final prototype's `SHADES` table holds.
`lower/colours.rs::shade` builds the palette `shaded_<property>` from eight colours evenly spaced between the two ends (`hue_colors`, the prototype's linear blend), feeds `view.ramp` with `darkest` and `lightest` (0 and 1 when left out, so the bounded confidence sheet lowers as before), and refuses two equal ends as a lowering problem at `[color.cells]`.
A rule darkest at the higher value reverses the palette and passes the lower value as the ramp's low end, since `view.ramp` takes its first band whenever `high` is not above `low`.
The ground's Number shading takes the same path.

**The Sheet pane.**
- Purpose is a field of several lines (`commit_area`) named "Purpose" with the placeholder "What is this model for?", committing on losing focus.
- Parameters draw a "Name" and "Value" header, then two rows per parameter: the name field ("Name of X"), the slider ("X value slider") and its field ("X value"), × ("Delete parameter X"), then Min, Max and Unit ("X min", "X max", "X unit").
  The slider steps by about a 200th of the range rounded down to 1, 2 or 5 times a power of ten, the prototype's `niceStep`, and a drag commits once when it is let go.
  A typed value outside the range widens it, Min above Max swaps them, equal ends make Max one more, an empty or taken name puts the old name back with "A parameter needs a name." or "A parameter called X already exists.", and every number field reads a percent.
  Unit is free text with the placeholder "none" and the suggestions in a popup under the field (D89).
  The trying line of S24 sits under its parameter.
  "+ Add parameter" opens the New parameter dialog at 1 in 0 to 10.
- Measures show each States property's states with a swatch and a "Plot" box named "Plot Alive" ("Plot Status Alive" when two properties share the state name), the "Plotted" list with each measure's colour button ("Colour of Alive: Green"), its sentence and × ("Remove measure Alive"), and "+ Add measure" opening the measure form: "New measure", "Number of cells in a state" or "Average of a number", the selects, Done and Cancel, with the three checks of the wording table.
  A count is named by its state and drawn in the state's colour, which follows a recoloured state, and an average is named "Average of Food" and takes the next of the measure colours.
- Colours show "Colour cells by" a select of every property, the states' swatches under a States property, and for a Number property "Shade" with the seven hues, a gradient swatch, and "from [Darkest at value] (dark) to [Lightest at value] (light)".
  The field refuses a value equal to the other end with "Darkest and lightest need different values.", and keeps a darkest value above the lightest one.
  The first property made colours the cells: a States property through its command as before, and a first Number property, with no States property and no rule, through a blue shading from 0 to 1.
- Buttons show each button's name, what it does ("Start again: Status", "Start again: every property", "Run once, every cell"), "Edit" and ×, and "+ Add button" opening the button form: Name ("Such as Clear", named "Button name"), Does with "Start again" beside "Start again for" (every property or one) or "Run once, every cell", and the two name checks.
  A Run once button shows its card under it, read-only, with the heading "Lines of Clear: run once, every cell, when pressed" and "No lines yet. Add a change below." while empty, and the Card pane shows that text under an empty card that runs once too.

**Toolbar, undo and the Charts tab.**
The sheet editor draws a toolbar above both panes: the model name as a frameless button named "Model name: Game of Life. Rename", which turns into a field named "Model name" (Enter or a click elsewhere keeps, Escape cancels), then Undo and Redo.
`SheetEditor` keeps the graph editor's `History` of canonical texts: `edit` and `keep_value` record the text before each kept edit, and Undo and Redo read a text back, close the pane's forms and restart the preview by the rule of D52, an undo of starts, the name or the purpose alone leaving the running world as the edit did.
Cmd or Ctrl+Z, Shift+Cmd+Z and Cmd+Y work anywhere in the Editor tab in sheet mode while no text field holds the focus and no modal of the app is open.
A new name or purpose leaves the running world alone.
The Charts tab shows "Tick Plot beside a state under Measures to plot the number of cells in it." while the run is the Editor's sheet and the sheet holds no measure (D87, D107).

**Tests.**
`src/tests/sheet_pane.rs` gains the Game of Life rebuild with its undo walk, the parameters' rules, the restart after Play, measures, shading, buttons, the toolbar rename, the Charts text, the words rule over the remaining sections in 26 states, and Keep as one undo step.
`src/tests/support.rs` gains `align_with_starter` and `without_card_lines`, with their own test, and `Harness::screen` for a tall screen.
`src/tests/sheet_editor.rs`'s Lines help test now orders the two help buttons by position, since the toolbar changed the order of the AccessKit nodes it relied on.
`src/tests/sheet_lower.rs` gains `a_hued_shading_ramps_from_its_darkest_to_its_lightest_value`.

```text
crates/henad-graph/src/sheet/
├── document.rs            # ColorRule hue, darkest, lightest; ShadeHue with ends
├── reader.rs              # the three colour rule keys
├── writer.rs              # the three keys after decades
├── sentence.rs            # words of the forms, the toolbar and the shading
└── lower/
    ├── colours.rs         # shade from the rule's hue and ends, hue_colors
    └── ground.rs          # the ground's shading through the same path
crates/henad-graph/src/tests/sheet_lower.rs   # a hued shading
crates/henad-app/src/ui/
├── charts.rs              # the text for a sheet with no measure
└── editor/
    ├── mod.rs             # the New model dialog keeps the undo keys
    └── sheet/
        ├── mod.rs         # toolbar, model name, History, undo, redo, keys
        ├── sheet_pane.rs  # Purpose, Parameters, Measures, Colours, Buttons and their forms
        ├── widgets.rs     # commit_area, suggestion_field over commit_edit
        └── card_pane.rs   # the text of an empty card that runs once
crates/henad-app/src/tests/
├── sheet_pane.rs          # the S16b tests
├── support.rs             # align_with_starter, without_card_lines, Harness::screen, the About modal
└── sheet_editor.rs        # Lines help buttons ordered by position
agents/henad-app-sheet.md, agents/henad-graph-sheet.md, agents/henad-graph-sheet-lower.md, agents/ui.md
CHANGELOG.md
```

**UI check.**
The check ran on the secondary display under the lock of plan section 2.4, from the worktree, through the egui MCP server.
The session was locked, so the app painted no frame for a screenshot, and every check reads the widget tree instead, as S24's checks after its review did.
From the New model dialog, Blank named Game of Life, then Status with Dead and Alive (Alive picked Green), By chance with a new parameter Initial density at 0.3, Plot Alive (the Plotted list showed "Number of cells whose Status is Alive" with "Colour of Alive: Green"), the starter's purpose, Randomise (Start again for Status, shown as "Start again: Status") and Clear (shown as "Run once, every cell" with "Lines of Clear: run once, every cell, when pressed" under it).
After Play the counter read "Tick 1844 (a tick is one step of time)", and the Charts tab's legend held an "Alive" entry.
The toolbar renamed the model to "Life sheet" ("Model name: Life sheet. Rename"), and Cmd+Z put "Game of Life" back.
Clicks on controls within about 50 points of the window's bottom edge were lost while the session was locked: the first Clear button came out as a Start again button, and the Edit form turned it into a Run once button through keyboard focus and Space.
No screenshot exists, and a look at the pane's layout on screen remains for the maintainer.

## Deviations

- **Three new colour rule keys.** The gate asks for a Number property shaded "from MCS step 900 to step 200 of the chosen hue" between two values, and the author format had nowhere to store the hue or the values.
  `hue`, `darkest` and `lightest` are optional, so every committed sheet reads, writes and lowers as before.
  The palette blends the two ends in eight steps, as the prototype blends them, rather than taking MCS steps 800 to 300.
- **The rebuild names the model in the New model dialog.** `SetSheetName` keeps the sheet's id, as the dialog keeps a starter's id under a new name, so a Blank renamed in the toolbar keeps `untitled_model` and cannot match `game_of_life`.
  Blank mints its id from the dialog's Name, so the rebuild names the model there, and the toolbar rename is tested on its own.
- **The alignment copies off-palette colours.** The Game of Life starter's colours (`#151515`, `#00E676`) are not among the twelve palette colours the editor offers.
  `align_with_starter` copies a state's or a measure's colour from the starter when the palette names both colours alike, so the rebuild still checks that the nearest colour (Black, Green) was picked.
  It also drops the starter's cards that run each tick along with their lines, since a card with no line is one the editor adds with its first line (S19), and sets `next_line` back to Blank's 1.
- **A button whose kind changes is deleted and added again.** No command turns a Start again button into a Run once button, which needs a card, so the button form deletes the button and adds it under its new name, at the end of the list.
- **A new name or purpose does not restart the world.** `Pending::one` counts `SetPurpose` and `SetSheetName` with `SetStartsAs` as edits that leave the running world alone, since neither changes what the world shows.
- **The Unit is free text (D89 over D3).** The plan's Unit select for rates is void, as the gate says.

**Words added.**
The milestone adds five UI words that the gate's wording table (sections 10.12 and 10.15) lacks, in the way section 10.21 lists the plan's own.

| Item | Words | Source | Built in |
| --- | --- | --- | --- |
| Button form heading for a new button | "New button" | the measure form's "New measure" | S16b |
| Model name tooltip | "Rename model" | the toolbar's model name button | S16b |
| Undo tooltip | "Undo last edit" | the graph editor's Undo | S16b |
| Redo tooltip | "Redo last undone edit" | the graph editor's Redo | S16b |
| Equal shading ends | "Darkest and lightest need different values." | the lowering's refusal of equal ends, in the form of "Min must be below Max." | S16b review |

## State after

Every section of a grid sheet is edited on the Sheet pane with the words of the final `SPEC.md`, and every edit is one step of Undo.
A blank model can be built into the Game of Life starter's sheet without its card lines, which S19 adds.
The Card pane stays read-only until S19, and so does a Run once button's card on the sheet.
`37-node-editor` holds the three unsigned commits, fast-forwarded with no merge-check fix.

### Merge check

- The merge rebase onto `37-node-editor` at `16fdc61` replayed the three commits over S29's lane packing.
  One conflict arose, in the Added list of `CHANGELOG.md`, where S29's lane packing line and this milestone's sheet editing line were both kept.
  The rebase brought in no henad-app change, so the UI check was not repeated.
- The full check then ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

- **The pane is long and narrow.** With every section editable, the Sheet pane in a 1600-point window scrolls well past one screen, and the parameter rows fill most of its default 40% width. A collapsible section header or a wider default would help.
- **Undo history is per sheet editor.** Opening another model drops it, as the graph editor's does. Undo also restores whole canonical texts, so a sheet whose text does not write (a number that is not finite) records an empty text that does not restore.
- **The suggestion popup is hand-made.** egui has no combo box with free text, so `suggestion_field` keeps its popup open in temp data. A Tab out of the field closes it, and a click elsewhere does too.
- **Average measures name their column "Average of Food".** A rename of the property leaves the measure's name, which is also its results column, as it was.
- **A Number property's colours only shade the cells.** The ground's shading takes the same keys through the same function, and S26 builds the controls for agents and the ground.
- **The session lock.** The UI check had no screenshots and lost clicks near the window's bottom edge, both recorded above.

## Decisions adopted for review

From the plan's section 6 and the gate's sections 7 and 14.6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D3 | The Unit column | the rates design, section 10, question 3 | (a) A select for rates ("per second" to "per year"), free text otherwise. Void under D89 | S16b, S25 |
| D87 | Plot scale and legend | `CHANGES-B.md` B12, round C C8, final `SPEC.md` 10.8 | **Plan's choice** within D56 and D73. The Charts tab keeps egui_plot's fit to the data with 0 in the y range (S9). The Scale select and the legend's start values are not built, and current values stay on the Stats tab. A sheet with no measure shows the Charts text (D107's words) | S9, S16b |
| D89 | The parameter Unit | `CHANGES-A.md` F8, `CHANGES-B.md` B6, final `SPEC.md` 7.4 and 8.9, against the rates design's question 3 (a) and D3 | **Conflict.** The final `SPEC.md`'s: Unit is free text with the suggestions "per second" to "per year", and a rate line reads the first of them anywhere in it. D3's select is void | S16b, S25 |
| D98 | Game of Life's Clear button | final `SPEC.md` 7.7 and 16, NOTES.md "Left out", against the author-file example (`set_everywhere`) | The final `SPEC.md`'s: Clear is a Run once button whose card holds "Become Dead from any state, always". `set_everywhere` stays in the schema for the ground, and no grid control writes it | S1a (the starter), S16b (the rebuild) |
| D107 | The Charts tab's empty text and start values | round D D5, `CHANGES-D.md` D5, final `SPEC.md` 10.8, updating D87 | **Plan's choice** within D87, taking the final `SPEC.md`'s words and its start dots. The text for a sheet with no measure is "Tick Plot beside a state under Measures to plot the number of cells in it." | S9 (the point), S16b (the text) |
| D111 | Starts as numbers | round D D8 part 2 | Plain text fields with "Use a parameter" beside them, committing on Enter or on losing focus as one edit and one undo step, with the red messages of the wording table, which are no step | S16a, S16b (the undo step) |

## Review

The review found one must-fix, one should-fix and six nits, and every finding held.
A shading darkest at the higher value lowered to a world of one colour with no problem reported, since `view.ramp` takes band 0 whenever `high <= low`.
`shade` now reverses the palette and swaps the ends, so the ramp still runs from dark at `darkest` to light at `lightest`, and the hued shading test holds the reversed case.
The reversal was taken over a refusal, because dark at the higher value is a natural thing for a researcher to want.
Undo and Redo of a purpose or a name edit restarted the world with the notice, while the edit itself had not.
`restore` now compares the two documents without their starts, name and purpose (`without_starts_or_words`), and the restart test holds an Undo and a Redo of a purpose past the start.
The Sheet pane refuses a darkest or lightest value equal to the other end in the field, with a new message listed under "Words added", and the fields show 0 and 1 for ends a hand-written rule leaves out, as the lowering reads them, where they showed 0 for both.
The undo keys now also wait while any modal of the app is open, read from egui memory's top modal layer of the previous frame, and the test harness draws the About modal to hold it.
The rebuild test walks the whole history with Cmd+Z and Shift+Cmd+Z as well as with the buttons.
The trailing "which" clauses in the docs of `dialog_open`, the sheet module, `without_card_lines` and `align_with_starter` became second sentences, and the paragraph of `agents/henad-graph-sheet-lower.md` was rewrapped.
The four UI words the review named are listed under "Words added", beside the new message.
The split of the topic-file text and the section tests between the two code commits was left as it is: the section tests use the toolbar's Undo and the text history from the second commit, so moving them would need a rewrite to keep the first commit building.
The agents/henad-app-sheet.md text and the tests for the code of `feat: edit parameters, measures, colours and buttons [draft]` therefore landed in `feat: model name, undo and redo in the sheet editor [draft]`.
After the fixes, fmt, clippy with `-D warnings` over henad-graph and henad-app, rustdoc with `-D warnings` over both, and the wasm32 typecheck of both pass.
With `HENAD_REQUIRE_GPU=1`, the henad-graph library has 433 passed and 1 ignored, its doc tests 12 passed, and the henad-app library 422 passed, with every other target passing.
The app checks ran again on the secondary display under the lock, from the tree only, since the app painted no frame for a screenshot.
On a Blank model with a Number property Level, typing 0 into "Lightest at value" showed "Darkest and lightest need different values." under the row and kept the rule, 5 in "Darkest at value" committed with no problem shown, and 1 back in the lightest field cleared the message.
With the About modal open, Shift+Cmd+Z left the sheet alone, and after Close it redid the edit.
An open menu is no modal, and Cmd+Z under the About menu still undid.
The restart rule for an undone purpose was not repeated in the app, and the test holds it.

## Benchmark commands left for the maintainer

None: S16b changes no engine, no shader and no hot loop. The shading's palette and ramp bounds are uniform constants of an existing view pass.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
