# henad-graph: edit commands and the editor region

`agents/henad-graph.md` holds the rest of the crate. The Editor tab of henad-app runs every gesture
through these commands (`agents/henad-app-editor.md`).

**Edit commands** (`edit/`). `GraphBuilder` holds a `GraphDocument` and an `EditorRegion` and runs
`EditCommand`s: `AddNode`, `RemoveNode`, `Connect`, `Disconnect`, `SetLiteral`, `BindParam`,
`SetConfig`, `SetRegion`, `Move`, `Frame`, `Paste`, and the declaration edits (add a category or
palette, add, rename and remove a lane, variant, parameter, statistic, action or pass, and
`MovePass` within its stage), and three edits in place for the Editor's forms: `SetPassEnabled` (a
parameter it binds must exist), `SetParam` (the descriptor with the same id, in its place in the
schema) and `SetWorld` (the same topology), and the region edits below. `apply` runs on a copy, so a
refused command changes nothing. Commands check structure only (ids, grammars, the body a node is
in, region cycles) and leave meaning to the checker. A new node takes `max(highest + 1, next_id)`
and writes `next_id` (an `[editor]` region is made if absent), and a kind with a `key` config gets a
random key distinct from the file's (`with_key_seed` fixes the stream for tests). The keys come from
the public `keys::KeyStream` (`keys.rs`), an `xorshift64` stream seeded from entropy
(`from_entropy`, a `RandomState` hash of a counter, so two streams differ where the hasher keys are
fixed) or from a seed (`seeded`, through `mix_seed`). `next_key` returns the high 32 bits of the
next state and `next_key_not_in` skips the keys a closure calls taken. The graph builder and the
sheet builder (`agents/henad-graph-sheet-edit.md`) are its two callers, and
`a_seeded_key_stream_gives_its_known_first_keys` holds the first three keys of seed 1, so a rebuild
test can name its keys. A stream draw
joins its pass's `draw_order`. `RemoveNode` drops wires from the node and its placement, moves a
region's members to the region around it, and is refused while a `[[stat]]` reads it. A rename
rewrites every reference: `config.lane`, category literals, `dead`/`alive`, rule `from`/`to`,
`case.<v>` ports, `stat.count_by` ports and `[[stat]] from`, parameter bindings, `enabled`, option
literals, `action`, `runs` and the canvas scope `pass.<id>`. A remove with references returns
`EditError::Referenced` with each place. `copy(scope, nodes)` returns a whole graph file
(declarations, one pass or function of the copied nodes, their placement, no stats, empty `runs`).
`Paste` mints ids and keys, keeps wires inside the copy, drops the rest and drops a binding of a
parameter the target lacks. Category rename and remove and palette edits are not written yet.

**Function edits** (`edit/functions.rs`). `AddFunction` (no ports, empty body), `RenameFunction`
(every call's `config.function`, the canvas scope, the `[editor] function.<id>` entry),
`RemoveFunction` (refused while a call names it), and the port edits over a `PortSide`
(`AddFunctionPort`, `RenameFunctionPort` with the `fn.input`/`fn.output` nodes, each call's input
or each reader and `[[stat]]` of each call's output, `SetFunctionPortType`, `RemoveFunctionPort`,
refused while a body node names the port or a reader reads it, an input leaving every call).
`MakeFunction { scope, nodes, id }` takes a selection as `WrapInRepeat` does (`selection`, top
nodes of one region, their subtrees): refused for a sink (`SINKS`, as the checker's), a put into a
region outside the selection, a `fn.input`/`fn.output`, or a node a `[[stat]]` reads. Each value
read from outside (a named outer output, or an inner output of a region around the selection) is
an input typed by the checker, each output a node outside reads is an output, nodes keep their ids
and placement, top nodes leave their region, and a `fn.call` with a fresh key takes their place in
the shared region. Its draws then take the call's key on top of their own, so they draw other
numbers. `Ungroup { scope, node }` copies the called body with fresh ids where the call sat, maps
`fn.input` to the call's inputs and the call's readers (and a `[[stat]]` reading a node output)
to the `fn.output` values, and gives each counter draw `compose_key(call key, key)`, so the copy
draws what the call drew. A call inside the body takes the composed key as its own key, and the
draws inside it draw other numbers. `ImportFunction { text, source, function, id }` copies the
function and every function it calls (`imported_functions`) from another graph file with fresh node
ids, placement and keys kept, refuses an id the file holds ("Import it under another name"), a
new id one of its callees already takes, a newer library, and text that does not read (`EditError::Import`), and records
`[editor] function.<id>.origin = { name, hash }` per copied function (`FunctionOrigin`,
`EditorRegion::function_origin`, `set_function_origin`), the hash FNV-1a 64 of
`writer::function_text` in the source file, as 16 hex digits. An import copies no declaration, so
a function that reads a field the file lacks reports it at check. Tests: `src/tests/functions.rs`.

**Region edits** (`edit/regions.rs`). `AddAccumulator`, `SetAccumulator` (type and combine of a
named accumulator), `RenameAccumulator` (the `init.<acc>` input, the puts that are direct members,
links to `<acc>` and `prev.<acc>`, a `[[stat]]`) and `RemoveAccumulator` (refused with the puts,
readers and `init.<acc>` that name it) act on a region's `config.acc` through `Accumulator::new`
and `to_config`, and refuse a name outside the port grammar, a duplicate, an inner output's name
and a seventeenth accumulator. `Combine::ALL` and `Combine::applies_to` (moved from the checker)
serve the Editor's menus. `MoveRegionOutput` sets `[editor] node.<id>.output_pos`, the canvas
position of a region's output half (`NodeLayout::output_pos`, rounded like `pos`, offset by a
paste). `WrapInRepeat { scope, nodes, bounds }` wraps nodes of a pass in a new Repeat in the
region the selection's top nodes share (refused for nodes of two regions or no node, a put whose
region the selection leaves out, a kind a region body refuses, such as `lane.set`, and nesting
library 1 does not allow: `check_wrap_nesting` refuses a Repeat in a region other than a Repeat or
`grid.fold`, a wrapped region other than those two, and regions past `MAX_REGION_DEPTH` once each
wrapped region moves a level down), in a pass or a function, typed through `body_report`:
the Repeat takes `count` 1 and `max` 1, its members follow the top nodes, and each outer output of
the wrapped subtree that a node outside reads (typed by the checker, refused when unknown) becomes
a `carry` accumulator named after the port, started at zero or the first variant, fed by a new put
and read by the reader instead. A Repeat that runs once computes the pass as before, draws
included, since the iteration word is 0 outside every region and at `k` 0
(`wrapping_nodes_in_a_repeat_keeps_what_the_pass_computes`). The halves go left and right of
`bounds`, the drawn rectangle, or of assumed node sizes.

**Bare links.** `name_bare_links` names the port on every bare link to a node, and on a `[[stat]]`
reading it bare, while the node has one output. `SetConfig` and `AddAccumulator` run it on their
node first, and `SetRegion` names each bare link from the moved subtree to a region on the new
path (`name_links_into_path`), where a bare link names nothing. Without it a second accumulator
turns a canonical text's bare `"n1"` into a link to one of several outputs. A call's outputs are
its function's declared outputs, so `AddFunctionPort` of a second output runs it on every `fn.call`
of the function in every body. A `fn.output` node or its `port` changes no call's outputs.

**The editor region** keeps a `toml::Table`, never refuses a value and never fails a read.
`frames()` reads every `[[editor.frame]]` as a `FrameLayout` for the Editor's canvas. Its
writer orders the keys and rounds `pos` to whole points, and `to_text` is a fixed point for any
table, which `a_strange_editor_region_stays_readable_and_a_fixed_point` holds. The writer leaves
out a value nested deeper than `MAX_INLINE_DEPTH` (64) arrays and inline tables, since the `toml`
reader refuses nesting past 80 and a dotted key in the source is written as nested inline tables
(`a_deep_dotted_editor_key_still_has_a_canonical_text_that_reads`).
