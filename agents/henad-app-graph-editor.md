# henad-app: the graph editor

`ui/editor/graph/` is the canvas of the Editor tab (`agents/henad-app-editor.md`, which holds the
Apply path of a graph, its preview, the expert mode and the tests). It edits a graph model through
henad-graph's `GraphBuilder` and `EditCommand`s (`agents/henad-graph-edit.md`).

`mod.rs` holds `GraphEditor`: the builder, `History`, the open `Scope`, the breadcrumb `trail` (the
bodies a function was entered from through calls), one `BodyCanvas` per body opened (keyed by
scope), the `CheckReport` and a per-scope map of `NodeIssues`. `diagnostic_scope` puts a diagnostic
in its pass, or in the function that holds its node (a function's diagnostics carry a node and no
pass), and `issue_counts(scope)` counts them. Each canvas takes an egui id of its own from the load
counter and the editor's count of canvases made (`canvases_made`), never from the scope's name.
egui-snarl keeps the selection, the transform and the node sizes under that id, and a body reopened
after its canvas was dropped, or a new pass that takes a renamed pass's id, would otherwise read a
stale selection by pin index. Every gesture is a `GraphRequest` (`Edit(GraphEdit)` of commands with
a coalesce key, `AddNode` with an optional `DroppedWire`, `Undo`, `Redo`, `Copy`, `Cut`, `Paste`,
`Delete`, `Arrange`, `Open` (clears the trail), `Frame`, `FrameAll`, `OpenCall`, `OpenParent`,
`MakeFunction`, `Ungroup`, `AddFunctionPort`, `PickImport`, `Import`, `CloseImport`). `graph_ui`
draws the side panel (`passes.rs`, `world.rs`), the toolbar and the canvas, then `settle` runs the
requests: while the primary button is down they wait in `pending`, and once it is up a drag's
`Move`s and any wire egui-snarl dropped without a callback (`lost_wires`) commit first, then the
requests, then the open canvas syncs. This order is the guard of trap 8 of record
#62: a removal while a wire is in flight panics egui-snarl. `edit` runs commands on a clone, so a
refused command changes nothing, and its message (capitalised, with a full stop) is `status`, a
`GraphStatus` whose `warning` makes it a warning notice. A copy's "Copied N nodes." is a note
(`GraphStatus::note`), an information notice. `world.rs` leaves a hidden parameter out, and its
world table and parameter defaults set `clamp_existing_to_range(false)`, so a value the file holds
outside a range is drawn as it is and left to the checker.
`refresh` checks again, rebuilds `issues` and reopens the first pass when the open one is gone.
The *code revision* counts changes of `GraphFile::code`'s hash, which `refresh` compares, so a
`Move` or Arrange changes the revision and not the code revision. `preview_key` and the `program`
cache follow the code revision, and a move leaves the preview running
(`moving_a_node_leaves_the_preview_running`).
Undo keeps the canonical text before each edit, `[editor]` included (`editor/history.rs`, 200
snapshots, which the sheet editor shares), and consecutive edits with one coalesce key make one step
(a literal dragged, a parameter's range, the world table) until the gesture ends. `settle` ends it
when the primary button goes up, before and after the batch the release runs, and when keyboard
focus moves (`pressed`, `focused`), so two drags of one field are two steps. The world table and a
parameter's range and default show `shown_world` and `shown_param`, the last waiting `SetWorld` or
`SetParam`, so a dragged field shows its value while the requests wait for the release. Keys over
the canvas while no field has focus: command Z, command shift Z or command Y, Delete or Backspace on
the selection, and egui's `Copy`, `Cut` and `Paste` events. Copy puts
`GraphBuilder::copy` text on the clipboard, and Paste places the nodes' top left at the pointer.

`canvas.rs` holds `BodyCanvas` (the `Snarl<NodeView>`, the `NodeId` to egui-snarl node map, the
drawn rectangles, the transform, `framing`), the viewer and `show_canvas`. `sync` brings the `Snarl`
to the document by difference (remove, insert, update in place, then wires sorted by `wire_order` so
a run is reproducible), never by rebuilding, which would move the selection. A `NodeView` holds the
pins in the order of the kind's resolved ports, outer outputs before inner outputs, so a pin index
maps back to a port name, and types and rates come from the report's `PassReport`. The colour of a
pin is its type and its shape its rate (`sockets.rs`, a data palette outside MCS). A constant input
edits in place (`DragValue`, a checkbox, a variant menu), a bound parameter shows as a chip, and the
node menu holds Frame, Copy, Inputs (bind to a parameter, set a constant, clear), Settings
(configuration keys with a short list of values), Add statistic (a tick-rate output in a stats pass)
and Delete. A node with an error takes a red 500 stroke and a red 900 badge, a warning alone orange,
and the badge's press frames the node, as does a press on a problem in the list under the canvas. A
framing holds over frames until the target's rectangle stops changing. A plain click selects through
the modifier write of record #62 around `show`. Quick-add (`quick_add.rs`) opens on the background
and on a dropped wire, and adds the node where the pointer was when the menu opened (`menu_origin`),
since egui-snarl passes the menu's top left: `offered_kinds(document, Option<&Pass>, ..)` keeps the
kinds the pass's stage (`ContextSet::allows`), backend and domain take (`fn.input` and `fn.output`
never), or in a function (`None`) the kinds with no sink effect (`NodeDef::sink`), the world's
domain and the function's own input and output (`function_takes`), that have a socket for the wire's
type, and that match every word of the query or a synonym. Opened inside a region's frame
(`region_at`, the deepest frame painted this frame), it offers the kinds a region body takes
(`ContextSet::region`, `region.put` included, `lane.set` not) and the node joins that region
(`GraphRequest::AddNode::region`), and outside every frame no region-only kind. Inside a frame,
`can_add` (the nesting rule of `can_join` for a new node) leaves out a region kind that cannot nest
there, and `GraphEditor::add_node` refuses the same cases. A Repeat added from
quick-add starts with `count` 1, `max` 1 and `config.acc = []`. Arrange (`arrange.rs`) places canvas
nodes (`CanvasKey`: a node, or a region's output half) in columns by longest path, each as wide as
its widest drawn node, a region's input half then moved right to the column before its first reader
(`the_input_half_of_a_region_sits_in_the_column_before_its_body`): a link to an inner output leaves
the input half, a link to an accumulator the output half, and every node inside a region reads the
input half and feeds the output half, so a body sits between its halves. Every canvas of the six
graph starters places every node in its `[editor]` region, laid out once by `arrange` with the
app's fonts and style and each frame as a group, and
`every_starter_opens_with_no_node_over_another` (`graph_canvas.rs`, with `setup_custom_fonts` and
`setup_custom_styles`) holds that no two drawn nodes of a starter's canvas intersect. A change to
the node drawing that breaks it calls for laying the starters out again. `place_unplaced` places an
output half only in a body placed from scratch, and counts a node as placed by
`EditorRegion::node_pos`, so a converted copy's labelled nodes with no `pos` are arranged too. It
runs at load, on every edit's draft before the snapshot (`GraphEditor::keep`, which `edit`,
`add_node`, `wrap` and `add_function_port` call, and `refuse` shows a refused command's message)
and after `restore`. Otherwise a node an
edit leaves unplaced, such as a new function port's node or an import from a file with no node
placement, reaches `BodyCanvas::moved` as a move, an edit of its own that clears redo after each
undo.

**Regions** (E2 step 1). A region draws as two canvas nodes (`NodePart`): the input half
(`BodyCanvas::nodes`, the region's inputs and inner outputs) and the output half
(`BodyCanvas::output_halves`, its accumulator outputs, titled "<title> output", no badge). Both map
to the region's id, so selecting or deleting either half acts on the region. The output half sits at
`[editor] node.<id>.output_pos`, or one column right of the region's contents, and a drag of it is
`MoveRegionOutput` (compared with `placed`, the position the last sync gave each canvas node).
`document_wires` and `output_pin` put a link to an inner output on the input half and any other
link, a bare one included, on the output half. Every region has a `CanvasFrame` (`region` set, no
title, `region_colors` by depth: gray 900, 800, 700 fills) around both halves and
`NodeBody::region_contents`, painted outermost first from the rectangles cached in the last frame,
which nests frames. `regions_entered` gives the region a node moved since the last sync joins on
release: the deepest region frame, drawn without the moved canvas nodes, that holds the node's
centre, never one around the node (the node menu's Move out of region reaches those) and nothing for
a node whose region moves with it. `settle` runs those `SetRegion`s in the drag's edit, after its
`Move`s, and while the button is down `show_canvas` strokes the target frames blue 500
(`drop_targets`). The node menu holds Move into region (every region outside the node's own subtree
but its current one) and Move out of region (to the region around its region). Move out of region
also moves the node, with every node inside it, right of the frame it leaves by `COLUMN_GAP`
(`moves_out_of_frame`, in the same edit) when any of them lies within `FRAME_MARGIN` of the frame
drawn without them. Otherwise a non-member stays drawn inside the frame and reads as a member. `can_join` filters
drop targets, both menus' targets and Move out of region by library 1's nesting: a plain node joins
any region, a Repeat holds a Repeat or a `grid.fold`, a `grid.fold` a Repeat, a radius fold holds no
region and sits in none, and the chain of regions stays within `MAX_REGION_DEPTH` (3). The toolbar's
Repeat (`MDI_REPEAT`) sends `GraphRequest::WrapInRepeat` of the selection, and `GraphEditor::wrap`
runs `EditCommand::WrapInRepeat` with the selection's drawn bounds, then `fit_wrap` moves the halves
clear of the wrapped nodes and the puts once the canvas has drawn them at one size for two frames,
inside the wrap's undo step. The input half of a region holds the region controls
(`region_controls`): a Repeat's Max (`SetConfig`, coalesced, `clamp_existing_to_range(false)` so
an out-of-range `max` is drawn as it is and left to the checker), one row per accumulator of any region
with `acc` (a name field that renames on losing focus, the type menu `accumulator_types`, the
combine menu filtered by `Combine::applies_to`, a type change keeping the combine when it applies
and taking `carry` otherwise, and Remove accumulator), and Add accumulator (`acc`, `f32`, `sum`).
`GraphEditor::edit` adds `max` = count clamped to 1 to 65536 after every constant `count` of a
Repeat (`fill_repeat_max`). `settings_lines` leaves out the keys the controls show.

**Functions** (E2 step 2). `functions.rs` is the functions panel under the pass stack: a row per
function (open, issue badges, `from <file>` with the origin's hash as hover), the open function's
forms (Name renames, each input and output with a name field, a type menu of `canvas::value_types`
and Remove, Add input and Add output, which `GraphEditor::add_function_port` runs as the port and a
`fn.input`/`fn.output` node naming it, Remove function), an add form, and Import function.
`PickImport` sets `pick_import`, which `editor_ui` turns into `OpenTarget::Function`, and
`receive_import` passes the file to `set_import_source`, which lists its functions (a file that is
no graph or holds none is a warning). Each listed function imports under the name in its field
(`GraphRequest::Import`). A call's node menu holds Open function (`OpenCall`, the open body pushed
on the trail) and Ungroup. The toolbar shows the breadcrumb (`breadcrumb_ui`: a link per trail
entry, `OpenParent(index)`, then the open body), Repeat in any body, and Function
(`MDI_FUNCTION_VARIANT`, `make_function`, named `function`, `function_2`, ...). A function's canvas
syncs with `CheckReport::functions`' report of its body. Settings lists functions for a call's
`function` and the open function's ports for `fn.input`/`fn.output`. An Expression node
(`math.expr`) draws `expression_editor` in its body: the text (multiline, commits on losing focus),
each input (name field that moves the input's value to the new port, `moved_input`, and refuses
another input's name through `GraphRequest::Refuse`, which sets the status line, a type menu and
Remove), Add input (`x`, `f32`), and the outputs as one comma-separated field. Its `text`, `inputs`
and `outputs` leave `settings_lines`.

`GraphEditor::program` compiles the document through `GraphProgram::compile` (`editor/program.rs`,
with `CompiledProgram` and `LOADS`, which the sheet editor shares), `GridProgram` or
`AgentProgram` by the world, once per code revision (`CompiledProgram`), for the generated WGSL view
and the preview. `wgsl_view.rs` is a right panel, toggled by the toolbar's WGSL switch
(`GraphRequest::ShowWgsl`), that shows `GeneratedShader::text` of each shader whose `passes` hold
the open pass as a selectable non-wrapping monospace label, and `HOST_PASS`, `DISABLED_PASS` or
`NOT_COMPILED` otherwise. View in viewport (`view_pass.rs`) is a node-menu submenu over the outputs
`can_view` accepts (element rate, `f32`, `u32`, `i32`, `bool` or a category) of a node outside every
region. `GraphRequest::View` sets `viewing` (a `ViewTarget`) when `view_file` builds and checks: on
a copy of the builder it switches every view pass off, adds the palette `view_ramp` (`RAMP`, nine
viridis stops, a data palette) and the view pass `view_in_viewport`, pastes `upstream` (the node and
everything it reads, regions whole) into it, and colours the value (a category by its own palette, a
`u32` as an index, an `i32` and an `f32` through `view.ramp`, a flag as 0 or 8). The document never
changes, and a refusal (a draw, which a view pass cannot make) is `status`. `preview_program`
compiles the view file while `viewing` holds, and drops a view the document no longer allows with
"Stopped viewing ...". `preview_key` (load, revision, view changes) is what the preview compares.
