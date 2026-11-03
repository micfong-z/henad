# henad-graph

**henad-graph**: graph models, the models a researcher builds in the node editor. It sits above
henad-compute and outside egui. `agents/dependencies.md` holds its edges. Node models compile to
WGSL alone, so a file has one `[[target]]` with `backend = "gpu"` and the grid and agents
topologies only. Network topology, sequence passes and CPU interpreters are outside this crate.

Modules. `document.rs` holds `GraphDocument`, every table before `[editor]`, as plain data with
public fields. `node.rs` holds `Node`, `NodeBody` (`node`, `region_members`, `region_path`,
`duplicate_ids`), `Accumulator` and `Combine`. `value.rs` holds `Literal`, `InputValue`,
`ConfigValue`, `PortRef` and `MemberName`. `types.rs` holds `ValueType` and `Rate`, and `ids.rs`
holds `NodeId` and the grammars `is_id` and `is_port_segment`. `limits.rs` holds the limits of
`ir-spec.md` section 13 as the plan changes them. `model_text.rs` holds what every kind of model
file shares (`LineEnds`, `toml_string`, `ParamTable`, `write_param`), and henad-explore's
`model_file.rs` reads them from here. `reader.rs` is private and turns text into the document and
`Places`. `writer.rs` is private and writes the semantic text. `editor.rs` holds `EditorRegion`.
`file.rs` holds `GraphFile`, `GraphFileError`, `Places` and the constants `FORMAT`,
`FORMAT_VERSION`, `KIND_GRAPH`, `LIBRARY` and `CODE_FORMAT`, and the crate-private `Placer`, which
places many diagnostics with one scan of the text. `catalogue/` holds the node kinds, `draw.rs`
the counter-based draw words, `check/` the checker, `host/` the host evaluator and `edit/` the
edit commands and `GraphBuilder`. `starters/` at the crate root holds the grid starters
(`game_of_life`, `vote`, `sir`), ported to one GPU target with `node_<model>` ids and no `apply`
key. `starters.rs` embeds them with `include_str!` (`scripts/check_packaging.sh` refuses a path
out of the crate) and returns them through `starters()` in editor order and `starter(file_name)`.
Game of Life's Randomise is the 9-node press-seed form (`ctx.press_seed`, `ctx.index`,
`rng.hash`, `bits.xor`, `rng.hash`, `rng.threshold`, `rng.below`, `logic.select`, `lane.set`).
The exact ports (`gpu_sir_exact.toml`) sit in henad-models' `tests/fixtures/graphs/`, and
henad-models' `src/tests/graph_corpus.rs` holds every grid corpus file's init to its stock seed
(`agents/models.md`).

**The reader** checks the header first, as `model_file.rs` does: `format`, `format_version`,
`kind` and `[graph] library`, ignoring other keys. It checks `MAX_FILE_BYTES` before any of that,
and holds the canonical text to the same size after the read, so every file that reads has a
canonical text that reads back (`a_file_whose_canonical_text_passes_the_limit_is_refused`).
Every semantic table is `deny_unknown_fields`, which is how an `apply` key, `[[pass.var]]`, `after`
and `[network]` are refused. The reader checks only what the document's own types need: table
shapes, node ids, port and config key grammars, type strings, literal forms and finite floats. It
leaves what a graph means to the checker. It keeps a duplicate node id in the body, as the repair
state needs, and `NodeBody::duplicate_ids` lists them. It sorts passes by stage and nodes by id,
each stably, and moves their `Places` with them, so `document.passes[i].body.nodes[j]` matches
`places.passes[i].nodes[j]`. A table's place is its header, since `toml::Spanned` reports that.

**The `in` walk.** TOML stores `in.init.position` as nested tables, and `{ param = "x" }` under a
port is a value. The walk reads the first segment and descends by a fixed table
(`config_port_depth`): `init` and `case` one more segment, `rule` two, every other name none. A
`fn.call` descends nowhere, since its interface ports are bare and one can be named `init`. An
accumulator named `param` or `u32` therefore reads correctly, and a test holds that.

**The canonical writer.** `GraphFile::to_toml` is `semantic_text` plus the region's text. Rules
kept from `ir-spec.md` section 10: `runs = []` is written, an optional key without a default is
not, `draw_order` only when non-empty, colours as four bytes, `u64` as lowercase `0x`, floats in
`{:?}` form. `write_node` reads the node's definition: config keys in definition order with their
defaults filled (`effective_config`), then inputs in `resolved_inputs` order (fixed ports, then
ports configuration makes, in the configuration's order) with a literal default filled, and a
`region.put` feeding a `count` accumulator gets `in.value = { u32 = 1 }`. A key the definition
does not name, or a kind with no definition, follows in lexical order, so a broken file still has
one text. `normal_config` writes a `U64Text` value as lowercase hexadecimal. A `ConfigValue::Table`
follows `INLINE_KEY_ORDER`, then lexical order. An `init.<acc>` stays unwritten when absent, since
a float `min` identity has no literal form.

**The catalogue.** One `NodeDef` per kind, `static` tables in `catalogue/state.rs`, `math.rs`,
`bits_logic.rs`, `conv.rs`, `random.rs`, `region.rs` and `view.rs`, filled from the `BASE`
constant and group constants with `..`. `definitions()` lists them, and `lookup(op, library)`
returns the newest with `since <= library`. A def holds: one type variable (`type_var`, a set of
`TypeClass`), `ConfigKey`s with a `ConfigType` and an optional default, fixed `InputSocket`s
(type, fastest accepted `Rate`, `InputDefault`), `OutputSocket`s (`OutputRate::Join` or
`Pinned`), `ConfigPorts` for ports configuration makes, `inner_outputs` and `is_region` for a
region, `ContextSet` (stages plus a `region` flag), an optional `Domain`, `Effect`s, `Backends`,
`Parity`, `NodeCost` and `KnownAnswer`s. `resolved_inputs`, `resolved_outputs` and
`resolved_inner_outputs` expand a node's ports, and the writer, the checker and every later
consumer read ports through them. The milestone G1 kinds are all in: 64. A kind with no row in
`node-catalogue.md` has its definition written here, with WGSL's integer rules and the NaN
behaviour stated, and the definitions say a shader may assume no NaN or infinity.

**Known-answer cases** (`catalogue/cases.rs`). `CaseForm::Node` gives config lines, input values,
a `CaseContext` (seed, phase, element, tick, iteration, press, stream state) and expected outputs.
`CaseForm::Pass` gives tables after a grid header, the lanes before, and lanes, stats and colours
after the pass with id `case`, other passes being there only so the file checks. `CaseScope::Host`
marks an answer WGSL leaves indeterminate (NaN, infinity, `0^0`). `CaseValue::Nan` and `Near`
compare loosely, and every other float compares by bits. `src/tests/case_files.rs` builds a file
from each case over a fixed case world (categories `health` S, I, R and `life` Dead, Alive, the
palette `ramp` of four colours). `known_answers.rs` runs every case, node and pass, through the
host evaluator. A node case's inputs go into the document as they are, so a NaN or an infinity
reaches the node although a file cannot write one. G2's WGSL runner must run the same cases.

**Draws** (`draw.rs`). `key_word(key, phase, seed)`, `draw_bits(k, element, tick, iteration,
sub_draw)` and `press_word(seed, press)`, with `GRAPH_DEFAULT_SEED` and `GRAPH_SEED_SALT`. The
core is the provisional `chainm` of the PractRand gate, which failed both candidates: the
maintainer chooses the core, and the `rand.*` cases and their lock lines move with it. Phase
words: `PHASE_INIT` 0, `PHASE_TICK` 1 (view and stats too, which draw nothing), and
`action_phase(j)` = 2 + j for the action at position `j` of `[[action]]`, its reruns included.
The tick word `t` is the tick in a tick pass, the press count in an action, 0 in init.
`iteration_word(outer, max, k)` is `k` at the top and `outer * max + k` nested, so a draw inside
a Repeat reads `Some(k)`, and one outside every region reads `None`. WGSL must match all of this.

**The host evaluator** (`host/`). `HostEvaluator::new(&GraphFile)` checks the file (errors give
`HostError::Checked`) and compiles every pass once (`compile.rs`). A pass the host cannot run
compiles to an error that `run_pass` returns, and only an init pass or a rerun pass with one
refuses the whole file. `init(world, params, seed)` runs the enabled init passes in stack order
on the init stream (`grid_init_rng` or `agent_init_rng`, continuing across passes) and returns
`HostLanes`. `rerun(action, press, ..., stream)` runs the init passes of the action's `runs` in
list order on the caller's action stream (start it at `action_seed(seed)`), skips tick passes,
which run on the device, and returns the positions of the written lanes to upload.
`run_pass(id, lanes, params, context, stream)` runs any pass, which is how the cases run tick,
view and stats passes, and returns `PassOutcome` (uniform values by node and port, colours,
written lanes). `params` are the file's parameters in schema order, without the engine's grid
or agent prefix. A missing or mistyped value takes its default. `HostWorld` is the grid size or
the agent count, and `HostLanes` holds one `LaneColumn` per `[[lane]]` as GPU words (bool 0 or
1, a category its variant number, two words for a vec2, u64 or f64, low first). Narrowing to a
lane's `storage` is the upload's job. A pass reads its start and commits at its end (written
lanes are staged copies). The top of a pass splits into prelude (uniform, run once), element
block, and post block (reads a reducer, run once after the elements). An element value that
reads a reducer is refused. Each scope orders its members by Kahn's algorithm with ties by node
id, a region counting every link of its body, and consecutive `draw_order` entries add edges. A
Repeat seeds each accumulator (`init` or the combine's identity: largest finite value for
`min`, smallest for `max`), copies the running value to `prev.<acc>` at the start of each
iteration, and a `region.put` folds into the running value. Nothing panics on a checked file:
a bad value is `HostError::Fault` with the pass and node, and the mutation test runs the host
on every edited file that checks. `run_pass` and `rerun` first check that the `HostLanes` hold
the document's lanes (count, ids, types, one value per element) and return `Fault` otherwise.
`HostLanes::new` refuses lanes past `MAX_HOST_LANE_BYTES` (4 GiB) before allocating, and every
column and staged copy is reserved with `try_reserve_exact`, so a failed allocation is an error
and no abort. Host init has no work bound yet: `cost * elements` of the init and rerun passes is
for G2's `Demand` to count.

**Edit commands** (`edit/`). `GraphBuilder` holds a `GraphDocument` and an `EditorRegion` and
runs `EditCommand`s: `AddNode`, `RemoveNode`, `Connect`, `Disconnect`, `SetLiteral`,
`BindParam`, `SetConfig`, `SetRegion`, `Move`, `Frame`, `Paste`, and the declaration edits
(add a category or palette, add, rename and remove a lane, variant, parameter, statistic,
action or pass, and `MovePass` within its stage). `apply` runs on a copy, so a refused command
changes nothing. Commands check structure only (ids, grammars, the body a node is in, region
cycles) and leave meaning to the checker. A new node takes `max(highest + 1, next_id)` and
writes `next_id` (an `[editor]` region is made if absent), and a kind with a `key` config gets
a random key distinct from the file's (`with_key_seed` fixes the stream for tests). A stream
draw joins its pass's `draw_order`. `RemoveNode` drops wires from the node and its placement,
moves a region's members to the region around it, and is refused while a `[[stat]]` reads it.
A rename rewrites every reference: `config.lane`, category literals, `dead`/`alive`, rule
`from`/`to`, `case.<v>` ports, `stat.count_by` ports and `[[stat]] from`, parameter bindings,
`enabled`, option literals, `action`, `runs` and the canvas scope `pass.<id>`. A remove with
references returns `EditError::Referenced` with each place. `copy(scope, nodes)` returns a whole
graph file (declarations, one pass or function of the copied nodes, their placement, no stats,
empty `runs`). `Paste` mints ids and keys, keeps wires inside the copy, drops the rest and drops
a binding of a parameter the target lacks. Category rename and remove, palette edits and
function edits are not written yet.

**The checker** (`check/`). `GraphFile::check()` returns a `CheckReport`: every diagnostic in file
order, `duplicate_ids` (the repair state), a `PassReport` per pass (cost, and the inferred
`PortType` of each output and inner output) and `dual_lanes`. `view.rs` builds a `BodyView` per
body: positions by id, defs, and the region tree (`parents`, `depths`) built in one linear walk
that reports a cycle and nesting past `MAX_REGION_DEPTH`. `body.rs` types each body in link order
(sources first, nodes on a cycle last, reading holes), checks contexts, backends (init runs on the
host, every other pass on the GPU), configuration (`config.rs`), inputs, rates, link visibility
across regions, puts, host types, and warns of unreached nodes. `bounds.rs` warns when a Repeat's
`count` can pass `max`. `cost.rs` sums costs bottom-up with saturating `u64`, functions first in
call order with Kahn's algorithm, so no step recurses on a hostile file. `passes.rs` holds the
rules across a pass and across passes, and `declarations.rs` the tables. Every pass needs a
`domain`, stats passes included, since library 1 has no domain-free reducer. `schedule.rs` orders
the top of each pass (a region stands for its body) and splits it as the host does into prelude,
element and post: a node that runs per element and reads a reducer's result of its own pass is an
error at the input that reads it (`ir-spec.md` section 6.1, reducers end a map pass), and tick-rate
arithmetic after a reducer is allowed. It also reports a stream draw whose sources reach a draw
that `draw_order` lists after it (V17). `lane.set` is allowed in init, tick and action passes
only, never in a view or stats pass or a region body. Diagnostics of a node
carry its pass and node id. `fn.call`, `fn.input` and `fn.output` are A2 kinds: today they report
as unknown, while cost, call depth, call cycles and inlined size already follow `fn.call`.

**The library lock.** `library.lock` at the crate root, one line per definition (hashes of the
signature's `Debug`, the semantics and the cases' `Debug`, parity, cost) and one per limit.
`the_library_lock_matches_the_catalogue` fails on a kind without cases and on any line that
differs. `HENAD_WRITE_LIBRARY_LOCK=1` writes it. Renaming a catalogue field moves every signature
hash. The test prints the corpus files that use each kind (`--nocapture`), which is the "Used by"
information, and a coverage line, the kinds the corpus uses out of every kind.

**The editor region** keeps a `toml::Table`, never refuses a value and never fails a read. Its
writer orders the keys and rounds `pos` to whole points, and `to_text` is a fixed point for any
table, which `a_strange_editor_region_stays_readable_and_a_fixed_point` holds. The writer leaves
out a value nested deeper than `MAX_INLINE_DEPTH` (64) arrays and inline tables, since the `toml`
reader refuses nesting past 80 and a dotted key in the source is written as nested inline tables
(`a_deep_dotted_editor_key_still_has_a_canonical_text_that_reads`).

**Code identity.** `GraphFile::code` is `ModelCode::new(CODE_FORMAT, semantic_text)`, and
`GraphFile::source` is `ModelSource::runtime(target id, code)`, which records
`CodeOrigin::Runtime`. `from_code` refuses another format with the message
`Recorded under X, and this build runs henad-graph/1`.

**Outside the crate.** henad-explore's `ModelFile::Graph` holds a `GraphFile`, and its `entry`
returns the checker's errors, then `HostEvaluator::new`'s, then `ModelFileError::Unsupported`
until G2's WGSL compiler builds an entry. henad-cli's `--model-file` prints a graph's warnings
and errors with places through it. The facade's `henad::graph` (feature `model-files`) names the
crate's root items and public modules. G2 must give a GPU grid entry the schema that
henad-models' `graph_corpus.rs` builds: `grid_width` and `grid_height` (1 to 16384, defaults from
`[grid]`), then the file's `[[param]]`s, its stats by label and its actions by id. Its
`seed_buffers` is `init(HostWorld::Grid { width, height, boundary }, &params[2..], seed)` and
`lanes.column(id).words()` per buffer, narrowed to the lane's storage.

Tests sit in `src/tests/`: `file_format.rs` with `full_graph.toml` as a file in a hand-written
spelling, `catalogue.rs`, `known_answers.rs`, `checker.rs`, `host.rs` (corpus inits replay,
reruns, a Repeat equals its hand-chained copies bit for bit, mixed-radix draws), `edit.rs`,
`library_lock.rs` and `mutation.rs`.
`corpus.rs` reads the starters through `starters()` and, at run time, the graphs in
`crates/henad-models/tests/fixtures/graphs/`, since a packaged crate has no path out of itself.
`two_spellings_of_one_graph_hash_alike`, `a_canonical_text_reads_back_to_itself` and
`moving_a_node_changes_the_text_and_leaves_the_hash` are the gate of the file format, and
`the_reader_and_checker_never_panic_on_the_mutation_corpus` and
`a_hostile_region_tree_or_call_graph_is_reported_without_a_panic` the gate of the checker. Keep
them.
