# henad-graph

**henad-graph**: graph models, the models a researcher builds in the node editor. It sits above
henad-compute and outside egui. `agents/dependencies.md` holds its edges. Node models compile to
WGSL alone, so a file has one `[[target]]` with `backend = "gpu"` and the grid and agents
topologies only. Network topology, sequence passes and CPU interpreters are outside this crate.

Modules. `document.rs` holds `GraphDocument`, every table before `[editor]`, as plain data with
public fields. `node.rs` holds `Node`, `NodeBody` (`node`, `region_members`, `region_path`,
`duplicate_ids`), `Accumulator` and `Combine`. `value.rs` holds `Literal`, `InputValue`,
`ConfigValue`, `PortRef` and `MemberName`. `types.rs` holds `ValueType` and `Rate`, and `ids.rs`
holds `NodeId` and the grammars `is_id` and `is_port_segment`. `keys.rs` holds `KeyStream`, the
stream of draw keys the graph and sheet builders share (`agents/henad-graph-edit.md`).
`limits.rs` holds the limits of
`ir-spec.md` section 13 as the plan changes them. `model_text.rs` holds what every kind of model
file shares (`ModelHeader`, `HeaderProblem`, `read_header`, `LineEnds`, `toml_string`, `ParamTable`,
`write_param`), and henad-explore's `model_file.rs` reads them from here. It also holds the
crate-private text helpers both writers use: `push_key`, `string_array`, `multiline_array`,
`color_array` (a graph file's four bytes) and `color_hex` (a sheet's `"#RRGGBB"`). `sheet/` holds
the author document, a model file of kind `sheet`, and `agents/henad-graph-sheet.md` describes it.
`reader.rs` is private and turns text into the document and `Places`. `writer.rs` is private and
writes the semantic text. `editor.rs` holds `EditorRegion`. `file.rs` holds `GraphFile`,
`GraphFileError`, `Places` and the constants `FORMAT`, `FORMAT_VERSION`, `KIND_GRAPH`, `LIBRARY`
and `CODE_FORMAT`, and the crate-private `Placer`, which places many diagnostics with one scan of
the text. `catalogue/` holds the node kinds, `draw.rs` the counter-based draw words and `draw.wgsl`
their WGSL twin, `check/` the checker, `host/` the host evaluator, `wgsl/` the WGSL compiler of grid
and agents graphs and `edit/` the edit commands and `GraphBuilder`. henad-graph takes henad-compute
with `runtime-wgsl` on, for the public specs and `compose_wgsl`. `starters/` at the crate root holds
the grid starters (`game_of_life`, `vote`, `sir`) and the agent starters (`boids`, `ants`,
`random_walk`), ported to one GPU target with `node_<model>` ids and no `apply` key. `starters.rs`
embeds them with `include_str!` (`scripts/check_packaging.sh` refuses a path out of the crate) and
returns the grid ones through `starters()`, the agent ones through `agent_starters()`, and either
through `starter(file_name)`. The Editor's starter menu lists both, grid first, and the grid-only
tests read `starters()`. Both compile to WGSL.
Game of Life's Randomise is the 9-node press-seed form (`ctx.press_seed`, `ctx.index`,
`rng.hash`, `bits.xor`, `rng.hash`, `rng.threshold`, `rng.below`, `logic.select`, `lane.set`).
The exact ports (`gpu_sir_exact.toml`) sit in henad-models' `tests/fixtures/graphs/`, and
henad-models' `src/tests/graph_corpus.rs` holds every grid corpus file's init to its stock seed
(`agents/models.md`).

**The header.** `ModelHeader::read` reads `format`, `format_version`, `kind`, `[graph] library`
and `[sheet] version`, ignoring every other key, and `ModelHeader::check(kinds)` refuses another
format, another version or a kind outside `kinds`, naming them as a list ending in "or" ("this
reader only reads 'gpu_grid' or 'graph'"). The graph reader, the sheet reader and henad-explore's
`ModelFile` and `GridRuleFile` all call it, and each reader checks its own version key after it.
`read_header(text)` returns the kind of any TOML text, a file its reader refuses included. It reads
`kind` alone, so a header key of the wrong type cannot hide it.

**The reader** checks the header first through `ModelHeader` with the kinds `["graph"]`, then
`[graph] library`. It checks `MAX_FILE_BYTES` before any of that, and holds the canonical text to
the same size after the read, so every file that reads has a canonical text that reads back
(`a_file_whose_canonical_text_passes_the_limit_is_refused`).
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
one text. `normal_config` writes a `U64Text` value as lowercase hexadecimal and a `CountList`
(`grid.life_like`'s `birth` and `survive`) ascending with each count once, since both are sets. A
`ConfigValue::Table` follows `INLINE_KEY_ORDER`, then lexical order. An `init.<acc>` stays
unwritten when absent, since a float `min` identity has no literal form. `normal_link` writes a
link bare when it names the only output of a node outside every region around the reader, and
keeps the port of a region read from its body (inner outputs). `normal_stat_source` does the same
for `[[stat]] from`. `respellings_of_the_life_starter_write_one_text` holds the four respellings the
round 1 audit found. Because a canonical text holds bare links, `AddVariant` and `RemoveVariant`
first name the variant on a bare link to a `stat.count_by` over a one-variant category
(`name_bare_counter_links`), which would otherwise name nothing or hide a reference.

**The catalogue.** One `NodeDef` per kind, `static` tables in `catalogue/state.rs`, `math.rs`,
`bits_logic.rs`, `conv.rs`, `random.rs`, `region.rs` and `view.rs`, filled from the `BASE`
constant and group constants with `..`. `definitions()` lists them, and `lookup(op, library)`
returns the newest with `since <= library`. A def holds: one type variable (`type_var`, a set of
`TypeClass`), `ConfigKey`s with a `ConfigType` and an optional default, fixed `InputSocket`s
(type, fastest accepted `Rate`, `InputDefault`), `OutputSocket`s (`OutputRate::Join` or
`Pinned`), `ConfigPorts` for ports configuration makes, `inner_outputs` and `is_region` for a
region, `ContextSet` (stages plus a `region` flag, `allows(stage)` the stage test the checker and
the app's quick-add share), an optional `Domain`, `Effect`s (`NodeDef::sink` naming the first one no
function body holds), `Backends`,
`Parity`, `NodeCost` and `KnownAnswer`s. `resolved_inputs`, `resolved_outputs` and
`resolved_inner_outputs` expand a node's ports, and the writer, the checker and every later
consumer read ports through them. The kinds of G1 and A2 are all in: 114. A kind with no row in
`node-catalogue.md` has its definition written here, with WGSL's integer rules and the NaN
behaviour stated, and the definitions say a shader may assume no NaN or infinity. A float to
integer conversion follows WGSL, which naga emits as a clamp before the cast: `conv.to_u32` from an
`f32` tops out at 4294967040 (`0x4f7fffff`) and `conv.to_i32` at 2147483520 (`0x4effffff`), the
largest `f32` values the target holds, and the host form clamps the same way
(`host/value.rs`). An Apple M4 Pro returned exactly these values.

**Known-answer cases** (`catalogue/cases.rs`). `CaseForm::Node` gives config lines, input values,
a `CaseContext` (seed, phase, element, tick, iteration, press, stream state) and expected outputs.
`CaseForm::Pass` gives tables after a grid header, the lanes before, and lanes, stats and colours
after the pass with id `case`, other passes being there only so the file checks. `CaseScope::Host`
marks an answer WGSL leaves indeterminate (NaN, infinity, `0^0`). `CaseValue::Nan` and `Near`
compare loosely, and every other float compares by bits. `src/tests/case_files.rs` builds a file
from each case over a fixed case world (categories `health` S, I, R and `life` Dead, Alive, the
palette `ramp` of four colours). `known_answers.rs` runs every case, node and pass, through the
host evaluator. A node case's inputs go into the document as they are, so a NaN or an infinity
reaches the node although a file cannot write one. `CaseScope::Host` also marks a case over a
type only the host holds (`u64`, `f64`). `gpu_cases.rs` runs every case of scope `Both` on a
device through the compiler's own shaders, with `support.rs`'s headless device and a runner that
binds a shader as the engine does. A pass case runs the shader of its pass `case`. A node case
becomes a pass: each input whose socket takes an element rate comes from a lane `in_<k>` read by
`n<200 + k>`, so the node's WGSL form computes it, an input of a uniform socket stays a literal
that the host evaluates into the uniform block, and each output goes to a lane `out_<k>` (a view
kind's through `view.color` on the palette `ramp`). The element is the last cell of a one-row
grid, the tick the tick buffer's word, and a region iteration the compiler's `outer_iteration`. A
case in the init stage runs on the host alone, since init never runs on the device. 313 cases run
on the device, the test asks for more than 300.

**Draws** (`draw.rs`). `key_word(key, phase, seed)`, `draw_bits(k, element, tick, iteration,
sub_draw)` and `press_word(seed, press)`, with `GRAPH_DEFAULT_SEED` and `GRAPH_SEED_SALT`. The
core is `draw_core`, `pcg4d2`: `pcg4d(pcg4d([k, e, t, (i << 4) | (s >> 2)]))[s & 3]`, with `i`
0 outside every region and `pcg4d` the JCGT function, which henad-core's random primitives hold
(`rng::pcg4d` and `henad::rng::pcg4d`). `draw.rs` re-exports it and `draw.wgsl` imports it. It is
the one core that passed every walk of the PractRand gate, which failed both candidates, and is
the settled core of library 1. A grid rule's `rng` starts from the same two rounds.
`the_draw_core_keeps_its_words` pins its words, since the `rand.*` case words and their lock
lines are drawn from them. Phase
words: `PHASE_INIT` 0, `PHASE_TICK` 1 (view and stats too, which draw nothing), and
`action_phase(j)` = 2 + j for the action at position `j` of `[[action]]`, its reruns included.
The tick word `t` is the tick in a tick pass, the press count in an action, 0 in init.
`iteration_word(outer, max, k)` is `k` at the top and `outer * max + k` nested, so a draw inside
a Repeat reads `Some(k)`, and one outside every region reads `None`. The WGSL twin is `draw.wgsl`
(`DRAW_WGSL`, import path `henad_graph::draw`). A shader reads the key word from its uniform
block, `e` is `henad_index`, `t` the tick buffer's word in a tick pass and the press count in an
action, and `i` the loop's `henad_iteration_n<id>`.

**The host evaluator** (`host/`). `HostEvaluator::new(&GraphFile)` checks the file (errors give
`HostError::Checked`) and compiles every pass once (`compile.rs`) into a `Program`
(`program.rs`), which the WGSL compiler reads too. A pass the host cannot run compiles to an error that `run_pass` returns, and only an init pass or a rerun pass with one
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
block, and post block (reads a reducer, run once after the elements). After the split,
`hoist_uniform_body_values` moves each instruction of a Repeat body (nested ones included) that
`Operation::is_pure` admits and that reads only constants, parameters and prelude slots to the end
of the prelude, in body order. Draws, stream bits, lane access and effects never move. The WGSL
compiler then reads a lane seed, a press word or a powi base computed in a body from the uniform
block, as it reads every other prelude value. An element value that
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
and no abort. Host init is bounded by work: `Demand::shortfalls` refuses a host run whose
`cost * elements`, over the init and rerun passes, is past `HOST_WORK_BUDGET`.

**Edit commands** (`edit/`) and the editor region (`editor.rs`) are in
`agents/henad-graph-edit.md`.

**The checker** (`check/`). `GraphFile::check()` returns a `CheckReport`: every diagnostic in file
order, `duplicate_ids` (the repair state), a `PassReport` per pass (cost, and the inferred
`PortType` of each output and inner output), one per function body checked (`functions`, the cost
of one call) and `dual_lanes`. `view.rs` builds a `BodyView` per
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
arithmetic after a reducer is allowed. `stat.counter` is ready after the elements too
(`NodeDef::is_ready_after_elements`): the host learns a counter from the readback, so the total and
values computed from it alone run in the post block, and an element read of one is that error,
naming a counter total (`a_counter_total_is_read_after_the_elements_alone`). It also reports a
stream draw whose sources reach a draw that `draw_order` lists after it (V17). `lane.set` is allowed
in init, tick and action passes only, never in a view or stats pass or a region body. Diagnostics of
a node carry its pass and node id. Function bodies, normalised passes and the agent rules are below.

**The library lock.** `library.lock` at the crate root, one line per definition (hashes of the
signature's `Debug`, the semantics and the cases' `Debug`, parity, cost) and one per limit.
`the_library_lock_matches_the_catalogue` fails on a kind without cases and on any line that
differs. `HENAD_WRITE_LIBRARY_LOCK=1` writes it. Renaming a catalogue field moves every signature
hash. The test prints the corpus files that use each kind (`--nocapture`), which is the "Used by"
information, and a coverage line, the kinds the corpus uses out of every kind.

**Code identity.** `GraphFile::code` is `ModelCode::new(CODE_FORMAT, semantic_text)`, and
`GraphFile::source` is `ModelSource::runtime(target id, code)`, which records
`CodeOrigin::Runtime`. `from_code` refuses another format with the message
`Recorded under X, and this build runs henad-graph/1`.

**The time scale**. A `[[param]]` table takes two optional keys, `hidden = true` and
`tick_unit = "day"` (read through `TimeUnit::parse`). `ParamTable::into_graph_param` reads both,
and `into_descriptor`, the grid rule's reader, refuses either at its table. The descriptor is
`hidden()`, and `GraphDocument.time` holds the one `TimeScale` (unit and parameter id) a table's
`tick_unit` names. The reader refuses a second `tick_unit` at its table, since the document holds
one. `write_param` writes `hidden = true` only when set, and the writer adds `tick_unit` after the
tick length's keys, so a file setting neither keeps its code.
`graph_codes_are_unchanged_by_the_new_keys` in henad-models' `lowered_fixtures.rs` pins the
starters', the corpus files' and the `tests/fixtures/lowered/round1/` files' hashes at `be338ec`.
Keep it. `check_tick_length` refuses a tick length that is not an `f32`, not hidden, or has a `min`
of 0 or below, and a time scale naming no parameter. `RenameParam` renames the time scale's
parameter, `renamed_param` keeps `hidden`, and `RemoveParam` drops the time scale with its tick
length. henad-app's World pane rebuilds a descriptor for `SetParam` in `world.rs`'s `param_with`,
which keeps `hidden` too. A new descriptor flag goes into both copiers.
`GridProgram` and `AgentProgram` copy `document.time` to `EntryDeclarations.time`, so a
manifest's replayed text and an expert copy keep their time axis. `src/tests/time_scale.rs` holds
the round trip, the refusals and the entry. The three rates fixtures in henad-models set both
keys on `tick_length`.

**The WGSL compiler** of grid and agents graphs (`wgsl/`) is in `agents/henad-graph-wgsl.md`.

**Outside the crate.** henad-explore's `ModelFile::Graph` holds a `GraphFile`, and its `entry`
returns the checker's errors, then `HostEvaluator::new`'s, then `GridProgram::compile`'s or
`AgentProgram::compile`'s, or the entry. henad-cli's
`--model-file` prints a graph's warnings and errors with places through it. The facade's
`henad::graph` (feature `model-files`) names the crate's root items and public modules.
henad-models' `graph_corpus.rs` holds the compiled entry's schema to the stock model's and runs
the testing kit over every grid corpus entry, `graph_grid_parity.rs` holds Game of Life and Vote to
the stock models every tick, `graph_sir_parity.rs` the SIR files, and `graph_repeat.rs`
`repeat_trials.toml` (`agents/models.md`).

Tests sit in `src/tests/`: `file_format.rs` with `full_graph.toml` as a file in a hand-written
spelling, `catalogue.rs`, `known_answers.rs`, `checker.rs`, `host.rs` (corpus inits replay, reruns,
a Repeat equals its hand-chained copies bit for bit, mixed-radix draws), `edit.rs`,
`library_lock.rs`, `mutation.rs`, `regions.rs` (the region edits), `wgsl.rs` (every starter
compiles, no shader but the display returns early, the range table, region forms, uniform body
values, and files past the workgroup, uniform and shader-size limits built in the test),
`gpu_cases.rs`, `gpu_repeat.rs` and `agent_wgsl.rs` (agents' budgets, uniform and workgroup limits,
the replay flag, false among the starters and sheet starters for boids and bounded confidence
alone, a float sum on a grid writing `partials`, the forms of counters and deposits).
`sheet_support.rs` holds the sheets the tests read and the helpers that edit, read, write and lower
their text (`edited` checks that the text holds the old text, `edited_once` that it holds it once,
`only_refusal` that lowering refuses with exactly one error), the sheet edit commands and the host
runs. `support.rs` holds the rest: the graph tests' `node_id`, `link`, `starter_builder`,
`assert_checks`, `graph_errors` and `graph_refusal`, the texts past the device limits that `wgsl.rs`
and `agent_wgsl.rs` share, and `mutations`, through which every mutation corpus draws its edits over
its own table of hostile values and lines. A device test takes its device from `device_for`, which
prints the skip, and builds through `context`, `params_of` and `built` there.
`HENAD_DUMP_GRAPH_WGSL=<dir>` makes `every_starter_compiles` and `every_agents_graph_compiles` write
each shader's text there.
`gpu_cases.rs` runs every `Both` case on the device, an agents pass case through
`run_agent_pass_case` (the buffers of `compile_parts`, an index built on the CPU, `counters`,
`partials` and `agent_colors` read back), and a grid stats case that sums floats on the grid, with a
`partials` buffer of one float per lane per 16 by 16 block summed on the host, and again as an
agents world of one agent per cell. It also fails on a kind with a WGSL form outside init passes
whose own cases never ran on the device (`vec.from_angle` had a host case alone until the round 3
audit). `CaseValue::NearVec2F` compares a `vec2f` per component within a tolerance. `support.rs`'s device raises `max_storage_buffers_per_shader_stage` to 16,
since a radius fold case binds 9. `gpu_repeat.rs` runs `host.rs`'s Repeat
bodies (`CARRIED`, `chained()`, `TWO_DRAWS`, `NESTED_DRAW`, `pub(super)` there) in a tick pass,
once through the step shader on the device and once through `run_pass`: the integer half of
`CARRIED` equals its hand-chained copies and the host bit for bit, two iterations of one draw
differ and replay, and draws read the mixed-radix iteration word, each equal to the host's. Its
float half replays on one device but may differ from its copies and the host on a device that
fuses a multiply with an add (`agents/gpu-traps.md`), so
`a_float_repeat_body_equals_its_hand_chained_copies_on_the_device` is `#[ignore]`d as a probe of a
device and no gate.
`a_repeat_runs_in_stats_view_and_action_passes_on_the_device` holds a Repeat with a per-cell count
in the other map passes to the host: the statistics finished from the device's tallies, every
cell's colour and the action's lane.
`corpus.rs` reads the starters through `starters()` and, at run time, the graphs in
`crates/henad-models/tests/fixtures/graphs/`, since a packaged crate has no path out of itself.
`two_spellings_of_one_graph_hash_alike`, `a_canonical_text_reads_back_to_itself` and
`moving_a_node_changes_the_text_and_leaves_the_hash` are the gate of the file format, and
`the_reader_and_checker_never_panic_on_the_mutation_corpus` and
`a_hostile_region_tree_or_call_graph_is_reported_without_a_panic` the gate of the checker. Keep
them.

**Agent models, milestone A2 on the host.** The kinds of A2's row sit in `catalogue/space.rs`
(world, vector and space kinds), `fields.rs` (`lane.get_at`, fields, counters, `stat.sum`,
`stat.mean`, `stat.counter`), `folds.rs` (`grid.fold`, `agent.radius_fold`, `stencil_offsets`),
`functions.rs` (`fn.*`) and the `A2_KINDS` tables of `math.rs`, `conv.rs` and `random.rs`.
`vec.hypot` is left out: a CPU oracle's quirk with no WGSL form. The *domain grid* is the grid, or
an agents world's field grid, `max(side, 1)` truncated per axis, which wraps as the world does. A
cell reference is `y * width + x`, and `u32::MAX` is no cell: `field.get` there reads 0, a deposit
there is dropped. New socket types: `Field`, `FieldLayer`, `LaneElement`, `Vector`, `Scalar`,
`Wide`, `InterfacePort`. `InputDefault::FirstLayer` is written as the field's first layer literal,
and `OwnCell` is never written and required in a pass over agents. `PassCase` gained `agents` (an
agents header over a world of `width` by `height`), `deposits` and the counters before and after,
and a lane id of a case may be a field layer `<field>.<layer>`.

**Fields that add up** (row D). `FieldCombine` is `Copy` and `Eq`, so a sum holds its resolution as
an exponent: `FieldCombine::Sum { resolution_shift }`, the resolution 2^-k for k from 0 to
`MAX_RESOLUTION_SHIFT` (24), `resolution()` its `f32`, `deposit_scale()` 2^k and `wrap_bound()` 2^32
times the resolution. All three take the exponent from one private helper capped at 24, so a
hand-built shift past 24 still counts and merges at one resolution. The file writes `combine =
"sum"` and `resolution` as a number in the writer's form (`1.52587890625e-5` for 2^-16), and the
reader takes `resolution` only with `sum`, needs it there, and refuses any other value at the key
with the nearest allowed one (`FieldCombine::sum`). Every `GraphFile` comes through the reader, so
no checked document holds a shift past 24. `check/agents.rs` refuses a deposit into a sum whose
`value` is a literal at or past the wrap bound, or a parameter whose maximum is, and names the
bound. Other deposits can still wrap at run time, as a counter does. The host's deposit column of a
sum is `u32`, the total as a count of the resolution: `host::run::sum_word` rounds `max(value, 0)`
times `deposit_scale()` half to even and saturates, a NaN adding 0, and the column adds it wrapping.
`HostLanes::deposits` documents both column types. The catalogue case `a sum in whole resolutions,
rounded half to even` holds ties at 0.25. Tests: `tests/sum_fields.rs` (round trip, refusals, wrap
bound, one exponent for every shift), `gpu_cases.rs`'s
`many_deposits_into_one_cell_add_up_exactly_on_device_and_host` (a million deposits into one cell at
2^-10, NaNs, negatives and ties included, the word equal on both), and henad-models'
`graph_sum_fields.rs` (a run replays bit for bit, and the field after one tick is the host's word
times the resolution through the `finish`).

**Normalisation** (`normal.rs`, `expr.rs`). `normalise_body` replaces each `math.expr` by its
expansion and each `fn.call` by a copy of its function's body, through a queue, so nested calls and
expressions in functions settle without recursion, and stops past `MAX_NODES_INLINED`. Copies take
ids from `first_free_id`, sit in the call's region, and keep an `Origin` (the calls, outermost
first, the function and the node as written). A draw's key becomes `compose_key(call, key)` per
enclosing call, innermost first. `replaced` holds each replaced node's output values with links
resolved. The expression language is `math.expr`'s semantics: assignments to `config.outputs`, Rust
precedence, literals typed by their neighbour, `vec * f32` as `vec.scale`, `vec / f32` as `math.div`
by `vec.make(s, s)`. The parser counts one level of `MAX_DEPTH` (64) per parenthesis, unary
operator, operand of a chain (`a + b + c`) and component read, each a level of the tree that
`build` and the drop recurse through, so a long flat chain is refused and never overflows a stack
(`a_long_flat_expression_is_refused_within_a_small_stack`, on a 256 KiB thread).
`HostEvaluator::from_document` normalises first and reports a fault or a refusal at the node as
written (`located`), and `document()` is the normalised document.

**The checker over functions and agents.** `check()` checks each function body alone
(`functions::check_function`: types with `fn.input` at the constant rate, no sink, one `fn.output`
per output), then `functions::inlinable` finds the functions whose bodies and callees hold no error.
`cost::check` runs before any normalisation and counts each `math.expr` as the nodes it expands
into. A file past `MAX_NODES_INLINED` (65536) after inlining is reported there and checked as
written alone, so a small file cannot make the checker build a huge expansion. A pass whose calls
all reach those and whose expressions expand is checked normalised
(`BodyView::normalised`, places from `normal_places`): a problem in a copy is reported at the
outermost call with a note `in node nK of the function 'f'`, and the call's ports are typed from the
values its outputs take (`replaced_ports`). Any other pass is checked as written, a call standing
for its function's ports (`ConfigPorts::FunctionInterface`). Context sets, rates and nesting
therefore reach a call site through its copies. `agents.rs`: field writes (static fields in init
alone, one write per layer, no deposit into a static field), no `field.get` of a field in its own
`finish` or a function that one calls (`check_finishes`, since the update writes the field in
place), `lane.get_at` reading only `cell` or
`other` of a fold around it, nesting (Repeat holds Repeat or `grid.fold`, `grid.fold` holds Repeat,
a radius fold holds and sits in no region), V12 (no radius fold after the tick pass that writes
positions), V14 (one `cell_size` source, compared structurally), and V18 after inlining (a composed
key that meets another draw's key). A radius fold reading the positions its tick pass writes makes
the position lane dual. The written keys stay checked by `passes::check_keys`, and cost through
calls by `cost.rs`, with `math.expr` costing its expansion.

**The host over agents.** `HostWorld::Agents` holds the count and the world's width and height.
`HostLanes` holds the lanes, then each field's layers (`layers()`, `column("<field>.<layer>")`), the
deposits of each dynamic layer (`deposits(id)`, merged by the engine's field update, not here) and
the counters (`counter`, `set_counter`, wrapping `u32`). The radius fold builds a `SpatialHash` of
the start-of-pass positions per cell size and walks it in `for_each_within`'s order (`run::walk`),
counting candidates and faulting past `MAX_FOLD_CANDIDATES` with the node, and faulting before it
walks when its bounds cover more than `MAX_WALK_CELLS` (65536) cells, a negative radius none. A draw
in a radius fold's body reads the other agent's index as its iteration word, and one in a
`grid.fold` its table position, mixed radix under a Repeat. `stat.sum` and `stat.mean` fold chunks
of 8192 from `+0.0`. `stream.float` draws `next_float` on the stage's stream.

**The agent corpus.** `boids.toml` writes its steering as one `math.expr`, and `ants.toml` its
neighbour pick (`best_neighbour`, holding the `grid.fold`), its two moves (`move_if_passable`) and
its delivery (`math.expr`) apart, so `walk` holds 37 nodes, down from the corpus file's 62. A host
run of every pass equals the corpus files' form on the same lanes. The ants' lane `reward` is
`reward_held`, since the parameter `reward` takes that id. `gpu_boids.toml` and `gpu_ants.toml` sit
in henad-models' fixtures. henad-models' `graph_agent_corpus.rs` holds every agent file's init to
the stock `GpuBoids` and `GpuAnts` seed buffers (the random walk to its init written out) at three
worlds, both parameter sets and four seeds, and the ants' reset rerun to the init. Tests of the
rules sit in `src/tests/agent_rules.rs`. The oracles of the agents corpus on a device, the agent
Repeat tests and the boids fold's faulting corner are henad-models' (`agents/models.md`).
`stat.sum`'s `+0.0` start is pinned by two cases, an `f32` and an `f64` chunk over two `-0.0`.
