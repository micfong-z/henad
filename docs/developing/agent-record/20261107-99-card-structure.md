---
date: 2026-11-07
title: "Card structure"
description: "S22 of the #37 redesign. The Card pane gains When blocks that nest, each ending with \"+ Add inside line N\", moves that follow the outline, Draw new numbers, the \"otherwise\" notice, and the notes the relations between lines decide: gap, coverage, order, the notes inside a When and the offers to fold one. The forest fire is rebuilt from Blank with line 3 reading \"otherwise (when line 2 does not apply)\", and the Editor guide is rewritten with sheets and cards first."
icon: material/file-tree-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s22-card-structure on bdf64d4 with no rebase needed: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: bdf64d4
delta_state: "committed on `37-node-editor` by fast-forward: `feat: when blocks, moves and otherwise on the card [draft]`, `docs: editor guide for sheets and cards [draft]`, `fix: s22 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Card structure

> S22 gives the card its structure.
> "+ When, for several lines" adds a When line, and every When block ends with a row "+ Add inside line N" whose menu lists every kind of line, so Whens nest and inner blocks close before outer ones.
> A change added inside a When starts "always", a Become there from the state the nearest When tests, with a grey note naming the condition it holds under.
> The line menu moves a line up and down along the outline, draws new numbers and deletes, and a line moved above its result is skipped until it moves back.
> The first time a line starts with "otherwise" a notice says why, and the gap, coverage and order notes, the notes inside a When and the offers to remove a When or move its condition into its one change are computed in henad-graph and drawn under their lines.
> The forest fire is rebuilt from Blank by accessible names and equals its starter, and the Editor guide now describes sheets and cards first, then node graphs, then grid rules.

## State before

`37-node-editor` at `bdf64d4`, after S19 (card lines written with blanks, pickers and chips) and S26 (agents and the ground on the Sheet pane).
The Card pane wrote the lines of a cells card at its end, but had no When button, no add row inside a block, and a line menu holding Rename and Delete alone.
The wording table (S4) already held every word of the When blocks, the moves and the notes, and the edit commands (S12) already held `AddLine` inside a block, `MoveLine`, `DeleteLine`, `MoveConditionInto` and `DrawNewNumbers`.
Nothing computed the gap, coverage, order or When notes.

## What was done

**Relation notes (henad-graph).**
`CardRelations::notes(sentences, runs)` returns the card's `RelationNote`s, each with the line it sits under, its words and a `RelationNoteKind`, a line's notes in the wording table's order:

- `Gap`: two or more top-level Become lines of one property from one start state, each testing one Count with a compare or outside condition, with two or more new states, and the counts from 0 to 8, 4 or 9 that none passes.
- `Coverage`: every Become line of a States property at the top level, running, with a start state, testing one Count or holding always, when each value of the count gives one end state whatever the cell's own state, and two or more end states appear.
- `Order` (D109): two or more Become lines of a property, at any depth, all running and starting from distinct named states.
- `WhenRepeats` and `OnlyOneInside`: the fold offers, never on a When holding a When.
- `Inside` and `NothingInside`: the note under a change, When or result line inside a When, and under an empty When.

`sentence::when_repeats_note` takes the numbers of the lines inside ("Lines 3 and 4 already say from Alive"), as the final editor's page writes it, and `only_when_note` takes an optional condition, so a When with a blank condition reads "Only when line 4 holds.".
`AddButton::group` names the groups of the add-inside menu, and `check::holds_draw` is public for the card's Draw new numbers.

**The Card pane (henad-app).**
- "+ When, for several lines" joins the add buttons of a card that runs each tick, and a card holding a When shows "These add a line at the end of the card." above them.
- `closing_blocks` finds the blocks that close after each line, inner ones first, and `add_inside_ui` draws each "+ Add inside line N", named "Add a line inside line N", with a menu of the groups Change, When, Chance, Look around, Yes or no and Calculate that adds at the end of the block.
- The condition arrow follows "When" too, and "Choose when" on a When line reads "When ..." with no "always", while a line inside a When leads with "Line 4 already gives a condition. Choose always to use only that one.".
  `offers_condition` keeps "at rate" out of every "Choose when", a button's card included.
- "Line 2 actions" holds Rename, "Move line 2 up", "Move line 2 down" (disabled at the outline's edges), "Draw new numbers" on a line that draws, and Delete, whose tooltip on a When reads "Delete. Lines inside stay.".
- `explain_otherwise` keeps the ids of lines whose "otherwise" has been explained, counting every such line of a newly opened model, and shows "Line 3 now starts with otherwise. ..." above the cards the first time a line gets it, until the next card edit.
- `card_notes` appends the relation notes after the existing ones: grey, or amber with "Remove this When" and "Move it into line N", each one edit.

```text
crates/henad-graph/src/sheet/
├── relations.rs           # RelationNote, RelationNoteKind, CardRelations::notes and its five note kinds
├── sentence.rs            # AddButton::group, when_repeats_note over line numbers, only_when_note with an optional condition
└── check/                 # holds_draw public
crates/henad-graph/src/tests/
├── sheet_check.rs         # the order, coverage, gap, fold and inside notes
└── sheet_sentence.rs      # the plural repeats note and the bare inside note
crates/henad-app/src/ui/editor/sheet/
├── card_pane.rs           # When button, add-inside rows, line menu moves and Draw new numbers, otherwise notice, relation notes
└── picker.rs              # Choose when on a When line, the lead inside a When, offers_condition
crates/henad-app/src/tests/
└── sheet_card.rs          # forest fire rebuild, run once picker, nested When, the remove offer, moves that skip
docs/guide/editor.md       # sheets and cards first, then node graphs, then grid rules
docs/assets/app/
├── editor-sheet.png       # new: the Sheet and Card panes with the otherwise notice
└── editor-when.png        # new: a nested When with its add rows and notes
agents/henad-app-card.md, agents/henad-app-sheet.md, agents/henad-graph-sheet.md, AGENTS.md (topic table row)
CHANGELOG.md
```

**Tests.**
henad-graph: `the_order_note_shows_on_game_of_life_and_sir_and_not_on_the_vote_or_the_forest_fire` (and goes when a line is skipped), `the_vote_is_covered_and_a_gap_between_two_thresholds_is_named` (the vote's coverage note, and "None of lines 2 and 3 applies when B in my block is 4." with line 3 at most 3) and `a_when_offers_to_fold_into_its_lines_and_notes_what_holds_inside_it` (both offers, a result inside, an empty When, and no offer on a When holding a When).
henad-app: `a_blank_model_builds_the_forest_fire_with_otherwise_and_equals_the_starter` (the sheet from Blank, the five lines, the notice shown once, Draw new numbers on line 3, equal canonical text and lowered code after `align_with_starter`, the words rule over every menu, picker and note opened), `the_run_once_condition_picker_offers_no_rate`, `a_nested_when_stores_its_parent_and_deleting_the_outer_when_keeps_its_lines` (two Whens and a Become through the add rows, depths 0, 1 and 2, the inside notes and the lead, Delete keeping the lines one level up, Undo restoring it and walking back to the starter), `a_when_that_repeats_its_lines_offers_to_remove_itself` and `moving_a_line_above_its_result_skips_it_and_moving_it_back_runs_it`.

**UI check.**
The check ran under the lock of plan section 2.4, from the worktree, with the window on G15F1, through the egui MCP server.
The screen was unlocked, so every step has a screenshot (copies kept outside the repository).
- New model, Blank, a States property Cover with Empty, Tree and Burning, and the Editor widened by dragging its divider.
- Line 1 "Burning neighbours = count of the 4 cells around me whose Cover is Burning", line 2 "Become Burning from Tree, when Burning neighbours is at least 1", and line 3 from "+ Chance", Burning from Tree.
  As soon as line 3's old state was chosen it read "otherwise (when line 2 does not apply) become Burning from Tree, with chance ..." and the notice "Line 3 now starts with otherwise. Line 2 also changes Cover for a cell that is Tree. When both apply, line 2 wins." stood above the card (`henad-s22-otherwise-notice.png`).
  The chance 0.001 typed into its picker took the notice away.
- "+ When, for several lines" showed its heading and the item with its grey pattern, and the new When's "Choose when" opened by itself with "When ... is ...", "When ... is at least ...", "When ... is at most ...", "When ... is less than ... or more than ..." and "When with chance ...", and no "always".
  "When my Cover is Empty:" took the arrow after "When", "Nothing inside yet." and "+ Add inside line 4".
- The add-inside menu "Add a line inside line 4" listed Change, When, Chance, Look around, Yes or no and Calculate with their patterns (`henad-s22-add-inside.png`).
  Its When gave line 5 "When with chance 0.01:", and "+ Add inside line 5", Become, Tree gave line 6 "Become Tree from Empty, always", the old state taken from line 4.
  Line 5 showed "Only line 6 is inside. Its own when can hold this condition." with "Move it into line 6" and "Only when line 4 holds: my Cover is Empty.", line 6 "Only when line 5 holds: with chance 0.01.", and the rows "+ Add inside line 5" then "+ Add inside line 4" (`henad-s22-nested-when.png`).
- "Line 2 actions" listed "Move line 2 up", "Move line 2 down" and "Delete line 2" (`henad-s22-line-menu.png`).
  "Move line 2 up" put the Become first, its Burning neighbours chip red and "Uses Burning neighbours, which is below this line" under it (`henad-s22-move-skips.png`), and "Move line 1 down" put it back.
- "Move it into line 6" removed line 5 and gave its chance to the Become, after which line 4 offered "Line 5 already says from Empty, so this When repeats it." with "Remove this When" (`henad-s22-moved-back-folded.png`), which removed the When and left "Become Tree from Empty, with chance 0.01" at the top level.
- Play ran the preview past tick 158000, the cells turning from Empty to Tree to Burning (`henad-s22-running.png`).
- The app was quit, `app.ron` restored from its backup and the lock removed.

## Deviations

- **henad-graph changes.** The plan places S22 in `card_pane.rs`, but the gate's section 11 puts the gap, coverage and order notes in `sheet/relations.rs`, and the fold and inside notes were computed beside them so that the card draws one list.
  `when_repeats_note` and `only_when_note` changed signature, both added in 0.4 and never released.
- **No rate items.** The add-inside menu's Chance group shows "Become, with chance" and "With chance" only, and no "Choose when" offers "at rate", since the card writes no rate line until S25.
  The gate's "Chance (all four items)" waits for S25.
- **The notice stays until the next card edit.** The final editor shows it as a toast for seven seconds.
  The app has no toast, and the notice takes the place of the restart notice's form, an information icon and a line, drawn above the cards.
- **Screenshots from the running app.** The plan asks for the guide's screenshots to be taken headless.
  No headless renderer of the app exists, so the two images in `docs/assets/app/` are crops of the UI check's screenshots on the secondary display.
- **The forest fire in the UI check.** The check built lines 1 to 3 and the Whens of a smaller model rather than the whole starter, whose rebuild from Blank, with its parameters, measures and button, is held by the harness test.
- **Plan wording.** The plan's topic file for the card is `agents/henad-app-sheet.md`.
  S19 moved the Card pane to `agents/henad-app-card.md`, which takes S22's text, and `agents/henad-app-sheet.md` keeps its pointer.
- **Three accessible names follow D112 over the gate's 10.1 and 10.9 rows.** The gate names the add row "Add a line inside line N", "Remove this When" "Remove line N and keep the lines inside" and "Move it into line M" "Move the condition of line N into line M", which do not hold the buttons' visible words, against D112's label-in-name rule.
  D112 is the later decision and binds every control, so the add row is named by its label "+ Add inside line N", like the other add buttons, and the two note buttons take the line prefix before their words, "Line N: remove this When and keep the lines inside" and "Line N: move it into line M", the form of S19's "Line N: use is more than 0 instead".
  "Add a line inside line N" stays the menu's title and dialog name.
  The maintainer may prefer to keep the gate's words as AccessKit descriptions.

## State after

The card writes When blocks, nests them, moves lines along the outline and folds a When into its lines, and the notes the final editor computes from the relations between lines show under their lines.
The forest fire is rebuilt from Blank with "otherwise", and the Editor guide describes the sheet first.
Rate lines (S25), agent lines (S28) and the Diagram (S27) are still absent.
`37-node-editor` holds the three unsigned commits `b891941`, `03bfc2d` and `c6e4b77`, fast-forwarded with no merge-check fix.

### Merge check

- The branch already sat on the tip of `37-node-editor` at `bdf64d4`, so the merge rebase replayed nothing and raised no conflict.
  The rebase brought in no henad-app change, so the UI check was not repeated.
- The full check then ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

- **"otherwise" against a skipped line.** After "Move line 2 up" the skipped Become was line 1, and line 3 read "otherwise (when line 1 does not apply)" against it.
  The rule is structural and ignores whether the higher line runs, as the final editor's page does.
- **Notes wrap to the card's left edge.** A note longer than the card's width wraps under the line numbers, as "condition." did under line 5 of the check, since a wrapped row of `horizontal_wrapped` starts at its left.
- **The optional "for each ..." blank leaves a gap.** On a When with a chance, the unpainted blank (D99) takes its space before the ":", which reads as a stray colon at the end of the row.
- **The add-inside menu is tall.** With every group it runs past 500 points in a 960-point window, and a When near the bottom of the card opens it partly off screen.
- **Move menu items stay drawn when disabled.** The first line shows "Move line 1 up" disabled rather than hidden, as the final editor hides its arrows only on hover.

## Review

A reviewer with a fresh context found one "should fix" and seven nits, and all eight were fixed in `c6e4b77` (`fix: s22 review findings [draft]`).
The label-in-name check `assert_controls_hold_their_words` now also reads every note button and add row through the test-only `card_pane::card_buttons`, and runs in the nested When test (before and after line 6, which covers "Move it into line 6") and in the fold-offer test, besides the forest fire.
The three names that broke D112 were renamed, as the deviation above says.
The "otherwise" notice now covers every card, a button's card included, and opens with the line prefix ("Clear line 2 now starts with otherwise. ..."), since `sentence::otherwise_notice` takes the prefix instead of a number.
`explain_otherwise` requests another frame when it sets a notice, which it does after the cards are drawn below it.
No test holds the repaint, because the harness always runs several frames, and the UI check below shows the notice within three frames of the edit.
`gap_notes` takes `runs` like the coverage and order notes, and `the_vote_is_covered_and_a_gap_between_two_thresholds_is_named` adds a vote whose two Become lines read the count from below them and get no gap note.
On a card that runs once, the add-inside menu offers the Change group alone (`add_inside_groups`), held by the new `a_button_card_explains_its_otherwise_and_its_when_offers_changes_alone`, which also holds the button card's notice and failed with the old filters.
The add-at-end note stays on such a card, since "+ Change" there does add at the end.
The guide, the CHANGELOG and the nested test's comment now say that a change inside a When with no condition starts always, and that a Become there takes the state of the nearest enclosing When testing "my [property] is [state]".
The topic-file paragraphs were rewrapped, the `line_menu_ui` doc and a test literal were brought under 120 columns, and the picker doc and `agents/henad-app-card.md` lost the colon and the "X and not Y" frame.

After the fixes `cargo fmt --check`, clippy over henad-graph and henad-app with `-D warnings`, `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-app --all-targets --all-features` (henad-app 449 passed, henad-graph 451 passed and 1 ignored), the henad-graph doc tests, the wasm32 typecheck, `cargo doc` with `-D warnings` and `zensical build` all passed.
The UI check ran again under the lock on G15F1 with Game of Life.
A Become from Dead added on Clear's card read "otherwise (when line 1 does not apply)", and the notice "Clear line 2 now starts with otherwise. Line 1 also changes Status for a cell that is Dead. When both apply, line 1 wins." appeared above the cards within three frames (`review-1-clear-otherwise-notice.png`).
A When on "my Status is Dead" showed its add row named "+ Add inside line 4", whose menu is the dialog "Add a line inside line 4", and a Become added from it brought the note "Line 5 already says from Dead, so this When repeats it." with the button named "Line 4: remove this When and keep the lines inside" (`review-2-remove-this-when.png`).
The app was quit, `app.ron` restored and the lock released.

## Decisions adopted for review

From the plan's section 6 and the gate's sections 7 and 14.6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D33 | A condition can be written in three places | `CHANGES-A.md` "Not fixed", F3 | **Plan's choice**, no recommendation in the source. Keep round 1's When blocks and change conditions, with the prototype's fix: a change inside a When starts at "always" and shows the inherited condition | S22 |
| D37 | The prototype's departures from round 1 and `SPEC.md` | `CHANGES-A.md` "Departures" (F1, F4, F13) | Kept: a model opens paused at tick 0, When blocks nest, every When block ends with its own add row | S13a, S16a, S22 |
| D58 | Line tools | the app recon | One always-drawn menu per line, never hover-only tools | S19, S22 |
| D91 | A "majority" item | `CHANGES-B.md` B19 and `CHANGES-C.md` C16, left to the maintainer | Not built. "is at least 5" on the 3 by 3 block says it, the gap note speaks when two thresholds leave a gap, and the coverage note confirms when they settle every count. Both notes are computed in henad-graph's `sheet/relations.rs` and drawn by the card | S22 |
| D100 | The prototype's departures after round B | `CHANGES-B.md` "Departures" | Kept as built: results scoped to their When, the fold offers, "always" shown on the line, among others | S19, S22, S24, S25 |
| D101 | The prototype's departures after round C | `CHANGES-C.md` "Departures" | Kept as built: the coverage note among others | S19, S21, S22, S25, S27 |
| D104 | The condition word on a line | round D D2, final `SPEC.md` 8.4 | Plain bold text, with a square chevron after it named "Line N: change the kind of condition" that opens "Choose when", on a When line after "When" | S19, S22, S25 |
| D108 | Tooltips on the add buttons and the Look around heading | round D D6, final `SPEC.md` 8.2 | "+ When, for several lines" takes "Give several lines one shared condition" | S19, S22, S25 |
| D109 | The order note | round D D7, final `SPEC.md` 8.10 item 9 | A grey note under the last Become line of a property whose Become lines all run and start from distinct named states, lines inside a When included. Game of Life and SIR show it under line 3, and the vote and the forest fire do not. **Departure** from round 1, which has no confirming note | S22 |
| D113 | The prototype's departures after round D | `CHANGES-D.md` "Departures" | Kept as built: the plain condition word with its arrow, the order note, among others | S1a, S16a, S19, S22, S25 |

**Words added.**
No word outside the wording table is shown, apart from the plural "Lines 3 and 4 already say from ...", which the final editor's page writes, and "Only when line N holds." for a When with a blank condition, the page's form without a condition.

## Benchmark commands left for the maintainer

None: S22 changes no engine, no shader and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
