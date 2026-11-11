---
date: 2026-11-07
title: "Sheet and card panes"
description: "S13b of the #37 redesign. The Card pane draws each line part by part from the sentence: chips coloured by what they refer to, dashed blanks, read-only selects, the grey always and the muted otherwise with its explanation. Notes of the check sit under their lines, a result used above its line turns red, and hovering otherwise or a result chip highlights the line it names. Every sheet section and the lines have a help popover, and the panes, lines and controls carry the accessible names of the final specification."
icon: material/card-text-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s13b-sheet-card-panes after its merge rebase onto 717c3b5 (S17 and S23 merged): three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test, and the egui MCP checks were repeated on the rebased branch"
baseline_commit: 2bed719
delta_state: "committed on `37-node-editor` by fast-forward: `feat: sentence parts for the card [draft]`, `feat: sheet and card panes with chips and help [draft]`, `fix: s13b review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Sheet and card panes

> S13b finishes the display half of the sheet editor.
> henad-graph's `Sentences` now writes a line as parts (`SentencePart`): words, chips, blanks, selects, "always", "otherwise" and the empty "for each ..." blank, and the plain sentence is those parts joined.
> The Card pane draws each part as its own control: a chip coloured by what it refers to, a dashed blank that names what it wants, a read-only select, the grey "always" and the muted "otherwise (when line 2 does not apply)" with its explanation as tooltip and AccessKit description.
> Notes of the check sit under their lines, a result used above its line turns red with its tooltip, and hovering "otherwise" or a result chip highlights the line it names.
> Every section of the Sheet pane and the card's lines have a "?" help popover with the wording table's text and example.
> The panes, the line lists and every line are named AccessKit groups ("Line 2: ..."), every control's name holds its visible words, and a test keeps every forbidden word out of sheet mode.

## State before

- The branch was cut from `37-node-editor` at `2bed719`, with S13a (sheets in the app) merged.
- The Card pane drew each line as one plain label from `Sentences::line`, the Sheet pane drew its sections under plain headings, and every problem of the check sat in the list under the panes as `<line>:<column> <message>`.
- `Sentences` wrote a line as a string only, so nothing could tell a chip from the words around it.
- The baseline of the full check is the gate's: `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372, and the workspace tests on `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone.

## What was done

### Sentence parts in henad-graph

`crates/henad-graph/src/sheet/sentence.rs`:

- `SentencePart` (`Text`, `ConditionWord`, `From`, `Chip`, `Blank`, `Select`, `Always`, `Otherwise`, `ForEachBlank`), `SentenceChip` (a `ChipKind`, a grey `lead` such as "my", and a `name`), `ChipKind` (`Literal`, `Parameter`, `Property`, `State` with its property, `AnyState`, `Result`, `Relation`) and `sentence_text`.
- `Sentences::line_parts(line_id, otherwise)` returns a line's parts, and `line` joins them, so the writer's comments and the card come from one builder.
  Every private writer of `Sentences` (`change`, `condition`, `phrase`, `compare`, `around`, `formula` and the rest) now returns a private `Parts`, which merges adjacent words, and `slot`, `value` and `result_words` keep their string signatures.
- Each blank names what its slot wants, as the checker names it: the old state of a Become is `Blank::OldState`, an empty condition `Blank::When`, the right side of a comparison a number, a state or a list by its comparison.
  The comparison, the neighbourhood, total or average, the rate's unit and the operator of a one-operator Calculate are `Select` parts.
  A chance or rate with no "for each" ends with `ForEachBlank`, which reads as nothing in plain text.
- Every existing sentence test passed unchanged after the rewrite.
  New tests in `src/tests/sheet_sentence.rs`: `the_parts_of_a_life_line_name_each_chip_select_and_word`, `otherwise_blanks_and_the_empty_for_each_are_parts_of_their_own` and `no_line_of_a_test_sheet_holds_two_adjacent_word_parts`.

### The Card pane

`crates/henad-app/src/ui/editor/sheet/card_pane.rs`, rewritten:

- Each card shows its heading, its line count ("3 lines", "1 line") and, for a card that runs every tick, the Lines help and the law in a quiet box.
- `card_lines(document, sentences, report, card_index)` returns a `CardLine` per line: number, depth, prefix ("Line 2", "Clear line 1"), sentence, parts, the ids and tooltip of its "otherwise", its red chips, the swatch colour of each state chip, and its notes.
- Notes are the report's diagnostics at that line, the skip note first: red for an error or for a skip note about a red chip, amber otherwise.
  The list under the panes keeps every problem the card does not draw, such as the errors of a line table no lines list names.
- A result chip whose line sits at or below its user, or whose When does not hold its user, is red, with "Alive neighbours is below this line. Move this line under it, or choose another value." or the out-of-scope tooltip, as the checker decides the skip.
- Chips: results fill blue 900 with blue 200 text, a red chip red 900 with red 200, the others gray 800 with gray 100, a parameter with a slider mark, a state with its swatch and a property with a grey "my".
  Blanks are dashed gray outlines with their word, selects outlined with a chevron and disabled.
- "otherwise" is a muted label that takes focus, named by its words, with the tooltip "Line 2 also changes Cover for a cell that is Tree. When both apply, line 2 wins." as tooltip and AccessKit description.
  Hovering or focusing it, or a result chip, highlights the named lines on the next frame in blue 950.
- The empty "for each ..." blank is laid out and named on every chance and rate, and painted only while its line is hovered or holds focus (D99).
- Each line is a group named "Line 2: " and its sentence, the lines a list named "Lines" or "Lines of Clear", and each pane a group named "Sheet" or "Card".
  egui gives the node of a `Ui` no bounds, so `accessible_group` in `sheet/mod.rs` sets them.

### Help popovers and the Sheet pane

- `crates/henad-app/src/ui/editor/sheet/help.rs`, new: `Help` (a sheet section or the lines) and `help_button`, a borderless "?" named "Help: World" or "Help: lines" that toggles a popover with the heading, the text, "Example:" with the example and Close help.
  The popover is a dialog named "World help", apart from the button's name, so a locator of the button finds one node.
- The Sheet pane's seven headings each carry their help button.

### Tests

`crates/henad-app/src/tests/sheet_editor.rs`:

- `every_line_of_the_life_starter_reads_its_sentence_by_accessible_name`: the four line groups of Game of Life, in order, equal "Line 1: Alive neighbours = count of the 8 cells around me whose Status is Alive" and the rest, and the tree holds the pane groups, the two line lists, chips, a select, "always" and the help buttons.
- `a_skipped_line_shows_its_note_under_it`, `a_checker_error_shows_at_its_line` (an unknown old state, shown once, under line 2), `a_result_used_above_its_line_is_red_with_its_tooltip`, `chips_name_the_property_when_two_share_a_state_name`, `otherwise_carries_its_partner_as_a_description` (forest fire line 3), `a_help_popover_opens_and_closes`.
- `the_sheet_editor_never_shows_a_forbidden_word`: the accessible texts (labels, values and descriptions) of every starter open, every help popover, a skipped and a red line, and the New model dialog, the New node graph control exempt (D96).
- `every_accessible_name_holds_its_visible_words` (D112): every control of every line of those sheets has its name in the tree, and every name but a select's holds its visible words, case and a trailing "..." aside.
- `the_world_tabs_show_no_forbidden_word_from_a_sheet` (D60): each starter's entry name, description, visible parameter labels, measures, buttons and the tick counter.
- The two S13a tests that read the card now read the new `card_lines` and the "Line N: " group labels.

### Checks in the running app

On G15F1 under the UI lock, the app run from the worktree with inspection, through the egui MCP server.
The screenshots stay out of the repository.

1. The first look at Game of Life showed the line count overlapping the card heading in the narrow Editor tab, since the count sat in a right-to-left layout. The header now lays the heading, the count and the help button left to right in one wrapping row, and the app was restarted.
2. `shot1-life-card.png`: New model, Create. The Card pane holds "Each tick, every cell", "3 lines", the law box, and lines 1 to 3 with the blue "Alive neighbours" chips, the "Alive" and "Dead" chips with their green and black swatches, the outlined "the 8 cells around me", "is" and "is none of the numbers" selects, and the chips "2" and "3". Clear's card reads "Lines of Clear: run once, every cell, when pressed", "1 line", and "Become Dead from any state, always" with "always" in grey.
3. `shot2-fire-otherwise-hover.png`: Forest fire, the pointer resting on line 3's "otherwise (when line 2 does not apply)". Line 2 is drawn on the blue highlight, the tooltip reads "Line 2 also changes Cover for a cell that is Tree. When both apply, line 2 wins.", and the dashed "for each ..." blank shows on the hovered line 3 and not on line 5. The tree held the "otherwise" label and the tooltip text. The egui MCP server shows no AccessKit description, and `otherwise_carries_its_partner_as_a_description` reads it from the tree headless.
4. `shot3-help-world.png`: Help: World opened a dialog named "World help" holding the World help text, "Example: One tick is 1 day" and Close help.
5. `shot4-life-lines-help.png`: Help: lines on Game of Life opened "Lines help" with the Lines text and its example. Close help closed it and the tree held no dialog after.

The app was quit, `app.ron` restored from this milestone's backup and the lock released.

### Docs and topic files

- `docs/guide/app.md`: the Sheets subsection says what chips, blanks, hovering and the help buttons do, and that a problem sits under its line.
- `agents/henad-app-sheet.md`: the Card pane, `CardLine`, notes, red chips, the part widgets, the help popovers, accessible names and the words rule, and the new tests (14,894 characters after the merge rebase).
- `agents/ui.md`: the chip fills as MCS mappings and the words rule's scope (5,461 characters).
- `agents/henad-graph-sheet.md`: `line_parts` and the parts tests (29,069 characters after the merge rebase onto S17, which moved the lowering into `agents/henad-graph-sheet-lower.md`).
- One CHANGELOG line under Added.

### Edited tree

```
CHANGELOG.md                                     sheet and card panes
agents/
├── henad-app-sheet.md                           card pane, help, accessible names, words rule
├── henad-graph-sheet.md                         line_parts
└── ui.md                                        chip fills, the words rule's scope
crates/henad-graph/src/
├── sheet/sentence.rs                            SentencePart, SentenceChip, ChipKind, line_parts, Parts
└── tests/sheet_sentence.rs                      three parts tests
crates/henad-app/src/
├── tests/sheet_editor.rs                        ten new tests, two updated
└── ui/editor/
    ├── mod.rs                                   line problems leave the list under the panes
    └── sheet/
        ├── mod.rs                               pane groups, accessible_group
        ├── card_pane.rs                         rewritten: chips, blanks, selects, notes, hover
        ├── help.rs                              new: the "?" popovers
        └── sheet_pane.rs                        help beside each heading
docs/guide/app.md                                chips, blanks, hover and help
```

### Deviations from the plan

- **The sentence parts live in henad-graph.** The plan draws the lines "from `sheet::sentence`", and chips need to know which words are a value. `Sentences` now writes parts and joins them for the plain sentence, a change to henad-graph that the plan does not list. One builder writes both forms, so the writer's comments and the card cannot drift apart.
- **The world tabs' check reads the entry.** D60 puts the text a sheet produces in the world tabs under the words rule. The test reads what the lowered entry declares (names, measures, buttons) and the tick counter instead of driving the world tabs, which the test harness does not draw.
- **The label-in-name test reads the card's controls.** The AccessKit tree holds a control's name but not the words it paints, so the test takes each control's visible words and name from `CardLine::control`, which the drawing uses, and checks that the tree holds that name.
- **Selects are drawn read-only.** The gate names the selects' accessible names for S13b and S19. S13b draws them as disabled combo boxes with their words as value, and editing them waits for S19.
- **No D80 to D112 row changed.** The gate's S13b scope holds as written, with the condition arrow left to S19 (D104).

### Review

A reviewer with a fresh context listed one must-fix, two should-fix and four nits, and every one held against the code and was fixed in `fix: s13b review findings [draft]`.
The must-fix was a regression: the list under the panes dropped every problem that named a line, but the card draws only the lines in its display order.
A line table that no lines list names, or one inside a block no line reaches, carries its errors at that line, and those errors showed nowhere.
`card_pane::draws_diagnostic` now says whether the card draws a problem, a card and a line in that card's display order, and `sheet_outcome_ui` lists every other one.
`an_error_at_a_line_the_card_does_not_show_stays_in_the_list` adds an unlisted `l9` to Game of Life and finds each of its two errors once.
The card and the help took a few words of their own: the "for each ..." blank, "from", the heading "Lines", the dialog names "World help" and "Lines help", and the line's group name.
`sheet::sentence` now exports `FOR_EACH_BLANK_WORDS`, `FROM_WORD`, `LINES_HELP_HEADING`, `help_dialog_name` and `line_name`, `SentencePart::From` reads `FROM_WORD`, and both wording tests hold the new entries.
Among the nits, each help button now owns its popover, since the popover's id derives from the button's, and `each_lines_help_button_opens_its_own_popover` fails on the old shared id.
An empty value on a Total or average line is now the "choose a number property" blank of wording table 10.5, held by `an_empty_total_or_average_value_is_a_number_property_blank`.
The label-in-name test also opens every help popover and the New model dialog and checks Close help, Create, Cancel and the starter radios, and the chip's doc comment lost its "yet".
The record no longer names the screenshots' folder, and the issue about splitting `agents/henad-graph-sheet.md` is struck, since S17 already split it.
The merge step refreshes the topic files' character counts with `wc -c` after the rebase.
The app was run again under the UI lock on G15F1: on Game of Life, Help: lines opened "Lines help" below its own button and Help: World opened "World help", and Close help closed each with no dialog left in the tree (`review1-lines-help.png`, `review2-world-help.png`).
Checks after the fixes: fmt, clippy with `-D warnings` over henad-app and henad-graph, `HENAD_REQUIRE_GPU=1 cargo test -p henad-app -p henad-graph --all-targets --all-features` (389 and 371 passed, 1 ignored), henad-graph's doc tests, rustdoc of both and the wasm32 typecheck of both.

## State after

- A sheet's card shows its lines as chips, blanks and selects with their notes under them, highlights the line that "otherwise" or a result chip names, and opens help from every heading.
- Checks run in the worktree: `cargo fmt --all -- --check`; `cargo clippy -p henad-app -p henad-graph --all-targets --all-features -- -D warnings -W clippy::all`; `HENAD_REQUIRE_GPU=1 cargo test -p henad-app -p henad-graph --all-targets --all-features` (387 and 370 passed, 1 ignored); `RUSTDOCFLAGS="-D warnings" cargo doc -p henad-graph -p henad-app --no-deps --all-features`; `cargo check --workspace --all-targets`; `cargo check -p henad-graph -p henad-app --all-features --lib --target wasm32-unknown-unknown`.
  The full check runs at the merge point.

### Merge check

- The merge rebase onto `37-node-editor` at `717c3b5` brought in S17 (agent sheets) and S23 (conversion), and conflicted in the second commit only.
  - `CHANGELOG.md` keeps both lines, S17's agent sheet lowering and S13b's sheet and card panes.
  - `crates/henad-app/src/tests/sheet_editor.rs` keeps both milestones' imports: S13b's `CardLine`, `LineNote` and `NoteColor` beside `card_lines`, and S17's `starter_choices` beside `new_model_modal`.
  - The other files S17 and S13b share (`agents/henad-app-sheet.md`, `agents/henad-graph-sheet.md`, `sheet/sentence.rs` and `src/tests/sheet_sentence.rs`) merged without a conflict.
- The full check then ran on the rebased branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other, with bans, licences and sources ok.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, with no new failing name and no aborted test binary in the log.
  No fix was needed.
- No baseline test passes now.
- The rebase brought in henad-app changes (S17's New model dialog offering the grid starters only, and S23's graph canvas), so the egui MCP checks were repeated under the UI lock on G15F1.
  New model and Create on Game of Life drew the card's four line groups by their accessible names, the disabled selects and "Clear line 1: choose when, now always".
  On Forest fire, the pointer on line 3's "otherwise (when line 2 does not apply)" highlighted line 2 and showed its tooltip, and the dashed "for each ..." blank showed on line 3 alone.
  Help: World opened the dialog "World help" with its example, and Close help left no dialog in the tree.
  The screenshots stay out of the repository.

## Issues found & future directions

1. A row of the Card pane can run a few points past the pane's right edge, so a short word at the end of a row ("from" on Clear's line) is clipped in a narrow Editor tab. The rows measure their width inside the scroll area, and the clip sits a little inside it. S16a's layout work on the panes is a natural place to look.
2. The Cells table still clips its last column in a narrow Sheet pane, as S13a noted.
3. The egui MCP server shows no AccessKit description, so "otherwise"'s explanation is read headless only. A screen reader reads it.
4. The highlight of a hovered "otherwise" or result chip draws one frame after the hover, from egui temp data. S19's editing may want the hover state in the card pane's own state instead.
5. The help dialog is named "World help" apart from its button "Help: World". The prototype names a menu's dialog by its heading, and S19's pickers can take the same pattern.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D57 | The primary action's colour | the app recon, `agents/ui.md` | MCS blue 700 and the app's dark look, in place of the prototype's orange | S13a, S13b |
| D60 | The words rule's scope | the app recon | The Editor tab in sheet mode and the text a sheet produces in the World tabs, not the Model, Performance and System tabs | S13b |

From the gate's section 7 and its round D addendum:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D96 | The words rule and the ways into the node graph | the final prototype spec, section 4 ("node" is forbidden), against D71 ("New node graph"), the expert design, section 5, and answer 8b | **Plan's choice.** The words rule exempts the controls that open or create a node graph: the "New node graph" menu in the Editor header and the File menu, "Convert to node graph", "Edit as node graph", the conversion modal, the converted copy's banner and the author model's line "Converted to node graph: X", as the final spec exempts the Facilitator's "Seed". Each forbidden-word test names these controls and checks every other text. Without it, S13b's test fails on the Editor header in sheet mode | S13a, S13b, S27 |
| D99 | The empty "for each ..." blank | round C C12, the final prototype spec, sections 3 and 8.8, beside D58 | **Plan's choice** within D58, which covers line tools and not blanks. The blank is laid out on every chance and rate and painted only while its line is hovered or holds keyboard focus, so its AccessKit node stays in the tree, as the page's opacity rule keeps it | S19 |
| D112 | Accessible names hold their visible words | round D D9, `CHANGES-D.md` D9, the final prototype spec, sections 3, 8.3 and 8.8 | Every control's accessible name contains its visible words, case and a trailing "..." aside (WCAG 2.5.3, label in name), and a control with no words of its own (the condition arrow, the line menu) is named by what it does. Three names change: the old-state blank is "Line N: choose the old state", with the picker title "Choose the old state", the "for each" prompt's button is "Line N: add for each infected neighbour", in the singular as it reads, and the grey "always" is "Line N: choose when, now always". An add menu item's grey pattern stays out of the accessibility tree, and the item's name is its label. S13b adds a test that every accessible name in the Editor tab in sheet mode holds its control's visible words, beside the forbidden-word test, and each later milestone that runs that test over its states runs this one too | S13b, S19, S25 |

S13b builds D99 on the read-only card, ahead of S19, since the gate's S13b scope draws the empty blank on hovered and focused lines.

## Benchmark commands left for the maintainer

None. S13b changes no engine path.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
