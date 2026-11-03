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
the counter-based draw words, and `check/` the checker. `starters/` at the crate root holds the
grid starters, ported to one GPU target with `node_<model>` ids.

**The reader** checks the header first, as `model_file.rs` does: `format`, `format_version`,
`kind` and `[graph] library`, ignoring other keys. It checks `MAX_FILE_BYTES` before any of that.
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
palette `ramp` of four colours). `known_answers.rs` holds every node case to a test-only
reference evaluation. Pass cases wait for the host evaluator, which must run every case.

**Draws** (`draw.rs`). `key_word(key, phase, seed)`, `draw_bits(k, element, tick, iteration,
sub_draw)` and `press_word(seed, press)`, with `GRAPH_DEFAULT_SEED` and `GRAPH_SEED_SALT`. The
core is the provisional `chainm` of the PractRand gate, which failed both candidates: the
maintainer chooses the core, and the `rand.*` cases and their lock lines move with it.

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
rules across a pass and across passes, and `declarations.rs` the tables. Diagnostics of a node
carry its pass and node id. `fn.call`, `fn.input` and `fn.output` are A2 kinds: today they report
as unknown, while cost, call depth, call cycles and inlined size already follow `fn.call`.

**The library lock.** `library.lock` at the crate root, one line per definition (hashes of the
signature's `Debug`, the semantics and the cases' `Debug`, parity, cost) and one per limit.
`the_library_lock_matches_the_catalogue` fails on a kind without cases and on any line that
differs. `HENAD_WRITE_LIBRARY_LOCK=1` writes it. Renaming a catalogue field moves every signature
hash. The test prints the corpus files that use each kind (`--nocapture`), which is the "Used by"
information.

**The editor region** keeps a `toml::Table`, never refuses a value and never fails a read. Its
writer orders the keys and rounds `pos` to whole points, and `to_text` is a fixed point for any
table, which `a_strange_editor_region_stays_readable_and_a_fixed_point` holds.

**Code identity.** `GraphFile::code` is `ModelCode::new(CODE_FORMAT, semantic_text)`, and
`GraphFile::source` is `ModelSource::runtime(target id, code)`, which records
`CodeOrigin::Runtime`. `from_code` refuses another format with the message
`Recorded under X, and this build runs henad-graph/1`.

Tests sit in `src/tests/`: `file_format.rs` with `full_graph.toml` as a file in a hand-written
spelling, `catalogue.rs`, `known_answers.rs`, `checker.rs`, `library_lock.rs` and `mutation.rs`.
`corpus.rs` reads the starters and, at run time, the graphs in
`crates/henad-models/tests/fixtures/graphs/`, since a packaged crate has no path out of itself.
`two_spellings_of_one_graph_hash_alike`, `a_canonical_text_reads_back_to_itself` and
`moving_a_node_changes_the_text_and_leaves_the_hash` are the gate of the file format, and
`the_reader_and_checker_never_panic_on_the_mutation_corpus` and
`a_hostile_region_tree_or_call_graph_is_reported_without_a_panic` the gate of the checker. Keep
them.
