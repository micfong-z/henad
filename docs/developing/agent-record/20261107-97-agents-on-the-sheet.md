---
date: 2026-11-07
title: "Agents on the sheet"
description: "S26 of the #37 redesign. The New model dialog offers the four agent starters and a blank world of agents, and the Sheet pane edits a world of agents: the kind of agent's name, its number at the start and at most, its properties (States, Number, Integer, Yes or no, Position, Heading) with their starts, the ground's properties with their marks (keep the largest or add up), the share each tick keeps and the states agents cannot enter, events, measures over agents, colours by heading or shaded, and buttons over agents. A blank model of agents rebuilds the random walk starter without its card lines."
icon: material/account-group
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s26-agents-sheet on 4a22a38 with no rebase needed: three unsigned [draft] commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 4a22a38
delta_state: "committed on `37-node-editor` by fast-forward: `feat: agents and the ground on the sheet pane [draft]`, `fix: the agents' largest number on a row of its own [draft]`, `fix: s26 review findings [draft]` and this record, unsigned, for the maintainer to reword and sign"
---

# Agents on the sheet

> S26 opens worlds of agents to the sheet editor.
> New model lists the random walk, boids, bounded confidence and ant foraging starters beside the grid starters, and Blank takes "Agents in open space": 1000 agents called Agent, each with a Position that starts anywhere at random and a Heading.
> In a world of agents the Sheet pane shows Agents, Ground and Events in place of Cells, and the Parameters, Measures, Colours and Buttons sections work over the agents and the ground.
> The final `SPEC.md` has no agent or ground words, so the words come from round 1 and follow the grid sections' forms.
> Three edit commands join henad-graph (`SetAgentKind`, `SetMarks`, `SetBlocked`), and a test builds the random walk's sheet from Blank by accessible names alone, equal to the starter without its card lines after the alignment of plan section 4.4.

## State before

`37-node-editor` at `4a22a38`, after S16b (every section of a grid sheet edited on the Sheet pane, the toolbar, Undo and Redo) and S20 (the agent lowering with the ground, picks, Repeat and the add-up merge).
The New model dialog held the agent starters out and showed "Agents in open space" disabled under Blank.
An agent sheet opened from a file showed the Cells section's empty text, since every section read the cells alone.
henad-graph had commands to add properties to the ground or to a kind of agent, but none to set the number of agents, a ground property's marks or fading, or the blocked states.

## What was done

**Commands and words (henad-graph).**
`SheetEdit::SetAgentKind` sets the number of agents at the start and the largest number the world holds, refusing a count of 0 or above the largest.
`SheetEdit::SetMarks` sets a Number property of the ground's marks rule and share kept per tick, and no marks rule clears the share, since the checker asks a property that fades for its marks rule.
`SheetEdit::SetBlocked` keeps agents out of a state of a States property of the ground, or lets them in, holding each state once.
A delete that leaves `[ground]` with no property and no blocked state removes it, as `AddProperty` creates it with the first property.
`sentence.rs` gains `SheetSection::Agents`, `Ground` and `Events` with headings, help and examples, `AGENT_WORLD` and `shown(made_of)`, the words of the three sections, the agent counts, the colour labels for agents and the ground, `set_everywhere`, and the four agent starters' descriptions in `STARTER_CHOICES`.
The random walk starter's measure is renamed "Average of Position" (`average_of_position`), the name the sheet's average gives it.

**The Sheet pane (henad-app).**
The pane now works on a *thing*: `cells`, `ground` or the kind of agent's id.
`property_table` and `property_form_or_add` draw any thing's table and form, `PropertyForm` remembers its thing, and `properties_of`, `find_property`, `all_properties` and `measured_thing` replace the cells-only helpers.
`form_kinds` offers States and Number, Integer for cells and agents, and for agents Yes or no, then a Position or a Heading while the agents hold none.
A new property starts in its first state, at 0, at no, anywhere at random (a Position), or with no start (a Heading).
Starts read and edit every kind: "Anywhere at random" or "On a cell whose Ground is Nest" for a Position, "All Yes" or "All No", "None until the first move" for a Heading, an Arrow's start in words, and a painted ground as "Open, with 4 shapes painted on it".
The number field of a start became `number_field` over a `NumberTarget`, which also fills a ground property's share kept each tick, with "Use a parameter" and the New parameter dialog.
"Changed by" counts Steer, Keep between, Leave and Move lines too.
`sheet_pane/agents.rs` draws the three new sections.
Agents holds the name (the kind's id follows the name while it equals the id the old name mints), the number at the start and at most, each an integer up to its limit, and the property table.
Ground holds its empty text or its table, a marks select ("Not marked", "Keep the largest", "Add up") with "Each tick keeps [share] of it" for each Number property, and "Agents cannot enter cells whose Ground is" with a box per state for each States property.
Events holds each event's name as a field with its delete button and an event form.
Measures count agents ("Number of agents whose Cargo is Searching") and average a Number, Integer or Position, Colours show "Colour agents by" and "Colour ground by" with Heading among the choices, and Buttons start agents or ground properties again or run once over every agent, keeping a button's other steps when it is edited.
`mod.rs` tracks the starts of every property for "Start again to see it", and `new_model.rs` lists every starter and creates a blank world of agents.

```text
crates/henad-graph/
├── sheets/random_walk.toml          # measure "Average of Position"
└── src/
    ├── sheet/
    │   ├── document.rs              # StartsAs::paints_single_cells
    │   ├── edit/mod.rs              # SetAgentKind, SetMarks, SetBlocked, an empty ground removed
    │   ├── lower/ground.rs          # on_ground through paints_single_cells
    │   └── sentence.rs              # Agents, Ground, Events sections and words, agent starters
    └── tests/
        ├── sheet_edit.rs            # the three commands, random edits with them
        └── sheet_ground.rs          # the predicate against the lowering
crates/henad-app/src/
├── ui/editor/sheet/
│   ├── mod.rs                       # starts of every property
│   ├── new_model.rs                 # agent starters, Agents in open space, blank_agent_model
│   ├── sheet_pane.rs                # things, property table and form, starts, measures, colours, buttons
│   └── sheet_pane/agents.rs         # Agents, Ground and Events sections (new)
└── tests/
    ├── mod.rs                       # sheet_agents
    ├── sheet_agents.rs              # the S26 tests (new)
    ├── sheet_editor.rs              # every starter offered, help of each world's sections
    ├── sheet_pane.rs                # helpers shared with sheet_agents
    └── support.rs                   # alignment over every thing
agents/henad-app-sheet.md, agents/henad-graph-sheet.md, agents/henad-graph-sheet-edit.md,
agents/henad-graph-sheet-lower.md
CHANGELOG.md
```

**Tests.**
`src/tests/sheet_agents.rs` holds `a_blank_agent_model_builds_the_random_walk_sheet_by_accessible_names`, `the_agents_take_a_name_and_an_integer_up_to_the_most_the_world_holds`, `agents_take_states_numbers_and_yes_or_no_and_no_start_on_an_unpainted_ground`, `a_ground_property_adds_up_its_marks_fades_and_blocks_agents` (`marks = "add_up"` in the text and `combine = "sum"` in the lowered graph), `events_are_added_renamed_and_deleted_on_the_sheet`, `the_ant_starter_shows_its_ground_and_starts_ants_on_single_painted_cells` and `the_agent_sections_show_no_forbidden_word_in_any_state` (29 states, at least 25 asserted, and the label-in-name check).
henad-graph's `sheet_edit.rs` adds `the_agents_take_a_count_up_to_their_largest_number` and `a_ground_property_takes_its_marks_and_fading_and_comes_and_goes_with_the_ground`, and its random edits run the three commands.
`the_new_model_dialog_offers_every_starter_with_its_description` replaces the test that held the agent starters out, and the words test opens the help of every section each starter's world shows.

**UI check.**
The check ran on the secondary display under the lock of plan section 2.4, from the worktree, through the egui MCP server.
The app painted no frame for a screenshot, as in S16b and S24, so every check reads the widget tree.
New model listed the nine choices in the wording table's order.
Boids opened from it with "Name of the agents" Bird, 50000 at the start, Position "Anywhere at random", Velocity's start in words and "Add property to Bird", and Play ran its preview to tick 405.
Ant foraging opened with "To food marks" at "Keep the largest", the share kept each tick as the "Trail kept per tick" chip, four "Agents cannot enter" boxes, and "Start again: every property, Set To food and To home to 0 everywhere" under Reset colony.
Played to tick 11641, the Stats tab read Total trail 792.886, a trail laid on the ground.
The tree showed the "Most agents the world holds" field squeezed to 24 points beside the count, so the largest number moved to a row of its own (`fix:` commit), and a second run showed it at its 72 points under a blank model of agents with 1000 agents called Agent.

## Deviations

- **The random walk starter's measure is renamed.** The sheet's average names a measure "Average of [property]", and the gate keeps the grid forms for agents, so no control writes the starter's "Average position".
  The starter now reads "Average of Position" (`average_of_position`), the only change to a committed sheet, and no test or fixture reads the old name.
- **Steps per tick is prepared as a part S26 does not build.** The random walk's Steps per tick is a whole number, which a parameter takes from a blank on the card that counts (S28), and no control of the sheet sets it.
  The rebuild test sets that parameter's type to Number in the expected document, after `without_card_lines`, as plan section 4.4 removes a part the milestone does not build.
  `align_with_starter` copies no parameter's type, so S28's rebuild has to set it through the card. The alignment covers the properties of agents and the ground as well as the cells.
- **The agents' name and number are on the sheet, not in the dialog.** Round 1 places them in the dialog for boids and on the sheet for the random walk.
  The sheet takes them, so the dialog keeps the gate's words, and the kind's id follows its name while it equals the id the old name mints, which gives the rebuild the starter's `walker`.
- **The Ground heading is always shown in a world of agents.** The plan has the section appear with the first ground property, but the button that adds one needs a place, so the heading shows with an empty text, and the table, marks and blocked states appear with the first property.
- **Events are their own section.** The plan lists events with the Ground section. Events belong to agents' Record lines, so they take a heading of their own after Ground.
- **A property that takes marks keeps all of it each tick to start with.** The checker refuses a marked property with no share kept, so choosing a marks rule puts in a share of 1 when the share is blank, and the share's field refuses an empty text.
- **No control paints the ground, and no control writes an Arrow's start** (the plan names neither).
  A painted start shows its base and its number of shapes, and an Arrow's start shows in words.
  The select of a painted States property shows "Painted" as a selected entry, and choosing it changes nothing. Choosing a state or By chance there replaces the painted start and its shapes, and only Undo brings them back.
  A Position can start only on a state the ground paints on single cells, so a ground made on the pane offers no on-ground start.
- **Ground measures and ground counts stay as the file holds them.** The Measures section counts and averages the agents. A ground measure such as the ants' Total trail shows by its name.

**Words added.**
The final `SPEC.md` has none for agents or the ground (gate section 11, S26), and no researcher met these.

| Item | Words | Source |
| --- | --- | --- |
| Section headings | "Agents", "Ground", "Events", each with "Help: [heading]" | round 1 section 1 |
| Agents help | "The agents move in open space. Name them and say how many there are at the start. Every agent has each property listed here. Starts as says what every agent holds when the run starts." Example "Walker: 10000 at the start. Position starts anywhere at random" | the Cells help, round 1 section 4 |
| Ground help | "The ground is the grid of cells under the agents. Agents read it and leave marks on its Number properties. Marks says how the marks left on one cell in one tick combine, and a share of each mark can fade every tick. Agents never enter a blocked cell." Example "To food: Number. Marks keep the largest. Each tick keeps 0.999 of it" | round 1's ants sheet |
| Events help | "An event is something that happens to an agent. A Record line counts it each time, and a measure can plot the count." Example "Delivery: an ant carrying food reaches the Nest" | round 1's ants sheet |
| Agent kind | "Name" (named "Name of the agents"), "Number at the start" (named "Number of agents at the start"), "At most" (named "Most agents the world holds", tooltip "The world keeps room for this many agents."), "Type a name for the agents.", "Type an integer from 1 to 1000000." | round 1's build steps |
| Add property | "+ Add property", named "Add property to Walker" or "Add property to the ground" | the Cells form |
| Kinds | "Position" ("Where the agent is in the world."), "Heading" ("The direction the agent last moved in."), "Yes or no" ("Holds yes or no, such as whether an agent is hungry."), "Arrow" | round 1 section 5 |
| Starts | "Anywhere at random", "On a cell whose Ground is Nest", "None until the first move", "All Yes", "All No", "Painted", "Open, with 4 shapes painted on it", "Every agent starts at" | round 1's sheets |
| Ground | "The cells under the agents. Add a property to give the agents a trail to leave or cells to avoid.", "Marks" (tooltip "What a cell keeps when several agents leave some on it in one tick."), "Not marked", "Keep the largest", "Add up", "Each tick keeps [share] of it" (named "To food, share kept each tick: number"), "Type a share from 0 to 1, such as 0.99.", "Agents cannot enter cells whose Ground is" with boxes named "Agents cannot enter Obstacle" | round 1's ants sheet, plan scope |
| Events | "No events yet. Add one for a Record line to count.", "+ Add event", "New event", "Event name" (placeholder "Such as Delivery"), "Type a name for the event.", "An event called X already exists.", "Name of event Delivery", "Delete event Delivery" | the button form |
| Measures | "Number of agents in a state", "Number of agents whose", "Number of agents whose Cargo is Searching" | the Cells measures |
| Colours | "Colour agents by", "Colour ground by", "Each agent takes one of eight hues by the direction it last moved in." | round 1's random walk |
| Buttons | "Run once, every walker", "Set To food and To home to 0 everywhere" | round 1's ants sheet |
| Starter descriptions | "Walkers take random steps and spread evenly across the plane.", "Birds steer by the birds around them and form flocks.", "People move their opinion towards the opinions close to it.", "Ants lay trails and find a short path from the nest to food." | round 1's summaries |

## Review

A reviewer with a fresh context listed ten findings, and `fix: s26 review findings [draft]` answers each.

- **Ground kinds (must fix).** `form_kinds` offered Integer to the ground, which the lowering refuses ("property 'count_here' of the ground is whole_number, and the ground holds numbers and states").
  The ground now offers States and Number, a property keeps its own kind when a file holds another, and `a_ground_property_adds_up_its_marks_fades_and_blocks_agents` reads exactly the radios States and Number under "Add property to the ground".
- **On-ground starts (should fix).** The Position select offered every state of every ground States property, and a state no painted cell holds failed to lower with `ON_GROUND_CELLS`.
  henad-graph gains `StartsAs::paints_single_cells(state)`, which `lower::ground::on_ground` and `start_select` both use.
  The S26 test now checks that an unpainted ground made on the pane offers no on-ground start, and the ants test checks that only Food and Nest are offered and that the Food start lowers.
  `agents_start_on_exactly_the_states_painted_on_single_cells` in henad-graph holds the predicate against the lowering for each of the ants' four ground states.
- **Parameter type in the alignment (should fix).** The copy of a parameter's type is reverted, and the rebuild test sets Steps per tick to Number in the expected document instead (the deviation above).
- **The Viewport check (should fix).** No image could be taken. The session screen was locked overnight (`CGSSessionScreenIsLocked` in `ioreg`), `screencapture` returned an all-black frame and saw one display only (`-D 2` refused, `-R` on the G15F1 rectangle failed with "could not create image from rect"), and the egui MCP screenshot timed out as before. The trail on the Viewport and the agent sections on screen remain for the maintainer.
- **Nits.** The count message reads "Type an integer from 1 to 1000000." (`integer_up_to`), the Agents help reads "Every agent has each property listed here.", `on`, `at` and `every` became `kept_out`, `start_again_index`, `thing` and `thing_word`, the three long topic-file paragraphs are re-wrapped at 100 columns, the alignment test also covers the ants' agent and ground start keys, and a painted start shows as its own selected entry in its select.

The repeated UI check ran under the lock on G15F1 with the egui MCP server, through the widget tree only.
Ant foraging from New model showed the Position select offering "Anywhere at random", "On a cell whose Ground is Food" and "On a cell whose Ground is Nest" only, the Ground select offering "Painted" first (clicking it left Undo disabled), "Add property to the ground" opening a form with the radios States and Number, the Agents help in its new words, and "Type an integer from 1 to 5000000." after 0 was entered as the count.
The Position select is wider than the sheet pane at a 1600-point window, and its right half sits under the card pane, where a click opens nothing. The left half opens it.
No benchmark ran. `cargo fmt`, clippy over henad-graph and henad-app with `-D warnings -W clippy::all`, `HENAD_REQUIRE_GPU=1 cargo test -p henad-graph -p henad-app --all-targets --all-features` (henad-app 429 passed, henad-graph 443 passed and 1 ignored), the henad-graph doc tests, henad-models' sheet tests, `cargo doc` with `-D warnings` and the wasm32 check pass.

## State after

A sheet of agents opens, is built from Blank and is edited on the Sheet pane in every section a world of agents has, and each edit is one step of Undo.
A blank model of agents can be built into the random walk's sheet without its card lines, which S28 adds.
The Card pane of an agent sheet stays read-only until S28.
`37-node-editor` holds the three unsigned commits, fast-forwarded with no merge-check fix.

### Merge check

- The branch already sat on the tip of `37-node-editor` at `4a22a38`, so the merge rebase replayed nothing and raised no conflict.
  The rebase brought in no henad-app change, so the UI check was not repeated.
- The full check then ran on the branch with `HENAD_REQUIRE_GPU=1`.
  Every step exits 0 but two, and both fail as the baseline does.
  `cargo deny` reports the baseline's three advisories, RUSTSEC-2026-0370, 0371 and 0372, and no other.
  The workspace tests fail on the baseline's budget test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets` alone, in the one failing target (henad-models' library), with no new failing name and no aborted test binary in the log.
- No baseline test passes now.

## Issues found & future directions

- **The agent sections are untested by researchers** (plan section 10). No round of the study met them, and their words come from round 1 alone.
- **The pane is narrow for agent rows.** The ground's blocked boxes and a property's "Use a parameter" button run past or wrap inside the pane's default 40%, as the parameter rows already did in S16b.
- **Measures over the ground are read-only.** The form counts and averages agents only. A count of ground cells in a state, a total over the ground and an event count, which the ants hold, have no form yet.
- **A button with only set-everywhere steps.** Editing a Start again button keeps its set-everywhere steps, but a button holding no start-again step reads as one whose kind changes, and Done would replace it.
- **No paint tool and no Arrow starts** (D74). The ants' painted ground and the boids' Velocity start are kept from the file and cannot be made from Blank.
- **The app painted no frame for a screenshot**, as in S16b and S24, and the review's check found the session screen locked, so the UI check read the widget tree only, and a look at the agent sections and the ants' trail on the Viewport remains for the maintainer.
- **The Position select runs under the card pane.** At a 1600-point window its right half sits beneath the card pane and takes no click.

## Decisions adopted for review

From the plan's section 6, as written there with design files named instead of cited by path:

| # | Question | Source | Recommendation taken | Built in |
| --- | --- | --- | --- | --- |
| D49 | The add-up merge | the recon of the engine, answer 10a | A power-of-two resolution (2^-16 for a sheet's "add up"), rounding half to even on both sides, a NaN adding 0, deposits clamped at 0 | S15, S20 (S26 offers "Add up" on the sheet) |
| D64 | Which round 1 models ship as starters | round 1 section 14, answer 13a | All eight, forest fire and bounded confidence included, and the ants keep their slower trail with a note | S1b, S10, S17, S20 (S26 lists the agent starters) |
| D67 | "Agents in open space" in New model | answer 12a | Enabled once agent sheets lower and edit | S26 |

Row R5 of round 1 (the sheet editor) is built here for agent sheets on the sheet pane.

## Benchmark commands left for the maintainer

None: S26 changes no engine, no shader and no hot loop.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
