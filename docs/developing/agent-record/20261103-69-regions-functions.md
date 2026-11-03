---
date: 2026-11-03
title: "Regions and functions in the graph editor"
description: "Milestone E2 of #37. Regions draw as two halves inside nested frames with their accumulators on the canvas, a selection wraps in a Repeat, and nodes move into and out of regions under library 1's nesting rules. Functions open under a breadcrumb on canvases of their own, a selection becomes a function and a call ungroups, a function imports from another model file with its origin, and the Expression node has an editor. The Editor runs graphs of agents, and the gate passed on the boids starter in the running app."
icon: material/vector-square
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor with E2 as ten unsigned [draft] commits after 5055658 and one of review fixes; the E2 gate met through the egui MCP server on the native app; HENAD_REQUIRE_GPU=1 check.sh lines passing apart from cargo deny's three advisories, which master fails too, and the henad-models budget test of record #68, which fails since the A2 review"
baseline_commit: 544add1
delta_state: "committed on `37-node-editor`: eleven unsigned commits after `5055658`, `d108779` to `6f59ac0`, for the maintainer to reword and sign"
---

# Regions and functions in the graph editor

> E2 ran in two steps in the worktree `37-e2`.
> Step 1 drew regions: two halves inside a frame, nested frames, the accumulators on the canvas, Repeat over a selection, and nodes dropped into a frame.
> Step 2 rebased onto A2, added functions (a breadcrumb, a canvas per body, Make function, Ungroup, Import function), the Expression node's editor and the nesting rules in the menus, and made the Editor run graphs of agents.
> On the boids starter in the running app, an edit of `clamp_speed` restarted the preview and Apply restarted the run at its own size, a Repeat bound to a new parameter with a node reading `k` restarted the preview, and a node moved out of a region and back.

## State before

Rounds 1 and 2 and A2 were committed on `37-node-editor` up to `544add1`, records #61 to #68.
The Editor's graph editor (E1, record #65) drew one canvas per pass, compiled grid graphs alone and listed grid starters alone.
A2 (record #68) added the agent kinds, `fn.*`, `math.expr`, `grid.fold` and `agent.radius_fold`, and `AgentProgram`.
henad-graph had no edit command for functions, and its `CheckReport` typed passes alone.

## What was done

### Step 1: regions on the canvas

Step 1 left no record, and this one covers both steps.

- **Region edits** (henad-graph `edit/regions.rs`): `MoveRegionOutput`, `AddAccumulator`, `SetAccumulator`, `RenameAccumulator`, `RemoveAccumulator` and `WrapInRepeat`, with `Accumulator::new`, `to_config`, `Combine::ALL`, `applies_to`, `NodeBody::region_contents` and `EditorRegion::set_output_pos`.
  `name_bare_links` and `name_links_into_path` name a bare link's port before an edit could make it read another output.
- **Halves and frames** (henad-app `canvas.rs`): each region draws as an input half and an output half (`NodePart`), with a frame per region behind both halves and its body, nested by depth, from the rectangles of the last frame.
  A node dropped inside a frame joins the region, and the target frame takes a blue outline while the button is down.
  The node menu holds Move into region and Move out of region.
- **Controls**: a Repeat's Max, and per accumulator a name field, a type menu, a combine menu and Remove, with Add accumulator.
  A constant `count` sets `max`.
- **Repeat** in the toolbar wraps the selection, and `fit_wrap` moves the halves clear of the wrapped nodes once they are drawn.
- **Quick-add in a frame** offers the kinds a region body takes and adds the node to that region.

### Step 2: functions, expressions and agents

#### henad-graph

- **Function reports.** `CheckReport::functions` holds a `PassReport` per function body checked, with the cost of one call, and `check_function` now returns the body's ports.
  `PassReport`'s docs name a body, a pass or a function.
- **Function edits** (`edit/functions.rs`, `PortSide`):
  - `AddFunction`, `RenameFunction` (calls, canvas scope and `[editor] function.<id>`), `RemoveFunction` (refused while called).
  - `AddFunctionPort`, `RenameFunctionPort`, `SetFunctionPortType`, `RemoveFunctionPort`.
    A function gains a second output only through `AddFunctionPort`, which first names the port on every bare link to each of its calls in every body.
    A `fn.output` node or its `port` changes no call's outputs, since a call's outputs are the declared ones.
  - `MakeFunction { scope, nodes, id }`: the selection's subtrees move into a new function with their ids and placement, each value read from outside becomes a typed input, each output read outside becomes an output, and a call with a fresh key takes their place.
    It refuses sinks, a put into a region outside the selection, a function's own input or output, and a node a statistic reads.
  - `Ungroup { scope, node }`: the called body is copied in with fresh ids, its inputs replaced by the call's and the call's readers by the outputs, and each counter draw takes `compose_key(call key, key)`, the key it had inside the call.
  - `ImportFunction { text, source, function, id }`: the function and every function it calls are copied with fresh ids, keys and placement kept, and `[editor] function.<id>.origin = { name, hash }` records the source file and the FNV-1a of the function's canonical text (`writer::function_text`).
    An id the file holds is refused with "Import it under another name", and text that is no graph is `EditError::Import`.
- **Repeat in a function.** `WrapInRepeat` takes a function body, typed through `body_report`.
- **Topic split.** `agents/henad-graph.md` reached 39928 characters, so its edit commands and the editor region moved to `agents/henad-graph-edit.md`.

#### henad-app

- **Graphs of agents.** `GraphProgram` compiles a graph through `GridProgram` or `AgentProgram` by its world, for the WGSL view and the preview.
  The starter menu lists the agent starters after the grid ones.
  A preview of agents runs at most `PREVIEW_AGENTS` (16384) agents in a world of the model's own size, and `PreviewRun` became an enum over grid and agents.
- **Functions** (`graph/functions.rs`, `mod.rs`, `canvas.rs`):
  - the functions panel under the pass stack, with issue badges, the origin of an imported function, the open function's name, inputs and outputs, Add input and Add output (each with the node that names it), Remove function, an add form and Import function;
  - a breadcrumb in the toolbar (`trail`, `OpenCall`, `OpenParent`), each function on its own canvas with its own egui id, typed by its report, with its problems on its nodes (`diagnostic_scope`, `issue_counts`);
  - Function in the toolbar, Open function and Ungroup in a call's node menu, and Settings menus for a call's function and for a function's input or output port;
  - Import function through a new `OpenTarget::Function`, whose file `set_import_source` lists.
- **The Expression node's editor**: the text, each input's name, type and Remove, Add input, and the outputs as one field, each committing when it loses focus.
  Renaming an input moves its wire.
- **Nesting rules on the canvas.** `can_join` filters the drop targets and both region menus: a Repeat holds a Repeat or a neighborhood fold, a neighborhood fold holds a Repeat, a radius fold holds no region and sits in none, and regions nest at most three deep.
- **Quick-add in a function** offers the pure kinds of the world's domain and the function's own input and output, and a pass never offers those two.
- **Found at the gate and fixed:**
  - A move of a node, or Arrange, restarted the preview, since the preview key followed every revision.
    The key now follows a *code revision*, the count of changes of `GraphFile::code`'s hash.
  - Arrange put the input half of a Repeat whose count reads no node in the first column, so its frame spanned the whole pass.
    The input half now moves to the column before its first reader.

### Tests

- henad-graph `src/tests/functions.rs`, eleven tests: make a function and ungroup its call with the pass computing as before over two host steps, an ungrouped draw drawing what the call drew, refusals, a function made inside a radius fold called inside it, a second output naming bare links, renames and removes of functions and ports, an import with its callees, fresh ids and origin, a function imported into another model checking, a function's report, a Repeat inside a function, and an empty function given ports and nodes.
- henad-app `src/tests/graph_functions.rs`, nine tests over the canvas harness: a call opening its function under a breadcrumb on a canvas of its own, a problem in a function, make and ungroup through the editor, an added output with its node, an import from a picked file, a wrap in a function, `can_join`'s rules, a radius fold's node menu, and the expression editor's rename typed through egui.
- `graph_preview.rs`: `an_agents_graph_previews_at_the_preview_count_and_applies_at_its_own` and `moving_a_node_leaves_the_preview_running`.
  `every_starter_previews_within_the_device_limits_and_the_work_budget` covers the agent starters.
- `arrange.rs`: `the_input_half_of_a_region_sits_in_the_column_before_its_body`, and `every_wire_of_a_starter_runs_rightwards_and_no_two_nodes_share_a_place` covers the agent starters, a link to an inner output leaving the input half.
- `quick_add.rs`: `a_function_offers_pure_kinds_and_its_own_input_and_output`.

### The gate in the running app

The native app ran from `target/debug` with `--features inspection` on the main monitor, driven through the egui MCP server, with screenshots.
The Chrome run was not done.

1. **A function edit.** New graph model, Boids Flocking.
   The preview ran at 16384 agents.
   In `flock`, the call `n49`'s node menu, Open function, opened `clamp_speed` under the breadcrumb **flock › clamp_speed function**.
   Setting the constant of Greater `n114` from 0 to 1000000 sends every boid to the fallback `(min, 0)`, and the preview restarted from tick 0 with Average Speed 3 and Average Velocity (3.0, 0.0).
   Apply read **Model applied.**, the run rebuilt at 50000 agents from tick 0, and after Play it read Average Speed 3 and Average Velocity (3.0, 0.0).
2. **A Repeat.** On a fresh boids starter, after Offload, a new Integer parameter `sub_steps` restarted the preview.
   Selecting To f32 `n37` and pressing Repeat wrapped it in Repeat `n124` with a `carry` accumulator `out`, and the preview restarted.
   Binding `count` to `sub_steps` through the node menu restarted the preview and warned **Repeat n124: `count` can reach 100, and `max` is 8, so the loop stops at 8** once Max was set to 8.
   Dropping a wire from `k` inside the frame opened quick-add with the region's kinds, and Maximum `n126` joined the region reading `k`, then `n25.count` as its second input, and the preview restarted each time.
   Maximum's output fed nothing, so it carries the warning that it reaches nothing the pass keeps, and a second drag to wire it into To f32 panned the canvas instead.
3. **Move out and in.** Maximum's node menu, Move out of region, took it to the top of the pass, where it showed the error **the input `a` reads n124.k, which only the body of n124 can read** and the preview kept running.
   Move into region listed **Agents in radius n25** and **Repeat n124**, and Repeat n124 returned it, the error cleared and the preview restarted.
4. **Make function and Ungroup.** Selecting Maximum and pressing Function made `function` with a call `n127` inside the Repeat and the note **Made function 'function'. Rename it in the Functions panel.**, and the call's Ungroup put Maximum back as `n130`.

Import function opens the system file dialog, which the MCP server cannot drive, so it was held by the tests alone.

### Review findings and fixes

A review of E2 after the gate confirmed seven findings, fixed in one further commit.
Each behaviour fix has a test that fails without it, checked by running the test with the fix taken out.

- **High: Undo stuck in a function canvas.** Add input or Add output added the port's node with no placement, and so did an import from a file whose `[editor]` places no node (the ants starter's places none).
  `BodyCanvas::moved` reported the node's drawn place as a move, recorded as an edit of its own, and after each undo `restore` brought the unplaced node back, the canvas recorded the move again and redo was cleared.
  `place_unplaced` now runs on every edit's draft before the snapshot (`edit`, `add_node`, `wrap`, `add_function_port`) and after `restore`, so a pasted node without placement is covered too.
  Tests: `one_undo_takes_back_a_port_added_in_the_open_function`, `one_undo_takes_back_an_import_with_no_placement`.
- **Medium: an import under a callee's name.** Importing ants' `move_if_passable` as `passable` passed the check, since the check read only the file's own functions, and the file then declared `passable` twice.
  `import_function` now refuses a new id that one of the imported callees takes.
  Test: `an_import_is_refused_under_the_name_of_one_of_its_callees`.
- **Medium: quick-add ignored the nesting rules.** Inside a frame, quick-add offered every kind with `contexts.region`, so it offered a Repeat inside a radius fold, a neighbourhood fold inside a neighbourhood fold, and a Repeat four deep, and `GraphEditor::add_node` added them.
  `can_add` applies `can_join`'s rule to a new node, quick-add filters by it, and `add_node` refuses the same cases with a status line.
  Tests: `quick_add_in_a_radius_fold_offers_no_region_and_the_add_is_refused`, and `can_add` cases in `regions_join_as_library_one_nests_them_three_deep`.
- **Low: WrapInRepeat broke the model.** It wrapped a put whose region lay outside the selection, and a sink such as `lane.set`, both of which the checker then reports.
  It now refuses both, as `MakeFunction` does.
  Test: `a_wrap_of_a_put_without_its_region_or_of_a_lane_write_is_refused`.
- **Low: a duplicate expression input.** Renaming an expression input to another input's name wrote the name twice and overwrote the other input's wire.
  The rename is refused with "Node n35 already has the input 'count'." through a new `GraphRequest::Refuse`, which sets the status line.
  Test: `the_expression_editor_refuses_the_name_of_another_input`.
- **Low: Max clamped on sight.** egui 0.36's `DragValue` clamps an existing value to its range by default, so drawing a Repeat with `max` 100000 or 0 edited it to 65536 or 1, with an undo step and the checker's error hidden.
  Max now sets `clamp_existing_to_range(false)`.
  Test: `a_bound_out_of_range_is_drawn_and_left_for_the_checker`.
- **Low: five lines over 120 columns** that E2 added, in `quick_add.rs`, `preview.rs`, `canvas.rs`, `arrange.rs` and henad-graph's `edit/mod.rs`, re-wrapped.

### Edited tree

```
AGENTS.md                                     the henad-graph-edit topic row
CHANGELOG.md                                  Added: regions, functions, agents in the Editor; Changed: preview key, arrange
agents/
├── henad-app-editor.md                       regions, functions, expressions, agents, code revision, tests
├── henad-graph-edit.md                       new: edit commands and the editor region, split out
└── henad-graph.md                            function reports, starters in the Editor
crates/henad-graph/src/
├── check/                                    CheckReport::functions, function costs
├── edit/
│   ├── functions.rs                          new: function edits, make, ungroup, import
│   ├── regions.rs                            new in step 1: region edits; selection, body_report
│   └── mod.rs, declarations.rs               the new commands, EditError::Import
├── editor.rs                                 FunctionOrigin, set_output_pos, function entries
├── node.rs                                   Accumulator::new, Combine::ALL, region_contents
├── starters.rs                               docs: both topologies in the Editor
├── writer.rs                                 function_text
└── tests/                                    new: functions.rs, regions.rs
crates/henad-app/src/
├── state.rs                                  PreviewRun over grid and agents
├── ui/editor/
│   ├── mod.rs                                agent starters, PickImport, receive_import, problems in functions
│   ├── preview.rs                            PREVIEW_AGENTS
│   └── graph/
│       ├── arrange.rs                        CanvasKey, the input half before its body
│       ├── canvas.rs                         halves, frames, controls, can_join, call menus, expression editor
│       ├── functions.rs                      new: the functions panel
│       ├── mod.rs                            GraphProgram, trail, function requests, code revision
│       └── quick_add.rs                      function bodies, regions
├── ui/files/                                 OpenTarget::Function
└── tests/                                    new: graph_functions.rs, graph_regions.rs
docs/
├── developing/agent-record/20261103-69-regions-functions.md
└── guide/editor.md                           regions and Repeat, functions, expressions, agents
zensical.toml
```

### Decisions made on the maintainer's behalf

- **The record covers both steps of E2**, as #68 did for A2.
- **A function's outputs grow through `AddFunctionPort` alone.** Step 1's handoff named `fn.output` nodes as the source of a second output, but a call's ports come from the declared `[[function.output]]`, so the bare-link naming sits on the declaration edit.
- **`CheckReport::functions` reuses `PassReport`.** A body report of a function holds the same maps, and its `cost` is one call's.
- **Making a function moves the nodes and keeps their ids.** Ids are unique across the file, so no renumbering is needed and the placement follows.
  Their draws then draw other numbers, since a call composes its key with theirs, and Ungroup keeps the numbers a call drew.
  A call nested inside an ungrouped body takes the composed key, and its own draws change.
- **An import copies functions alone.** Declarations a function reads (fields, categories, lanes) are not copied, and the checker reports what the file lacks.
  Each copied function gets an origin, the called ones included, and the hash is a 16-digit hexadecimal string, since TOML integers are signed 64-bit.
- **The preview of agents caps the count at 16384 and keeps the world's size.** No measurement chose the number. It is of the order of the 256 by 256 grid preview.
- **Add input and Add output add the port and its node together**, named `input`, `output`, `input_2` and so on, typed `f32`.
- **Make function names the function `function`, then `function_2` and so on**, with a note to rename it in the panel.
- **The preview key follows the code.** A placement edit changes the canonical text and no code, and restarting the preview for it made dragging a node restart the run.
- **Arrange moves a region's input half right** to the column before its first reader. The step-1 rule left a Repeat bound to a parameter in column 0.
- **No `AGENTS.md` change beyond the topic row.**

## State after

- `37-node-editor` holds E2 as ten unsigned `[draft]` commits after `5055658`, `d108779` to `fad443e`: four of step 1, rebased, and six of step 2, this record in the last, then `6f59ac0` of review fixes.
  The work ran in the worktree `37-e2`, which no longer exists. The round 3 audit (record #70) corrected this paragraph and the frontmatter.
- After the review fixes, `cargo fmt`, clippy over henad-graph and henad-app with every target and feature, their `HENAD_REQUIRE_GPU=1` tests (henad-app 346, henad-graph 167 and the one test ignored for Metal's fast math in record #67) and doc tests, rustdoc with `-D warnings` and the wasm32 check of both pass.
  The full `check.sh` was not run again after the fixes.
- `HENAD_REQUIRE_GPU=1`, every line of `check.sh` before `cargo deny` passes, and `cargo deny` fails on RUSTSEC-2026-0370, 0371 and 0372 alone (`zbus_names`, `zvariant`, `zvariant_derive` unmaintained; bans, licences and sources ok), as `master` does.
  By hand after it: clippy over the workspace, the doc tests, the README program, rustdoc with `-D warnings`, the scripts' tests and `./scripts/build_web.sh build` pass.
  The tests of every workspace crate pass but henad-models' `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`, which fails since the A2 review's walk charge (`b9f1019`), as record #68 reports. An earlier version of this paragraph called every test passing, which the round 3 audit reproduced as wrong.

| Gate | Result |
|---|---|
| On the boids starter, open `clamp_speed`, change it, Apply, and the run restarts | Pass, natively: preview restarted, Apply rebuilt at 50000 agents from tick 0, the change visible in the statistics |
| Wrap nodes in a Repeat, bind `count` to a new parameter, read `k` in the body, and the preview restarts | Pass, natively: each step restarted the preview. The node reading `k` feeds nothing, so it keeps a warning |
| Move a node into a region and out again | Pass, natively: out through the node menu with its error shown, back through Move into region, error cleared |
| The same in Chrome | **Not run** |
| Import function in the app | Not driven: the system file dialog is outside the MCP server's reach. Held by tests |

## Issues found & future directions

- **Wiring at small zoom.** Dragging from a pin to a node far across the canvas sometimes pans instead, as the second wire of the gate did. The wire has to start exactly on the pin's small hit area.
- **Arrange still spreads a region.** The input half now sits before its body, but the output half follows the longest path of the whole pass, and the frame can still cover nodes outside the region.
- **The function panel offers no default value or reorder of ports**, and an output removed while a call's reader names it is refused with the list of readers, with no offer to rewire.
- **Expression outputs renamed** in the outputs field leave readers of the old name with an error. A rename command for expression outputs would rewrite them.
- **Make function changes draws.** Keeping a selection's numbers would need a call key that composes to the identity, which `compose_key` does not have.
- **Import copies no declaration.** An import that also copied the lanes, fields and categories a function reads, refused on a clash, would make libraries of functions usable across models.
- **The preview cap for agents** is unmeasured. E1 measured the grid's, and the agents' should be measured the same way on a slower GPU.
- **The Chrome run of the gate** is outstanding.
- **WrapInRepeat checks no nesting depth.** The rule of `can_join` lives in henad-app, and the edit does not apply it, so wrapping a region chain three deep would make it four deep, which the checker reports. Read from the code, not shown by a test.
- **The world panel's DragValues** (`world.rs`, grid sides, agent count and size, parameter defaults) predate E2 and keep egui's default clamp of an existing value, the same shape as the Max finding. Whether a clamp there writes back depends on `changed()`, and was not checked.
- **Lines over 120 columns from before E2** remain: a doc line in `ui/editor/mod.rs` (121) and string literals in henad-graph's `check/cost.rs`, `edit/declarations.rs` and `tests/file_format.rs`.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
