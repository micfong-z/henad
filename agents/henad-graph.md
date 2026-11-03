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
`FORMAT_VERSION`, `KIND_GRAPH`, `LIBRARY` and `CODE_FORMAT`.

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
`{:?}` form. A node's config keys and inputs are written in lexical order, since no catalogue
exists yet. A `ConfigValue::Table` follows `INLINE_KEY_ORDER`, then lexical order. Config values
are written as read, so a `u64` string in `config.salt` has as many spellings as the file uses.

**The editor region** keeps a `toml::Table`, never refuses a value and never fails a read. Its
writer orders the keys and rounds `pos` to whole points, and `to_text` is a fixed point for any
table, which `a_strange_editor_region_stays_readable_and_a_fixed_point` holds.

**Code identity.** `GraphFile::code` is `ModelCode::new(CODE_FORMAT, semantic_text)`, and
`GraphFile::source` is `ModelSource::runtime(target id, code)`, which records
`CodeOrigin::Runtime`. `from_code` refuses another format with the message
`Recorded under X, and this build runs henad-graph/1`.

Tests sit in `src/tests/file_format.rs`, with `src/tests/full_graph.toml` as a file in a hand-written
spelling. `two_spellings_of_one_graph_hash_alike`, `a_canonical_text_reads_back_to_itself` and
`moving_a_node_changes_the_text_and_leaves_the_hash` are the gate of the file format. Keep them.
