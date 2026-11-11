---
date: 2026-11-08
title: "Rates in the card"
description: "S25 of the #37 redesign. The Card pane writes rate lines: \"+ Chance\" and \"Choose when\" offer rates on a card that runs each tick, the unit select sets one tick when none is set, the grey rate note shows the chance per tick with the mean stay and the long tick, a fixed rate asks to grow for each count, a competing group reads \"or\" with a bracket and one group note, and the line menu changes a typed chance into a rate and back. The rates SIR is rebuilt from Blank and equals its starter."
icon: material/timer-sand
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s25-rates-in-card on 7ec4f90 with no rebase needed: two unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 7ec4f90
delta_state: "committed on `37-node-editor` by fast-forward: `feat: rate lines and time notices in the card [draft]`, `fix: s25 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Rates in the card

> S25 lets the card write rates.
> "+ Chance" gains "Become, at a rate" and "At a rate", "Choose when" offers "at rate ... per ..." on a card that runs each tick, and a button's card still takes chances only.
> A rate line shows the chance per tick it gives in grey, read from the parameters' defaults or from the values the world tries, with the mean stay as its description and a second note when the tick is long for the rate.
> A fixed rate below a count of the state it leads to asks "Should this rate grow with the number of Infected neighbours?" with a one-click fix, and a unit chosen while one tick has no length sets one tick with a notice.
> Two rate lines out of one state read "or", joined by a bracket, with one group note, and the line menu turns a typed chance into a rate and back.
> The rates SIR is rebuilt from Blank by accessible names and equals its starter in canonical text and lowered code, and in the app its epidemic rises and falls over a time axis in days.

## State before

`37-node-editor` at `7ec4f90`, after S9 (the time axis), S21 (rates lowering) and S22 (card structure).
The lowering ran rates, and `sheet::rates` held `rate_note`, `largest_count_chance`, `chance_note` and `count_as_draw`, while the wording table held every rate word.
The edit commands held `ChangeToRate`, `ChangeToChance`, the tick's unit on a new rate (D83) and a unit chosen by hand.
The card left the rate items out of every add menu, offered no "at rate" in "Choose when", had no options for a rate's unit select, and drew no rate note.

## What was done

**henad-graph.**
`sheet::rates` gains four functions the card reads:

- `conversion(document, line_id)`: the `DrawKind` a typed chance or rate can change into, `None` with no `[time]`, a chance from 1 up or a rate with no unit, and no rate on a card that runs once.
- `chance_line_numbers(document)`: the numbers of the lines that draw a chance on cards that run each tick, for the Time notice.
- `grow_count(document, line_id)`: the nearest open Count above a Become at a fixed rate with no "for each" (or a Rate line whose result a Become takes whole) that counts the cells around in the Become's new state, as a `GrowCount`.
- `parameter_rate_unit(parameter)`: the unit of time a parameter's Unit names, through `unit_in_text`.

`rate_note` now returns no note for an empty rate.
The UI check found it printing "a chance that grows with r, where r is ...." under a rate line whose rate was still blank.
`SentencePart::Competing` is the word "or" before a lower line of a competing group, which `sentence::competing_parts` puts in front of a line's parts with the change's first word in lower case.
The sheet writer's plain text never contains `SentencePart::Competing`.
Only the card adds it, through `competing_parts`.
`sentence::long_tick_note` joins the largest chance and `LONG_TICK`, and "+ Chance" takes its final tooltip "Change a state by chance or at a rate" (D108).

**henad-app, the Card pane.**

- The add menus and the add-inside menu list all four Chance items, and "Choose when" offers "at rate ... per ..." where the card runs each tick.
- The unit select lists "second" to "year" (`unit_options`) and writes `LineField::RateUnit`.
  When the edit gave a sheet with no `[time]` one, `CardState::tick_notice` shows "One tick is now 1 week. Change it on the sheet." above the cards until the next card edit, or until the Sheet pane changes the length or unit of one tick.
- A rate's picker leaves out a parameter whose Unit is set and names no unit of time, and a chance's picker a rate parameter.
- `card_notes` adds, in the wording table's order after the count-as-draw offer, the grey `rate_note` (its `mean_stay` as hover text and AccessKit description), the long-tick note "Up to 0.909 with 8. This tick is long for this rate. A shorter tick follows the rate more closely.", the chance note, and the amber grow offer with "Add for each infected neighbour" named "Line 2: add for each infected neighbour", which puts the count into `tries`.
  The unit mismatch stays the checker's warning, among the line's notes.
- `card_lines` starts each lower line of a competing group with "or", muted and focusable, with the tooltip and description "Lines 3 and 4 compete. A cell leaves Infected at the sum of their rates, and each line takes its share." and the group's other lines highlighted on hover.
  `bracket_ui` paints a gray 500 bracket for each group in the pane's margin, just left of the rows, and the group note sits under the highest line, where `rate_note` puts it.
- The line menu adds "Change to a rate" or "Change to a chance" where `conversion` finds one.
- `card_pane` takes the world's tried values (`SheetTrial`) and reads the notes from the defaults with those in their place (D62).

**henad-app, the Sheet pane.**
An edit that gives one tick a new length or unit, while a line on a card that runs each tick draws a chance, shows "Lines 2 and 3 use chances per tick. They do not follow the time scale." under the Time line until the pane's next edit (T9).
The line numbers are read from the sheet each frame, so a card edit that turns a line into a rate, or renumbers the lines, updates the notice, and the notice leaves once no line draws a chance.

**Tests.**
henad-graph `src/tests/sheet_rates.rs`: `the_card_offers_the_conversion_a_typed_draw_allows`, `the_time_notice_names_the_chance_lines_of_cards_that_run_each_tick`, `a_fixed_rate_below_a_count_of_its_new_state_asks_to_grow_with_it` (with an empty rate, a worked-out rate and a count moved below the line) and `a_lower_line_of_a_competing_group_reads_or` (with `parameter_rate_unit` over the SIRD parameters and `long_tick_note`).
henad-app `src/tests/sheet_rates.rs`, new, with the helpers of `sheet_card.rs` shared as `pub(super)`:

- `a_blank_model_builds_the_rates_sir_with_its_lines_and_equals_the_starter`, the study's T2: one tick of 1 day on the sheet, line 2 from "Become, at a rate" reading "at rate ... per day", "Infection rate" through "New parameter..." in "per day", line 2's "for each" from the grow offer, line 3 with "Recovery rate", New outbreak's "Choose when" with no rate, then `align_with_starter` against `sir.toml`, equal canonical text and lowered code, label-in-name and the words rule.
- `the_run_once_card_offers_no_rate_after_rates_arrive`: the notes "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour, and 0.451 with 2. ..." and "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. ...", the mean stay, "at rate ... per ..." on line 3, "Change to a chance" once line 3's rate is typed, and on New outbreak no rate in "Choose when" and no "Change to a rate" for a typed chance.
- `the_sird_lines_3_and_4_read_or_with_one_bracket_and_one_group_note` (on a copy of henad-graph's `sird.toml`).
- `change_to_a_rate_and_back_keeps_the_chance_per_tick` (the chance SIR with a typed 0.05, the Time notice, the rate's note at 0.05, and 0.05 again).
- `the_long_tick_note_shows_at_one_day_and_leaves_at_a_quarter_day`.
- `removing_the_time_unit_marks_the_rate_lines_and_a_rate_unit_sets_it_again` (two "Not run yet" notes, then "week" on line 3 setting one tick of 1 week, its notice and the mismatch warning).

Each test records what its menus, pickers and notes show and holds the words rule over them.
A label's AccessKit node holds a `TextRun` child with its text, so the counts leave text runs out.

**UI check.**
The check ran under the lock of plan section 2.4, from the worktree, with the window on G15F1 (1600 by 960 at -1800, 60), through the egui MCP server.
Screenshots are kept outside the repository.

- From New model, Blank: one tick set to days, Health with Susceptible, Infected and Recovered, line 1 a Count of Infected.
  "+ Chance" showed "Become, with chance", "Become, at a rate", and under "Or a yes or no that several lines share" "With chance" and "At a rate", each with its grey pattern (`1-chance-menu.png`).
- "Become, at a rate" gave line 2 "Become Infected from Susceptible, at rate [rate] per day".
  "New parameter..." opened with the Unit "per day" filled, and "Infection rate" 0.3, max 5, gave the note "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259. A rate is not a chance. ..." and the amber "Should this rate grow with the number of Infected neighbours?" with "Add for each infected neighbour" (`2-grow-offer.png`).
- The button gave "for each infected neighbour", the note with 0.259 and 0.451 and the formula, and "Up to 0.909 with 8. This tick is long for this rate. ..." under it (`3-line2-note-long-tick.png`).
- Line 3, Recovered from Infected with "Recovery rate" 0.1 per day, read "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. ...", and the order note "Lines 2 and 3 change different cells (Susceptible and Infected), so their order does not matter." followed (`4-line3-note.png`).
  Hovering the note showed "On average a cell stays Infected 10.5 days at this tick length. It approaches 1/0.1 = 10 days as the tick shrinks." (`5-mean-stay-tooltip.png`).
- With Health starting Infected with chance 0.01 and the three states plotted, 24 presses of Step brought the counter to "Tick 24 (24 days)".
  The Charts tab's x axis read "Day", Infected rose to about 4,000 near day 11 and fell to 1,936 by day 24 while Recovered reached 8,063 (`6-tick-24.png`).
  Run to tick stayed disabled for the preview, so Step stood in for it.
- "one step (no unit)" on the sheet put "Not run yet: set how long one tick is, under World on the sheet" under lines 2 and 3 (`7-no-unit-not-run.png`).
  Line 3's unit select listed "second" to "year" (`8-unit-select.png`), and "week" set "One tick is 1 weeks" on the sheet with "One tick is now 1 week. Change it on the sheet." above the card, line 2's note then reading "Each tick (7 days), ... 0.878 with 1 infected neighbour, and 0.985 with 2." (`9-tick-set-notice.png`).
- Before line 2's rate was chosen, its note read "... a cell changes with a chance that grows with r, where r is ...." (visible in the screenshot after the old state was chosen).
  `rate_note` was fixed to give no note for an empty rate, held by `a_fixed_rate_below_a_count_of_its_new_state_asks_to_grow_with_it`, and the check was not run again for it.
- The app was quit, `app.ron` restored from its backup and the lock removed.

The SIRD group's "or" and bracket, the Time notice under the sheet's Time line, and "Change to a rate" were checked by the harness tests only, since opening a sheet file goes through a native file dialog the MCP server cannot drive.

**Docs.**
`docs/guide/editor.md` gains a Rates section: chances against rates, the unit and the tick, the note and its hover, the long tick, the three amber notes, competing lines, the button's card, the Time notice and the conversions, and the line menu and the conditions list gain their rate items.

**Edited tree.**

```
AGENTS.md                                         Card pane row names rates
CHANGELOG.md                                      app card rates
agents/
├── henad-app-card.md                             Rates paragraph, pickers, menus, sheet_rates.rs tests
├── henad-app-sheet.md                            the Time notice
└── henad-graph-sheet.md                          conversion, chance_line_numbers, grow_count, Competing
crates/henad-app/src/
├── tests/
│   ├── mod.rs                                    lists sheet_rates
│   ├── sheet_card.rs                             helpers pub(super)
│   ├── sheet_editor.rs                           LineNote::description
│   ├── sheet_rates.rs                            new, eight tests
│   └── sheets/sird.toml                          new, a copy of henad-graph's
└── ui/editor/sheet/
    ├── card_pane.rs                              rate notes, grow offer, or and bracket, conversions, tick notice, trials
    ├── mod.rs                                    trials passed to the card
    ├── picker.rs                                 at rate offered, unit options, parameters by use
    └── sheet_pane.rs                             time_changed and the notice under the Time line
crates/henad-graph/src/
├── sheet/
│   ├── rates.rs                                  conversion, chance_line_numbers, grow_count, parameter_rate_unit
│   └── sentence.rs                               Competing, competing_parts, long_tick_note, "+ Chance" tooltip
└── tests/
    ├── sheet_rates.rs                            four tests
    └── sheet_sentence.rs                         the tooltip
docs/guide/editor.md                              Rates section
```

## Deviations

- **The topic file.** The plan names `agents/henad-app-sheet.md`, but S19 moved the Card pane to `agents/henad-app-card.md`, which takes the rate lines and notes, and `agents/henad-app-sheet.md` takes the Time notice.
- **henad-graph changes.** The plan places S25 in `card_pane.rs`.
  The rules for the conversion offer, the grow offer and the Time notice are sheet logic and sit in `sheet::rates` beside `count_as_draw`, and the "or" is a `SentencePart` so that a line's accessible name reads it.
- **Which parameters a picker lists.** The plan's "A rate blank lists only rate parameters, and a chance blank none" (rates T2) is softened.
  A rate's picker lists rate parameters and parameters with no Unit, since the final `SPEC.md` 8.9 puts a parameter with no unit into a rate, and a chance's picker leaves out rate parameters.
- **A parameter with no unit put into a rate keeps no unit.** The final `SPEC.md` 8.9 says it takes the line's unit.
  S12's review moved that rule to a unit chosen by hand, as the prototype applies it, and `a_parameter_with_no_unit_takes_the_rate_unit_chosen_by_hand` holds it, so S25 leaves it.
- **The long-tick words.** The gate gives "up to 0.91 with 8" and `LONG_TICK` without a form joining them.
  The card shows one grey note "Up to 0.909 with 8. This tick is long for this rate. A shorter tick follows the rate more closely.", the chance at three figures by S21's number rule.
- **The group note sits under the highest line** of a competing group, where `rate_note` puts it, rather than after the group's last line as rates section 4 draws it.
- **"or" yields to "otherwise".** A lower line of a group that a line outside the group also overrides reads "otherwise (when line N does not apply)" alone, and keeps its place under the bracket.
- **The Time notice** shows after any edit of the pane that changes the length or unit of one tick while the tick has one, and stays until the pane's next edit, rather than "once".
  Its line numbers follow the card's edits while it shows.
  It does not show when the tick is removed, since no time scale is left to follow.
- **The tick notice** shows after a card edit that gave a sheet with no tick one, a unit chosen on a line or a rate parameter put into a rate, until the next card edit or a Sheet pane edit of the tick.
- **No "or" in the final `SPEC.md`.** D1's "or", the bracket, the long tick, the mean stay and the conversions take the words of gate 10.21, untested by any round.

**Review.**
A reviewer with a fresh context listed nine findings, and `fix: s25 review findings [draft]` fixes eight and records the ninth.

- The Time notice kept the line numbers of the moment the time changed, so a card edit left it stale.
  `PaneState::time_notice` became `time_changed`, and the notice is worked out from `chance_line_numbers` each time it is drawn and hidden while the list is empty.
  `change_to_a_rate_and_back_keeps_the_chance_per_tick` now holds "Line 2 uses chances per tick ..." after "Change to a rate" and "Lines 2 and 3 ..." again after "Change to a chance".
- The tick notice stayed after the Sheet pane gave the tick another length.
  A Sheet pane edit that changes the length or unit of one tick now calls `CardState::drop_tick_notice`, and `removing_the_time_unit_marks_the_rate_lines_and_a_rate_unit_sets_it_again` sets the length to 2 and holds that the notice has gone.
- Two doc paragraphs in `sheet::rates` (`rate_note` and `conversion`) passed 120 columns and are broken between sentences.
- The rule for which parameters a picker lists had no test.
  The T2 rebuild now holds that line 3's rate picker offers "Infection rate" and New outbreak's chance picker offers "Initial infected" but neither rate parameter, and `a_rate_picker_leaves_out_a_parameter_of_another_unit_and_a_chance_picker_every_rate_parameter` adds a parameter with the Unit "cells" and one with no Unit to the SIR sheet and compares both pickers' Parameters groups.
- D62 had no test.
  `the_rate_notes_read_a_tried_value_and_the_default_after_reset` opens `sir.toml`, waits for its preview, tries 0.5 for Infection rate through `apply_trial`, holds that line 2's note reads 0.393 and 0.632, and holds the default note again after Reset.
- The doc of `LineContext::parameters` and the Rates paragraph of `agents/henad-app-card.md` said a rate's picker leaves out a parameter whose Unit names no unit of time, which also covers a parameter with no Unit, which the code keeps.
  Both now say "whose Unit is set and names no unit of time".
- The record's "The writer's comments do not use it." names its subject now.
- `chance_line_numbers` joins the line numbers of several cards that run each tick.
  The fix needs new words, so it is listed under the issues below.
- The SIRD "or", the bracket and the group note had not been seen in the running app.
  The repeat UI check below found the bracket drawn across wrapped text, and `bracket_ui` now draws it in the pane's margin, `BRACKET_OFFSET` (5 points) left of the rows, with its clip rectangle widened to reach the margin.

**Review UI check.**
Under the lock of plan section 2.4, from the worktree, with the window on G15F1 (1600 by 960 at -1800, 60) and the Editor widened by dragging the dock separators, starting from the SIR epidemic of New model.

- Health gained Dead in its property form, and "+ Chance", "Become, at a rate" with Dead from Infected gave line 4.
  Line 4's rate picker listed Infection rate, Recovery rate and Initial infected, which has no Unit.
  After typing 0.05, line 4 read "or become Dead from Infected, at rate 0.05 per day", one group note under line 3 read "Each tick (1 day), an Infected cell leaves Infected with chance 0.139: Recovered 0.0929, Dead 0.0464. A rate is not a chance. Henad turns it into one as 1 − e^(−(0.1 + 0.05) × 1 day).", and neither line had a note of its own.
  The bracket, then drawn beside the numbers, crossed the wrapped words "per", "Recovered" and "for each" (`review-1-sird-or-bracket.png`).
- After the bracket fix and a restart, the same steps gave the bracket in the margin left of lines 3 and 4, clear of every wrapped row (`review-2-sird-bracket-in-margin.png`).
- Line 3's rate typed as 0.1 and "Change to a chance" gave "with chance 0.095162585", and one tick of 2 days on the sheet showed "Line 3 uses chances per tick. They do not follow the time scale." under the Time line (`review-3-time-notice-line-3.png`).
  Line 4 then read "otherwise (when line 3 does not apply)", with the S22 notice "Line 4 now starts with otherwise ...".
- "Change to a rate" on line 3 gave "at rate 0.05 per day", the rate that keeps 0.0952 per tick of 2 days, and the Time notice left at once, as the group note returned with 0.181 (`review-4-time-notice-gone.png`).
- "one step (no unit)" on the sheet, then "week" in line 3's unit select, set one tick to 1 week with "One tick is now 1 week. Change it on the sheet." above the card (`review-5-tick-notice.png`).
- A length of 2 on the sheet removed that notice (`review-6-tick-notice-gone.png`).
- The app was quit, `app.ron` restored from its backup and the backup deleted, and the lock removed.

## State after

The card writes and explains rate lines, and the rates SIR is built from Blank in the app.
Agent lines (S28), the Diagram (S27) and fire counts beside each line (S31) are still absent.
`37-node-editor` holds the two unsigned commits `81b5187` and `7268131`, fast-forwarded with no merge-check fix.

### Merge check

- The branch already sat on the tip of `37-node-editor` at `7ec4f90`, so the merge rebase replayed nothing and raised no conflict.
  The rebase brought in no henad-app change, so the UI check was not repeated.
- The full check then ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

- **The card pane is narrow.** In a 1600-point window the card is about 150 points wide, and a rate note wraps over ten rows.
  Dragging the divider between the Sheet and Card panes did not move it in the check.
- **Run to tick is disabled for the preview.** The Playback tab's Run to tick stays greyed for a sheet's preview, so reaching tick 24 took 24 presses of Step.
- **A competing group's note** names no line, and a group whose lines are far apart shows its note under the first line alone.
- **A parameter's unit and the rate's.** With the S12 rule, a parameter with no unit put into a rate stays unitless, against the final `SPEC.md` 8.9, and the maintainer may want the page's rule.
- **The picker's parameter filter** hides a parameter whose Unit is "cells" from a rate, which a researcher who wrote a free-text unit might not expect.
- **The "or" after a skipped higher line** of a group follows the structural rule, as "otherwise" does (S22).
- **The Time notice over several cards.** `chance_line_numbers` joins the line numbers of every card that runs each tick into one list, and line numbers are per card.
  A world with two such cards, such as an agents card beside a ground card, can show "Lines 2 and 2 use chances per tick", or name lines without their card.
  Naming the card needs words the wording table does not hold, so the notice is left as it is for the maintainer.
- **"Up to 1 with 8".** At a tick of 1 week, line 2's long-tick note reads "Up to 1 with 8", since 1 − e^(−0.3 × 8 × 7) rounds to 1 at three figures.
  A chance that rounds to 1 might read "almost 1" instead.
- **A competing group in two units has no note.** With line 3 at a rate per week and line 4 at a rate per day, neither line nor the group shows a note, by `rate_note`'s rule that a group's note needs one unit.

## Decisions adopted for review

From the plan's section 6 and the gate's sections 7 and 14.6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D1 | Competing rate changes out of one state | `rates/rates.md` section 10, question 1 | (a) Competing risks, written "or" with a bracket, an exception to answer 2a | S5, S21, S25 |
| D2 | Long ticks | `rates/rates.md` section 10, question 2 | (a) A grey note when the chance per tick at the largest count is above 0.5, stating that chance | S21, S25 |
| D3 | The Unit column | `rates/rates.md` section 10, question 3 | (a) A select for rates ("per second" to "per year"), free text otherwise. Void under D89 | S16b, S25 |
| D14 | Where a rate's unit lives | `rates.md` section 3 against `author-file.md` question 8 (a) and `CHANGES-A.md` F8 | **Conflict.** The line holds its unit, takes its parameter's unit when one is put in, and warns on a mismatch. Both are stored and the line's governs | S1a, S21, S25 |
| D62 | Which values a rate note reads | recon `henad-app.md` | The defaults while no run of the sheet is loaded, the live values otherwise | S25 |
| D83 | A rate's unit taken from the tick | round C C5, final `SPEC.md` 8.3, 8.9 and 12 | **Plan's choice** of the key `rate_unit_from_tick`. A new rate takes the tick's unit when one tick has a length, a parameter put into the rate replaces it, a unit chosen by hand clears the mark | S1a, S4, S12, S25 |
| D89 | The parameter Unit | `CHANGES-A.md` F8, `CHANGES-B.md` B6, final `SPEC.md` 7.4 and 8.9 | **Conflict.** The final `SPEC.md`'s: Unit is free text with the suggestions "per second" to "per year", and a rate line reads the first of them anywhere in it. D3's select is void | S16b, S25 |
| D100 | The prototype's departures after round B | `CHANGES-B.md` "Departures" | Kept as built: the optional "for each ..." blank, results named after what they draw on, among others | S19, S22, S24, S25 |
| D101 | The prototype's departures after round C | `CHANGES-C.md` "Departures" | Kept as built: the plain-words rate note, the tick's unit on a new rate (D83), among others | S19, S21, S22, S25, S27 |
| D104 | The condition word on a line | round D D2, final `SPEC.md` 8.4 | Plain bold text, with a square chevron after it named "Line N: change the kind of condition" that opens "Choose when", after "at rate" too | S19, S22, S25 |
| D106 | The note on a worked-out rate | round D D4, final `SPEC.md` 8.9 | Plain numbers for a fixed number times a count, words first for any other worked-out rate. Drawn by the card | S21, S25, S19 |
| D108 | Tooltips on the add buttons | round D D6, final `SPEC.md` 8.2 | "+ Chance" takes its final tooltip "Change a state by chance or at a rate" | S19, S22, S25 |
| D112 | Accessible names hold their visible words | round D D9, final `SPEC.md` 3, 8.3 and 8.8 | The "for each" prompt's button is "Line N: add for each infected neighbour", in the singular as it reads | S13b, S19, S25 |
| D113 | The prototype's departures after round D | `CHANGES-D.md` "Departures" | Kept as built: the plain condition word with its arrow, the renamed accessible names, among others | S1a, S16a, S19, S22, S25 |

**Words added.**
No word outside the wording table is shown, apart from the joined long-tick note "Up to 0.909 with 8. ..." (see the deviations).

## Benchmark commands left for the maintainer

None: S25 changes no engine, no shader and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
