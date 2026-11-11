# henad-graph: conversion of a sheet to a node graph

`agents/henad-graph-sheet.md` holds the sheet module, and `agents/henad-graph-edit.md` the editor
region the copy writes. The app's expert mode (`agents/henad-app-editor.md`) calls the conversion
and `changes_since_conversion`, and reads its words from `sheet::sentence`: `EDIT_AS_NODE_GRAPH`,
`MODEL_ACTIONS`, the banner's `converted_banner`, `edited_banner` with `node_edits` ("2 nodes
added, 1 changed", "1 node changed", "no node changed"), `original_changed_banner`,
`original_not_beside`, `open_original`, `converted_link`, `converted_date` ("7 Nov 2026") and
`FROM_AUTHOR_MODEL`.

**`sheet/convert.rs`** (private, `sheet` re-exports `convert`, `ConvertOptions`, `ConvertError` and
`ConvertRefusal`). `convert(&SheetFile, &ConvertOptions)` returns a `GraphFile`: the lowered
document cloned, every node with its id and key, so the copy runs tick for tick like the sheet at
one seed; header and target id and name from the options, header and target description
`sentence::converted_description` ("Converted from the author model Game of Life. Edit it as a node
graph."). The copy's code differs from the sheet's, since the id, name and description enter the
semantic text, and `origin.code` names the sheet's. `ConvertOptions { id, name, file, date }`
are public fields; `ConvertOptions::new(&SheetDocument, date, taken)` numbers the copy:
`<id>_expert` and `sentence::expert_copy_name`, then `<id>_expert_<n>` and "(expert n)" through
`expert_copy_number_name` while `taken(id, name)` says either is used, up to 999 tries, and `file`
is `<sheet id>.toml`. The library reads no clock and knows no installed model, so the date and
`taken` come from the caller. `ConvertRefusal::of(&SheetReport)` returns `Errors` before
`SkippedLines`, and `tooltip()` the two tooltips of the wording table
(`CONVERT_NEEDS_FIXES`, `CONVERT_NEEDS_FINISHED_LINES`). A sheet with errors has no `SheetFile`, so
the app asks `of` on its `ParsedSheet::check`; `convert` itself rechecks and returns
`ConvertError::Refused(SkippedLines)`. `ConvertError::Unreadable` holds a copy whose canonical text
does not read back.

The copy's `[editor]` region, written through `EditorRegion`'s typed setters:

- `origin`, a `ModelOrigin` with `kind = "sheet"`, `model` and `name` of the sheet, `file`
  ending in `.toml`, `hash` the FNV-1a 64 of `SheetFile::to_toml` (`source_hash`), `code` and
  `copy_code` the code hashes, each 16 lowercase hex digits, and `converted`.
- `node.<id>.label` only, never `pos`: the result node of each named line
  (`LineMap::result_node`, the fold for a Look around) takes the line's name, a sheet-owned
  `lane.get` at path `cells.<property>` or `agents.<property>` (`property_read`, roots in
  `PROPERTY_ROOTS`, the lowering's `property_path`) takes "my Status" (`my_property_item`), a
  `lane.get_at` "their Status". Property names come from the cells', every kind of agent's and the
  ground's properties (`property_name`). Such an entry has a label and no `pos`, and
  `EditorRegion::node_layout` returns it at `[0, 0]`, so "placed" is `EditorRegion::node_pos`. The
  app's `place_unplaced` (`ui/editor/graph/mod.rs`) and paste corner use `node_pos`, and opening a
  copy arranges every node (`a_converted_copy_opens_arranged_with_its_labels` in henad-app).
- One `[[editor.frame]]` per origin group (`frames`, also used by `changes.rs`), pass by pass in
  stack order. Grouping (`pass_origins`, `sheet_origins`): a line-owned node is its line's; a sheet
  node at `card.<card>.<property>` (the change chain's write) belongs to the lines that write that
  property on that card and run, in display order (`SheetLines::writers`: Become, Set, Steer and
  Keep between by their `property`, Move for the kind's Position and Heading properties);
  `<root>.<p>.count` (the shared `stat.count_by`) to the count measures of `p`;
  `<root>.<p>.starts_as...` to the start; `measure.<id>`, `button.<id>`, `color.<thing>` to that
  part. `<root>` is `cells` or `agents`. The rest resolve from links, in rounds to a fixed point:
  the read `<root>.<p>`, the part of a card several looks share (`card.<card>`, the agent lowering's
  shared radius and cell size, and its folds) and a `card.<card>.<property>` no line writes. Each
  round settles a node whose readers (nodes linking to it in the pass) all have a group, with the
  group they share (`shared_origins`: the union of their lines, or their one sheet part, else a
  frame of its own). A round that settles none settles the nodes whose resolved feeders share a
  group (the end of a chain no node reads), and a round that settles none of those leaves each
  remaining node alone. A group of one line merges with that line's own frame (Clear line 1 holds
  the read, transition and write). Order in a pass: single lines by display place, shared groups,
  then sheet parts. Titles from `sheet::sentence`: `line_frame_title(line_prefix,
  Sentences::line)` with the line's "otherwise" numbers ("Line 2: Become Alive from Dead, when Alive
  neighbours is 3", "Clear line 1: ..." on a button's card), `lines_prefix` and `new_value_words`
  ("Lines 2 and 3: the new Status (the first change that applies wins)", "my Status" when the group
  only reads, the prefix alone when the group does neither, as a shared look), `sheet_frame_title`
  with `starts_frame_words`, `my_property_item`, `Sentences::card_heading` for `card.<card>`,
  `new_value_words`, `measure_frame_words`, `button_frame_words` or `color_frame_words` ("colour
  each cell by Status", "colour each bird by Heading" for `by_heading`). An unknown path takes
  `SHEET_PART_WORDS`, never the raw path. Colours: blue for one line, purple shared, gray for
  the sheet. `line` holds one origin, `lines` several: line ids such as `l2`, or sheet part
  paths such as `cells.status.starts_as`, `measure.alive` or `button.clear`.
- `[[editor.note]]` per `purpose` (titled "Purpose", `SheetSection::Purpose.heading`) and per
  parameter `means` (titled with the parameter's name), no scope and no position.

**`sheet/changes.rs`** (private, `sheet` re-exports `changes_since_conversion`, `ChangeGroup` and
`ChangesError`). `changes_since_conversion(&GraphFile, &SheetFile)` refuses a copy with no origin
(`NoOrigin`) and a sheet whose `SheetFile::code` hash differs from `origin.code` (`OriginalChanged`,
whose `Display` is `sentence::original_changed`, "Game of Life has changed since this conversion."),
so an edit outside the code, such as a note, a comment or the purpose, keeps the changes listed. It
diffs the copy's nodes against the sheet's lowered graph by node id across passes and functions:
added, removed, and changed when kind, config, inputs, region or canvas differ. Each node goes under
the first frame of its canvas holding it in the copy, else in a fresh `frames` of the sheet, so a
removed node lists under the frame it sat in at conversion; others go in a group with no title per
canvas. Frames match by canvas and title. Groups follow the conversion's frames, then the copy's own
frames, then the untitled groups, each list in id order. Declarations are not compared, so a
`SetVariantColor` changes the code and lists nothing.

**The lowering's half.** `Builder::record_result` keeps the temporary id of each result line's
`Operand::Link` node (`CardLowering::line`) and each Look around's fold (`look.rs`), and `finish`
renumbers them into `LineMap::insert_result`. The table is outside the document, so code identity is
untouched.

**Tests** sit in `src/tests/sheet_convert.rs`: the copy's header and its passes equal to the lowered
graph's, `a_taken_id_or_name_numbers_the_copy`, `the_copy_has_its_own_code_and_names_the_sheets`,
`the_copy_frames_each_origin_group` (Game of Life's seven frames exactly, every node in one frame),
labels and no `pos`, the notes, `every_sheet_starter_converts_and_checks_clean` (over `starters()`,
no diagnostic, every node in one frame, every frame origin a line id or a part path under
`PART_ROOTS`, no title holding a raw path, an ending ": " or `SHEET_PART_WORDS`, no change against
its own sheet),
`the_origin_record_survives_a_round_trip`,
`a_sheet_with_an_error_or_a_skipped_line_does_not_convert`,
`the_converted_game_of_life_runs_tick_for_tick_like_the_sheet_on_the_host` (64 by 64, 200 ticks at
seed 7, Randomise at 60 and 160, Clear at 150, lanes equal every tick),
`a_highlife_edit_is_found_by_changes_since_conversion` (line 2's `n2001` made an or of two new
comparisons: one changed node under line 2's frame, two added in no frame; a removed `n3002` under
line 3's), `changes_need_the_sheet_the_copy_was_converted_from`,
`make_function_keeps_frame_members`, `copy_carries_frame_membership` and
`set_variant_color_round_trips_and_changes_the_code`. The inline test of `convert.rs`
(`agent_paths_group_and_title_like_cell_paths`) reads the boids test sheet through
`tests::sheet_support::sheet_text` and checks `writers`, `sheet_origins`, `sheet_title` and
`property_read` on agent paths, since the grid lowering writes none. henad-models'
`src/tests/sheet_convert.rs` runs the copy and the sheet on the device at 128 by 128 with the same
presses, lanes equal every tick and the Alive stat equal at the end.
