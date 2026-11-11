# henad-graph: the WGSL compiler

`agents/henad-graph.md` holds the rest of the crate: the IR, reader, writer, catalogue, checker and
host evaluator.

**The WGSL compiler** (`wgsl/`). `GridProgram::compile(&GraphFile)` checks the file, builds a
`HostEvaluator`, and writes one `GeneratedShader` per role (`ShaderRole`): one step per enabled
tick pass in stack order, a display shader over the enabled view passes, a reduce shader over the
enabled stats passes, and one per enabled action pass. A pass with `enabled = false` is left out,
and one bound to a parameter reads a uniform flag. Each body is the pass's host `Program` written
as WGSL (`emit.rs`), so the backends share one schedule. Every value is a `let` named
`n<id>_<port>` (`value_name`, a dotted port's `.` written `__`). `is_port_segment` refuses `__`,
which keeps the names injective. Lane writes are staged in `henad_write_<lane>` and stored at the
end of the pass. Prelude values that read no tick are evaluated on the host (`run::prelude`) into
the uniform block `HenadParams` (`uniform.rs`: `henad_width`, `henad_height`, prelude slots,
`henad_key_n<id>` key words, `henad_press`, `math.powi`'s `henad_powers_n<id>` packed four to a
`vec4`, enabled flags, tables last, padded to 16 bytes). Prelude values that read the tick are
computed in the shader. A reduce shader's post block runs on the host after the readback
(`run::finish`), which is how a `[[stat]]` gets its value (`GridHooks::stats`). The buffers are
the lanes, labelled `lane_<id>_cells` (one word per element, since a lane's `storage` is for a CPU
target), then a one-word dual `henad_tick` buffer when a GPU pass reads the tick or a tick pass
draws. A lane of `CheckReport::dual_lanes` whose writer tick pass runs is dual: its writer binds
`lane_<id>_cells_in` (current side) and `_out` (next side), passes before it read the current side
and passes after it the next. The last step writes the tick from invocation (0, 0), and a graph
that reads the tick with no tick pass gets an `advance_tick` step. A step's bounds check wraps its
body in a block, and no step returns early (`agents/gpu-traps.md`). Every other binding is one
`lane_<id>_cells`, read-only or read-write. A Repeat is a counted `for` loop with its accumulators
as `var`s, a grid count is unrolled over its stencil through `henad_at` (`offset_cell` of
`henad::space`), and a draw calls `draw_core`. Each shader goes through `compose_wgsl` with
`DRAW_WGSL`, and a problem there is a compiler fault, placed on the node whose `NodeRange` holds
its span. A text past `compose::MAX_FILE_BYTES` (1 MiB) is checked first and refused as a
problem of the file, placed on the pass whose ranges hold the most bytes (`oversized`), since a
checked file of many grid counts can expand that far.
`GeneratedShader` exposes `text` (before composition), `composed`, `ranges`,
`node_at(offset)`, `role`, `passes`, `label` and `cost`, which the Editor's WGSL view reads.
`GridProgram::spec` builds the `GpuGridSpec` (workgroup 16, `counters` the reduce tallies,
`init_cost` and each action's `rerun_cost` from the checker's costs, each pass's
`uniform_bytes` its block's size, and the reduce pass's `workgroup_bytes` four a tally, which
`Demand::shortfalls` holds to the device), and `GridProgram::entry`
the `ModelEntry` (`grid_width` and `grid_height` 1 to 16384 at the `[grid]` defaults, then the
file's parameters, each live or reload-only as `wgsl/live.rs` marks it, its stats, actions and
palette). `hooks.rs` holds `GraphHooks`.
`seed_buffers` runs `HostEvaluator::init`, and a fault panics, which the build's scope reports.
`rerun` evaluates each action pass's block first, so a fault refuses the press, then reruns the
init passes of `runs` on fresh lanes. A tick pass in an action's `runs` is refused, since a press
runs its reruns on the host. A problem the compiler finds names its pass and node and is placed on
the node's table, or the pass's (`place`). Graphs of agents are A2's.

**The WGSL compiler over agents** (`wgsl/agents.rs`, `wgsl/agent_hooks.rs`). `AgentProgram::compile`
checks the file, then `compile_parts` adds a lane `henad_merged_<field>` and a tick pass
`henad_update_<field>` (lane.get, `fn.call` of the field's `finish`, lane.set) for each field that
takes deposits and has a `finish`, after the last tick pass where the reader sorts them, builds the
`HostEvaluator` of that document with `from_document`, and types it with a check of the normalised
document through its canonical text (`GraphFile::from_normalised`, which skips the two size limits
of `parse`, since the inlined node limit bounds that text and it can pass 1 MiB for a far smaller
file), since the written report has no types for copies. Positions of passes and lanes are those of
that document everywhere after. Every shader but the display is a linear pass
(`@workgroup_size(256)`, `linear_index` over `henad_groups_x`), each pass body inside `henad_index <
params.henad_agents` or `henad_cells`. Steps: each enabled tick pass, then one `update_<field>` per
field that takes deposits over `Cells(layers)` (`max(field, bitcast(accum))` or a sum's add, `accum`
zeroed, then the `finish` body through the merged lane), then `advance_tick` when a GPU pass reads
the tick. The tick lives in word `TICK_WORD` (1) of the engine's `counters`, read with `atomicLoad`,
and `advance_tick` adds 1 from invocation 0 after the step's last pass. Word `FAULT_WORD` (0) is the
fault, the document's counters start at `FIRST_COUNTER` (2), and the reduce leaf's tallies follow,
`cleared_counters` in the spec. Buffers: each `[[lane]]` at its own position as `lane_<id>_<domain>`
(a `vec2` one interleaved `vec2<T>`, the position lane drawable, a lane of `CheckReport::dual_lanes`
whose writer runs dual), `field_<id>_cells` (layers one after another), `accum_<id>_cells` per field
taking deposits, `agent_colors` (drawable). `Role::Color` runs the view passes over agents and
writes packed RGBA from `henad_packed_*` constants (default: the first category lane over agents,
else white), `Role::Display` the view passes over cells (`cell_at` over `henad_texture`),
`Role::Reduce` one leaf over the longest stats domain: tallies in workgroup atomics then `counters`,
each float sum a `block_sum` lane into `partials` (a `vec2` two lanes, `sum_lanes` maps them back),
and an action one shader per action (a second action pass is refused). Forms: `lane.get_at`
bounds-checks with `arrayLength`, `field.get` reads 0 off the grid, `field.set` stores at the end of
the pass, `field.deposit` and `counter.add` are below, `grid.fold` a loop over a constant offset
table through `henad_step`,
`agent.radius_fold` the `henad::hash` walk over `cell_start` and `sorted`, its prefilter `d.x * d.x
+ d.y * d.y <= r * r` as the host's, its draws keyed on the other agent, and
`atomicMax(&counters[0], code)` past `MAX_VISITS` (code `2p + 1` for the fold at position `p` of the
plan's faults), or `2p + 2` before the walk when `walk_cells` of its bounds passes `MAX_WALK_CELLS`.
`AgentGraphHooks::fault` turns the code into the pass and node as written (through
`HostEvaluator::origin`, the outermost call for a copy) and the line of that node's table.
A grid's reduce shader sums floats as the agents leaf does: `var henad_sum_<k>` before the cell's
bounds check, `block_sum(henad_local, ...)` after the counters, and the total of workgroup
`wid.y * ceil(width / 16) + wid.x` at `partials[block * lanes + k]`. `GeneratedShader`'s
`sum_lanes` maps the lanes back, `GridProgram::spec` sets `GpuGridSpec::sums`, and
`GraphHooks::stats_with_sums` feeds each total to `run::finish`.
The reduce pass's workgroup bytes add 1024 for `block_sum`'s scratch.
`replays_exactly` is false when a tick pass's radius fold holds a `carry`, a float `sum`, or reads
its `k` or any `prev.<acc>`. The entry prepends `num_agents` (1 to `max_count`), `world_width` and
`world_height` (1 to 10000, step 50) as `agent_model_param_descriptors` does, and its spec carries
each pass's cost, uniform size and workgroup bytes, `init_cost` and each action's `rerun_cost`.
`AgentGraphHooks::step_cost` charges each radius fold of a step at the parameters: the cells its
walk covers (from the radius the prelude gives and the index of `geom`), plus the agents a uniform
spread puts there, at most every agent and the visit cap, times the body's instruction count plus
`VISIT_COST` (8). A walk past the cell cap costs 1, since it faults at once. `reports_faults` holds
when a fold can fault. `run_params_bytes` returns a prelude's fault, and `pass_params_bytes` alone
still zeroes the block. The field update runs one invocation per slot `layer * cells + cell`
(`henad_slot`): its body's `henad_index`, own cell and default `at` are the cell, a draw's element
word the slot, and the update writes the field at the slot. Its slot check wraps the update in a
block, as a grid step's bounds check does. A rerun that writes
some layers of a field is refused at compile, since a field uploads whole.

**Live and reload-only parameters** (`wgsl/live.rs`). `GridProgram::entry` and
`AgentProgram::entry` mark each of the graph's own parameters through `own_params(params,
&live::reload_only(&evaluator))`: reload-only (`on_reload`) where the host reads it at build, live
otherwise. The sizes in front (`grid_width`, `grid_height`, `num_agents`, `world_width`,
`world_height`) stay reload-only. `reload_only` reads the evaluator's document: the tick length of
its `time`, a parameter an init pass reads (`Program::params`) or runs by (`PassEnabled::Param`),
and one the reach or the cell size of a radius fold reads (`reach_params`, the slots its `radius`
and `cell_size` are computed from through the prelude and the element, regions flattened, found by
`emit::defined` and `operands` until nothing new is found). The neighbour index and the work
budget follow a fold's reach, both fixed at build. Every other parameter reaches the shaders only
through the uniform blocks, where the host evaluates the prelude (`math.powi`'s table included,
since `MemberSource::Table` recomputes the powers from its base at each write) or the reruns and the
stats, which read the engine's values. A live one is set on the running engine without a rebuild
(`agents/henad-compute.md`). The flags change no shader and stay out of the schema hash. Tests:
`each_sheet_starter_marks_what_its_build_reads_reload_only` and `the_sir_starters_rates_are_live`
(`tests/live_params.rs`), henad-models'
`a_live_rate_edit_keeps_the_tick_and_the_cells_and_later_ticks_follow_it_on_the_device`
(`sheet_grid.rs`: 64 by 64, 10 ticks at 0.3, an edit to 2.0, then 30 ticks equal to the host at
the new rate and apart from a run that kept 0.3), and the testing kit's `ApplyModes`, which every
sheet entry passes.

**Fault names** (`wgsl/fault_names.rs`). `FaultNames` is a table from a node id, as the file
writes it, to the words that name it in the document the graph was lowered from, such as "line 3,
Flockmates" for a radius fold a sheet lowered. It writes its own `Debug` (the number of names).
`AgentProgram::compile_with_fault_names(file, &names)` compiles as `compile` does, through the
private `compile_named(file, probes, Option<&FaultNames>)` that `compile_with_probes` shares, and
the fault messages of a named fold become `sheet::sentence::look_met_too_many` ("<words>: looked at
more than 65536 agents around one agent, and the run stops. A larger world or a shorter distance
looks at fewer") and `look_searches_too_far` ("<words>: would search more than 65536 cells around
one agent, and the run stops. A shorter distance searches fewer"), naming no pass, node or line. A
fold the table does not hold keeps the graph's message, and a graph file passes no table. The names
change no shader, so the text and the code identity stay. henad-explore's `ModelFile` passes a
sheet's `SheetFile::fault_names`.

**Counters and deposits.** A step that adds to a counter declares `var<workgroup>
henad_counter_tally`, one `atomic<u32>` slot per counter its passes add to, numbered in the order
the passes first add to each (`Emitter::counter_slots`). `counter.add` skips a zero amount and
adds any other into its counter's slot. `main` zeroes the slots before a `workgroupBarrier()` at
its top. After a second barrier at its end, the invocation of each slot adds a non-zero tally to
the counter's word of `counters` with one `atomicAdd`, the shape of the reduce leaf. A counter
therefore takes one global atomic per workgroup. Both barriers sit at the top level of `main`,
outside the guard of every pass. A tally wraps at 2^32 as the counter does, and the sum of the
tallies equals the host's total. The step's `workgroup_bytes` are four a slot, which
`Demand::shortfalls` holds to the device. The checker keeps `counter.add` at the top of a tick
pass over agents (its contexts, and no function holds a sink), and the compiler refuses one
anywhere else. `field.deposit` issues its `atomicMax` only inside the test that the value is above
0, so a zero, a negative or a NaN issues no atomic. An `accum` word starts at 0 and never falls,
and an `atomicMax` of 0 on it changes nothing. A field whose `combine` is a sum
(`FieldPlan::combine`, and `Operation::Deposit::combine` at the node) adds `u32(round(value * 2^k))`
with `atomicAdd` inside the same test, the scale 2^k a literal. The product by a power of two is
exact, and WGSL's `round` rounds half to even as `f32::round_ties_even` does on the host. Its field
update reads the word as `henad_word` and merges `field + f32(henad_word) * 2^-k`, both literals
from `literal`, where the max reads the word's bits as `henad_deposit` and merges `max(field,
henad_deposit)`. The scale and the resolution both come from `FieldCombine::deposit_scale` and
`resolution()`, one capped exponent. The word starts at 0 for both, so the zeroing stays. Tests:
`each_counter_flushes_once_per_workgroup_after_a_top_level_barrier` and
`a_deposit_issues_no_atomic_unless_its_value_is_above_zero` (`agent_wgsl.rs`) hold the text,
`no_step_returns_early` (`wgsl.rs`) every starter's and fixture's shaders but the display, and
`a_counter_over_many_workgroups_equals_the_host_total` (`gpu_cases.rs`) a million agents, half
adding 0, to the host's total on the device.

**Probes** (`wgsl/probes.rs`). `GridProgram::compile_with_probes(file, &Probes)` and
`AgentProgram::compile_with_probes` take a `Probes` list beside the file (and
`AgentProgram::compile_with_probes_and_fault_names` both, for a sheet), each a `PortRef` to a
`bool` output of a node in a tick pass, and `compile` passes an empty one. Probes are no part of the
document, so the canonical text and the code identity ignore them, and a graph with none compiles to
the shaders of `compile` (`probes_leave_the_code_identity_and_text_alone`). `probes::resolve` finds
each in the evaluator's programs and refuses one that names no output of a tick pass (an output of a
`math.expr` or `fn.call` included, which normalisation expands into copies), a node with a `region`,
which runs per iteration, and an output of another type than `bool`. A probe of a pass that `enabled
= false` turns off keeps its word and builds no tally. In the step that runs its pass, after the
element's instructions, `if (params.henad_count_fires != 0u && <output>)` adds 1 to the probe's slot
of `var<workgroup> henad_fire_tally`. `main` zeroes the slots before a barrier, and after a barrier
at its end the invocation `slot % 256` adds each non-zero slot to word `fire_base + i` of
`counters`, both inside `if (params.henad_count_fires != 0u)` at the top level of `main`. The flag
is a uniform, so the branch is uniform and a step outside the counted step pays one test.
`henad_count_fires` (`MemberSource::CountFires`, `Dispatch::counting`) comes from
`GridPassCtx::counting` or `AgentRun::counting`. The counts follow the reduce tallies: on a grid at
the reduce shader's counters (at least 1), the spec's `counters` growing by one word per probe, and
over agents after the fault word, the tick, the document's counters and the leaf's tallies, in
`counter_words`. A grid step with probes binds `counters` as a runtime-sized array and takes
`local_invocation_index`, so a starter's step gains one storage binding. Each slot adds four bytes
to the step's `workgroup_bytes`, which `GeneratedShader::pass_spec` now copies for every grid
shader. `HostEvaluator::run_pass_counting` counts the same probes on the host, adding to the probe's
count for each element where its output holds. Tests in `tests/probes.rs`:
`a_probe_on_each_transition_row_counts_as_the_host_does` (the Game of Life starter with its step
written as two `cat.transition` rows, every tick for 50 ticks and after 64 steps),
`a_probe_over_agents_counts_as_the_host_does` and the refusals. henad-models' `graph_probes.rs`
holds every starter probed on its `bool` outputs within 8 storage bindings, the SIR starter
stepping as it does with no probe, and
`every_probed_shader_keeps_its_barriers_in_uniform_control_flow` (those starters and every sheet
starter and grid sheet fixture with `sheet_probes`: no early return, and each `workgroupBarrier()` at
the top level of `main` or directly inside `if (params.henad_count_fires != 0u)`).

**Lane buffers and packing** (`wgsl/pack.rs`). `LaneLayout` places each lane of a
compiled graph in a lane buffer, one `LaneGroup` each, and the lane buffers come first among the
spec's buffers, before the tick buffer of a grid and the fields, deposits and colours of agents. A
grid's `GridBufferSpec`s, `GraphHooks::lane_buffers` (its `buffer_lens`, seed and rerun uploads),
the agents' `AgentBufferSpec`s, `BufferSource::Lanes` and the position buffer all come from it.
`pack::choose` emits every shader with each lane in its own buffer, and only when some shader binds
more than `limits::MAX_STORAGE_BINDINGS` (8, the WebGPU default, never the device's limit) storage
buffers does it emit again with the plain lanes packed (`Packing::Plain`), then with the dual lanes
too (`Packing::PlainAndDual`). The same file therefore compiles to the same shaders whatever device
it builds on. A model still past 8 after packing compiles as it is, and a device with fewer
bindings refuses it at build as before. `LaneLayout::packed` groups the `packable` lanes (every
lane a shader holds but the drawable position lane and the merged lanes) by `PackKey`: the domain,
the writer step of a dual lane (so the lanes of one buffer pair share their side rows at every
step) and the actions whose reruns write the lane (`rerun_writers`, since a rerun uploads a buffer
whole from fresh host lanes). A group closes at `MAX_PACKED_BUFFER_BYTES` (128 MiB, the default
`max_storage_buffer_binding_size`) at the domain's largest count, `max_count` agents or the cells
of the default world (`grid_cells`, `world_cells`), and a group of one lane keeps its own buffer.
A packed buffer is labelled `lanes_<domain>_<k>` (`_in`/`_out` when dual) and holds `u32` words,
each lane a segment of `count` elements at `offset * count` words, its elements in order so
neighbouring invocations read neighbouring words. The emitter's `load`, `load_other` and `store`
go through `packed_word`: `<offset>u * <count> + (<index>)`, `2u * (<index>)` for a `vec2`, with
`count` `params.henad_agents`, `params.henad_cells` or `params.henad_width * params.henad_height`
(`count_of`, which adds `henad_agents` to a display block that lacks it), reads through
`bitcast<f32>` or `bitcast<i32>`, a `vec2` from two words, writes through `bitcast<u32>`, and
`load_other` bounds the index by the count in place of `arrayLength`. `bound_lanes` binds each
lane buffer a shader uses once, read-write when any of its lanes is written. Below the budget
nothing changes, and every existing starter's shaders are as before.
`GridProgram::lane_buffer(id)` and `AgentProgram::lane_buffer(id)` return the public `LaneBuffer`:
its buffer's position, its words per element, `is_packed`, `segment_range` and `segment(words)`,
the lane's words from the whole buffer read back. The crate-private `lane_groups` of both programs
(`LaneLayout::lane_ids`) returns each lane buffer's label and lane ids in buffer order.
`SheetFile::shortfalls` compiles the lowered graph again when a shortfall names a buffer of the
model (`<id>_buffer<k>_a` of a grid, `<id>_<label>_a` of agents), and names the buffer by its
properties (`a_shortfall_names_the_properties_a_buffer_holds`). henad-models' tests read a graph's
lanes through `tests/lanes.rs` (`GraphLanes::of`, `of_text`, `of_starter`, `read`), and
`planted_entry` plants a lane by id into its segment, never by buffer index. henad-graph's
`gpu_cases.rs` and `gpu_repeat.rs` build and read a case's lane buffers through
`support::lane_buffers` and `support::lane_words`, both over `compiled.layout`, and
`sheet_agents.rs` reads a lane through `AgentProgram::lane_buffer`. Tests: `tests/packing.rs` (the
ants and boids starters with one more Number property and `sheets/three_strains.toml` with one more,
each past 8 unpacked, within 8 packed and built on `baseline_test_device`; the same texts on two
compiles and a build on both devices; the ants with a Number, a Whole number and an Arrow more (an
`i32` and a `vec2<f32>` lane packed at non-zero offsets) and the three strains with one more, each
equal to its unpacked twin after 20 ticks and a press of Reset colony, through the thread-local
`pack::UNPACKED` switch), and the layout's unit tests in `pack.rs`.
