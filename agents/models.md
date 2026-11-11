# Models

The example models in henad-models, and how a new model is added.

**henad-models**: concrete simulations — `sir.rs` and `game_of_life.rs` (`GridModel`), `boids/`
(`AgentModel` over `NoField`), `ants/` (`AgentModel` over `ScalarField`, the one composite
model), `virus_network/` and `team_assembly/` (`NetworkModel`), `gpu_game_of_life/` and
`gpu_sir/` (`GpuGridModel`), `gpu_boids/` and `gpu_ants/`
(`GpuAgentModel`). A CPU agent model is split into
`lanes.rs` (the `agent_lanes!` declaration), `mod.rs` (metadata, params, stats) and `step.rs`
(the kernels); ants adds `field.rs` for its pheromone layer. A network model has the same
`lanes.rs` and `mod.rs`. `virus_network/` adds `step.rs` (the node pass) and `wiring.rs` (the
initial edges and the rewire). `team_assembly/` has no node pass and no `step.rs`. It adds
`assembly.rs` (the global pass and the setup teams), `ring.rs` (`RetirementRing`, a bucket queue
that finds the nodes due to retire without a scan) and `live.rs` (`LiveSet`, a dense list of the
live nodes for uniform draws). A GPU model is one `mod.rs` of
declarations next to its `.wgsl` files. Each GPU port starts from its CPU counterpart's tick 0
bit for bit, which keeps the two backends fair to compare, and seeds only in `seed_buffers`. The
agent ports call the CPU `init`. The grid ports repeat its draws (`gpu_game_of_life` bit-packed),
and a device-free test per port compares the seeded buffer with the CPU grid.
`example_models()` (`lib.rs`) registers all ten into one `ModelSet` with henad-models' own
`build_info!()`. The registry tests sit in `src/tests/registry.rs`. They run the testing kit
over `example_models()` on a baseline device, check each model's skipped checks, and keep the
guards for the example models alone: no GPU entry builds without a device, `gpu_sir`'s refusal
at 6000 by 6000, each GPU entry's `GpuNeeds` against its widest pass, and the set's coverage.
`no_starter_takes_the_id_of_an_example_model` reads every henad-graph starter, sheet and graph,
through `ModelFile::parse` and holds its id apart from the ten.
`gpu_boids` declares `REPLAYS_EXACTLY = false`, and its entry's `metadata().replays_exactly`
reads it.
`src/tests/graph_corpus.rs` holds the grid graph corpus to the stock models: henad-graph's starters
and the fixtures in `tests/fixtures/graphs/` (the exact port `gpu_sir_exact.toml` and
`repeat_trials.toml`, which has no stock model), read through `ModelFile::parse`.
`tick_0_cells_of_every_grid_corpus_file_equal_the_stock_init` runs each file's init on the host
evaluator at two sizes, two parameter sets and four seeds, and compares every lane the stock seeds
with `seed_buffers` (Life unpacked from its bits, `gpu_sir_exact`'s `rng` lane too). A grid file
without an `Oracle` arm fails it. `Oracle::Trials` is `repeat_trials.toml`, whose cells start at
their declared initial value, and its `stock_id` and `schema` are `None`. `Vote` there is a copy of
the template's `vote.rs`, since the template sits outside the workspace.
`every_grid_corpus_schema_hashes_like_the_stock_model_under_its_id` builds a graph's schema as a GPU
grid entry would (`grid_width` and `grid_height` up to 16384 at the world's defaults, then the
file's parameters, stats and actions) and compares `schema_hash` under the stock id, and so does the
entry `ModelFile::entry` builds. `schema_hash` reads the model id, so a `node_<model>` entry never
hashes like its stock model, and the test asserts that too. It checks the four files with a stock
model and asserts that count. The budgets test also holds each compiled entry's `shortfalls` empty
at 16384 by 16384, and `every_grid_corpus_entry_conforms` runs the testing kit over every grid
corpus entry on a baseline device. `src/tests/graph_grid_parity.rs` builds the starters from their
model files and holds them to the stock models every tick, reading lanes with `read_buffer`: Game of
Life to `gpu_game_of_life` (cells unpacked, Alive, a 64-step batch, and every press of Randomise and
Clear at three seeds) and Vote to the CPU `Vote` (cells and Ones). `graph_corpus::vote` is
`pub(super)` for it. `src/tests/support.rs` holds the helpers the test files share:
`baseline_device`, `model_entry` (the entry a model file's text builds), `starter_entry`,
`fixture_text` and `fixture_files` (files under `tests/fixtures/`), `graph_corpus` and
`agent_corpus`, `params_for` (the defaults with overrides, each id checked) and `device_params`
(the grid size too), `build_gpu`, `step`, `press`, `snapshot`, `stat`, `stat_values`, the buffer
and lane readers, `unpacked_life_cells`, `declarations_like` (the declarations both
`planted_entry`s copy), `assert_entries_conform` (the testing kit over a list of entries),
`assert_device_runs`, `agent_sheets` and `roomy_limits` (a device that holds a buffer of any size).
`src/tests/graph_sir_parity.rs` holds `gpu_sir_exact.toml` to `gpu_sir` every tick in cells, `rng`
words and counts, over a 64-step batch and presses of Seed outbreak, at three sizes and seeds. It
holds the starter `sir.toml` to `gpu_sir` at tick 0, and every tick and press under the rate gates
(infection rate 0 or 1, recovery rate 0 or 1), which leave no decision to a draw. Past those the
starter meets the bands of `tests/consistency_sir.rs` at its size, rates and seed and one more seed,
over the step from tick 0.
`tests/gates/` holds the data and pure functions of the SIR bands, the boids gate and the ants gate
(`sir.rs`, `boids.rs`, `ants.rs`). Each consistency test and the device test of the same model
include the file with `#[path]`, so the two hold one copy of the scenarios, parameters and bands.
The ants gate reads `TO_FOOD` from its includer. `tests/support/mod.rs` holds what the consistency
tests share: `fixture_dir` (the `HENAD_FIXTURE_DIR` switch `scripts/validate_ports.py` sets),
`reference_fixtures`, a network model's `network_params` and `param_index`, and `band`.
`src/tests/graph_repeat.rs` holds `repeat_trials.toml`'s share of hits to `1 - (1 - p)^trials`
within five standard deviations, each device tick to the host evaluator's `run_pass` of its tick
pass cell for cell, and a copy with `max = 140` refused by `Demand::shortfalls` at 4096 by 4096 and
by `build` before the engine allocates. `repeat_trials.toml` keeps `max = 4`, the largest bound
whose submission at 16384 by 16384 fits `SUBMISSION_WORK_BUDGET`. Keep these files.
The agents corpus (`graph_agent_corpus.rs` holds its tick-0 lanes) builds through `ModelFile::entry`.
`src/tests/graph_agent_parity.rs` holds `gpu_boids.toml` and the starter `boids.toml` to stock
`gpu_boids`: position and velocity buffers equal at tick 0 on the device, every statistic within
`STATS_TOLERANCE` (0.002 of the larger of the stock value and 1) over 8 steps at two seeds, the
stock port's invariants (in the world, speeds in the band, the reduced average speed against the
lanes, 300037 agents), and `gpu_boids.toml`'s sweep equal to the stock sweep of
`interleaved_gpu_boids_runs_commit_in_plan_order` in its engine-owned fields on one and four tracks,
`run_key` aside since it hashes the schema, which reads the model id. Neither graph replays, as the
stock port does not. `src/tests/graph_agent_runs.rs` runs the testing kit over every agents entry.
Two of its tests take the agents corpus and the agent sheet starters through `ModelFile`, named
`starters/`, `graphs/` and `sheets/`, on limits that hold every buffer.
`every_agent_file_at_its_largest_population_in_its_largest_world_fits_the_work_budgets` holds each
at its `max_count` with `world_width` and `world_height` at their maxima (10000) to the submission
budget, and each but the three ants files to the host budget with no shortfall. Each ants file lays
a ground of one cell per world unit, and its init on the host passes `HOST_WORK_BUDGET` at 10000 by
10000 at every population (`GROUND_INIT_PAST_THE_HOST_BUDGET`): that refusal alone, equal at the
declared and the largest population.
`every_agent_file_in_its_declared_world_admits_its_pinned_largest_population` admits each at its
declared population with every other parameter at its default, bisects over
`ModelEntry::shortfalls` (no device) for the largest population it admits and holds it to a pin in
the test's table: 201012 for both boids starters, 195355 for `gpu_boids.toml`, 552857 for the
bounded confidence sheet and `max_count` for the rest. One agent past a smaller pin is refused by
the submission budget alone, in `Demand::shortfalls`' words with the heaviest step pass. A change of
a pass's charge moves a pin. The file also holds `random_walk.toml`'s Repeat on the device
to the host's `run_pass` within 1e-4 world units over three ticks (not bit for bit: after one tick
36 of 1554 words differed by one unit in the last place on an Apple M4 Pro), and builds a crowd of
70000 agents at the origin of a world of 1000, wide so the budget admits it, whose radius fold
faults with its pass, node and line at the next stats read and in a run that reads none, after
which the device runs stock `gpu_boids`.
`a_radius_fold_over_the_whole_world_is_refused_or_faults_with_its_place` holds a world-wide walk to
the budget over coarse cells and to the cell cap over fine ones, and
`a_field_update_reads_each_layer_at_its_own_cell` holds a `finish` to its own cell in each layer.
`graph_agent_parity.rs` also models on the CPU where the boids graphs' radius fold reaches its
65536-visit cap inside the stock bounds, at tick 0 from the stock seed
(`the_boids_graphs_walk_past_the_cap_only_where_the_budget_refuses_them`): a million boids at
`visual_range` 200 below about 1820 a side, at 50 below about 460. It holds the submission budget
to refusing both graphs in every such row, and per row and graph whether the budget refuses
(about 195000 boids of `gpu_boids.toml` and 201000 of `boids.toml` in the default world). No
device runs there. The stock port has no cap and no budget. The same file holds both boids graphs to the cross-engine gate of `tests/consistency_boids.rs` (its two
scenarios planted, one tick within 1e-5 of the CPU `boids` and of every engine's fixture) and to
that file's speed and position bounds over 50 ticks. `graph_agent_corpus.rs` holds every agents
corpus schema to stock `gpu_boids` and `boids`, or `gpu_ants` and `ants`, hashed under the stock id.
`src/tests/graph_ants_parity.rs` runs a contraction probe (the stock deposit's `max(best, field *
cut + reward)` in one expression against the generated two `let`s, equal on the device, and fused on
an Apple M4 Pro), holds `gpu_ants.toml` to stock `gpu_ants` bit for bit every tick in every lane,
both layers, the sites, the colours and the three statistics, across a Reset colony press (1540
ticks at 2000 ants reach the first deliveries), and holds the starter `ants.toml` to `gpu_ants` at
tick 0 on the device, to the CPU `ants` under the cross-engine gate of `tests/consistency_ants.rs`
bit for bit (the gate's ants and field planted through `planted_entry`, whose `PlantedHooks` wrap
the graph's `AgentHooks`), to exact decay of every cell no ant stands on, and to that file's other
checks. `src/tests/graph_agent_repeat.rs` holds the Repeat tests of the plan over agents, each on
the device and the host: known answers (zero iterations, `when` false, a clamped count, a count per
agent, an inner count reading the outer `k`), a hash chain equal to its hand-chained copies and to
`pcg_hash`, two iterations of one draw differing and replaying, a `grid.fold` in a Repeat and a
Repeat in a `grid.fold` at their closed forms, the random walk's replay, step length and mean
squared step, and a Repeat of 16384 refused by the budget at a million agents. Its float body
(`FLOAT_LOOP`, `prev * 1.1 + k`) replays on the device, and
`a_float_repeat_over_agents_equals_its_hand_chained_copies_bit_for_bit` is `#[ignore]`d as a probe of
a device and no gate: a device that fuses a multiply with an add rounds the loop and its copies
differently (`agents/gpu-traps.md`).
`src/tests/graph_grid_sums.rs` reduces a grid graph's `stat.sum` and `stat.mean` of an `f32` lane
and a `vec2f` sum on the device through the engine's `partials` tree, at sizes from 1 by 1 to 300
by 211, to closed forms that every order of addition reaches exactly. Keep these files.
`tests/fixtures/lowered/` holds the hand-lowered references a lowered sheet is compared with: eight
models in `round1/` (`r1_<model>`, SIR with chances per tick, Seed outbreak named as in the study
that wrote them) and `sir_rates.toml`, `sird_competing.toml` and `sird_higher_wins.toml`
in `rates/` (`r2_<model>`, the tick length a `tick_length` parameter in days). They are graph files
written by hand, with hand-numbered node ids and keys, and no script writes them.
`tests/fixtures/docs/lowered_fixture.md` holds the procedure: the mapping from each author form to
the graph, the rate conversion, and the 20-step `henad-cli --model-file` run with every action that
checks a file before it is committed, again after any edit. They sit apart from
`tests/fixtures/graphs/`, whose loaders hold every file to a stock model.
`src/tests/lowered_fixtures.rs` holds `every_lowered_fixture_reads`: the two directories hold
exactly these eleven files, each with `kind = "graph"` in its header, read by `GraphFile::parse` and
checked with no error. Keep these files.
`src/tests/sheet_grid.rs` holds the grid sheets on the device: the grid sheets of
`henad_graph::sheet::starters()` (filtered by the lowered world, so an agents sheet joining the
starters stays out), `tests/fixtures/sheets/sir_chance.toml`, a copy of henad-graph's test sheet
`src/tests/sheets/sir_chance.toml` (the SIR of `round1/`, with chances per tick), and
`tests/fixtures/sheets/sird.toml` below. Copy them again when the source changes, and henad-graph's
`the_copies_of_the_test_sheets_equal_their_source` fails until they are. Entries come from
`ModelFile::parse`. A test that needs given tick-0 lanes plants
them through `planted_entry`, whose `PlantedHooks` wrap the lowered graph's `GridHooks` and replace
the seeded words of the lanes it names, since no sheet start and no app tool paints cells.
The tests: the Game of Life sheet's blinker (period 2) and glider (moved by one cell on each axis
after 4 ticks) on a 32 by 32 world; `a_start_again_button_draws_a_new_layout_each_press` (two
presses of Randomise differ, replay in a second build, and equal the host's `rerun`) and
`a_rebuild_replays_the_same_start`; `every_grid_sheet_steps_alike_on_the_host_and_the_device`
(every lane every tick for 50 ticks at 64 by 64, two seeds, the forest fire at lightning 0.001 and
growth 0.05 so every draw decides cells, the SIRD's competing group included, and a press of New
outbreak after tick 10 on each sheet that has it, through `host_press` on the host);
`a_competing_group_with_a_rate_for_each_steps_alike_on_the_host_and_the_device`, the same
comparison (`assert_steps_alike`) over the SIRD with line 4's death rate for each infected
neighbour, whose group works out `e^(-h dt)` and its shares per cell in the tick shader, held to
Recovered and Dead cells at each seed (a device whose `exp` or division differs from the host's in
its last places fails only when a draw falls between the two thresholds); the chance SIR's lanes
and measures summing to the cells
for 200 ticks across a New outbreak press; Game of Life and Vote, whose cards draw nothing, equal to
their files in `lowered/round1/` in every lane every tick for 200 ticks at 64 by 64 from the tick-0
lanes each sheet draws at seeds 1 to 4, on the host (every tick pass in document order) and on the
device (both planted); and `every_grid_sheet_entry_conforms`, the testing kit over every grid sheet
entry. The forest fire and the chance SIR draw with other keys than their hand-lowered files and
meet bands instead (`the_forest_fire_and_the_chance_sir_sheets_sit_within_their_hand_lowered_bands`,
whose doc holds the procedure). Measures of one run: the peak share of Burning or Infected, each
state's share at the last tick, and the share of cells ever in that state (the burnt share). Each
side runs at seeds 1 to 4 from its own start, the forest fire at 512 by 512 for 150 ticks at
lightning 5e-5, the SIR at 512 by 512 for 60 ticks at infection and recovery chances 0.1. The
sheet's mean over the seeds sits within five standard errors of the difference of the means about
the hand-lowered file's mean, the variance of one run the larger of the variance pooled over the
eight runs and the binomial `p (1 - p) / cells`. Behind each band a planted rule error (the
"from" state dropped from the forest fire's lightning line, which then strikes empty cells too,
and from the SIR's recovery line) moves at least one mean outside. Keep these files.
`src/tests/sheet_rates.rs` holds the rate gates on the same device. The SIR starter
`sir.toml` (rates per day) at beta 0.3 and gamma 0.1 on 256 by 256 cells for 80 days, seeds 1 to 3,
at ticks of 1, 0.5, 0.25 and 0.125 days against 0.0625: the largest gap in infected share, sampled
at the start of each day, falls with each halving to at most 0.8 of the one before, and the chance
SIR at its one-day chances per tick, read with the same ticks per day, fails that rule, so the test
tells the two apart. `tests/fixtures/sheets/sird.toml`, a copy of henad-graph's test sheet (gamma =
mu = 0.05 per day as competing rates), at a tick of 7 days on 512 by 512 cells, seeds 1 to 3, ends
with the share of the removed cells that are Dead within 0.01 of 0.5, and the hand-lowered
`lowered/rates/sird_higher_wins.toml` within 0.01 of its predicted 0.4134 as the planted contrast.
`two_runs_at_one_seed_write_identical_statistics` runs the SIR starter twice at seed 5 and a tick of
half a day. The rates SIR also joins every grid-sheet gate of `sheet_grid.rs` through
`starters()`, and the SIRD joins them through `grid_sheets`, six grid sheets in all. Keep these
files.
`src/tests/sheet_agents.rs` gates the agent sheet starters (the random walk, boids, bounded
confidence and the ants, whose tick pass binds exactly 8 storage buffers) on a
`TestDeviceRequest::baseline()` device: `every_agent_sheet_lowers_checks_and_compiles` (no warning,
no graph diagnostic, 100 ticks and a stats read),
`the_lowered_agent_sheets_bind_at_most_8_storage_buffers` (each pass's `Demand`),
`every_agent_sheet_entry_conforms` (the testing kit, then at `max_count` either no shortfall or only
`SheetFile::shortfalls`' words with no forbidden word), and
`a_capped_fold_fault_names_the_author_line` (the boids with a planted move to the origin, so the
second tick's fold meets 70000 birds within the budget: the fault starts "lines 1 and 2, Flockmates
and Too close:" and names no node or pass, and the device runs the random walk afterwards). Three
bands hold each sheet and its `round1/` file on seeds 1 to 3, each procedure in the test's doc and a
planted rule error outside it: the random walk's mean squared step over a tick within five standard
errors of `4 * 2^2` (the planted error moves once outside the Repeat), the boids' speeds within 3
and 15 after each of 300 ticks with 20000 birds and their polarisation after the last above 0 and
at most 0.95 (the planted error steers by the average velocity, near 1, and a run on the device is
not repeatable at one seed, so no lower edge holds), bounded confidence's one
opinion group (runs of 0.01 bins each holding 1% of the people) after 100 ticks (the planted error
steers by the average, no group), and the ants' deliveries, at most 1000 by tick 2000 and at least
500 by tick 3000 at the defaults (`the_ants_sheet_delivers_food_in_band`: the `round1/` file
delivers nothing by tick 2000 at seed 2, so the lower edge sits at tick 3000; the planted error
picks the least To food as the Food way). Keep these files.

## Adding a new model

A user's model lives in a project made from `templates/model-project`, and the guide's pages under
`docs/guide/first-model/` add one there: a file or directory under `src/`, a `mod` line and an
`insert` line in `models()` in `src/lib.rs`, and no build file, since `ShaderBuild::discover("src")`
finds every shader. An example model of Henad's own goes into henad-models instead, as below.

Pick the trait matching the topology and the backend. All five are const metadata plus pure
functions. The engine owns allocation, buffering, chunking, RNG seeding, param storage, the views,
and the whole `SimState` impl.

1. **`GridModel`** (`henad-core/src/authoring/model/grid_model.rs`) — cellular automata over `u8` cells.
   Implement `init`, `step_cell`, `stats` and the consts; `cpu/grid_engine.rs` does the rest, including the parallel
   row-wise step. Grid width/height are prepended to the param list at indices 0 and 1. See
   `game_of_life.rs`, `sir.rs`.
2. **`AgentModel`** (`henad-core/src/authoring/model/agent_model.rs`) — a population of agents, optionally over a
   field. Declare lanes with `agent_lanes!`, then implement `init`, `run_step_pass` and `stats`.
   `run_step_pass` is normally one call to the generated `lanes.run_pass(CHUNK, seed, tick, ..)`
   with a per-agent closure, which is where the chunking and seeding happen — see
   `boids/step.rs::run`. `num_agents`, `world_width` and `world_height` are prepended at indices 0,
   1 and 2. `Lanes` comes from the macro; four more associated types pick the behaviour:
   - `Field` — `NoField` (boids), `ScalarField<S>` for scatter-plus-decay layers (ants), or
     `CaField<M>` to put a `GridModel` underneath a population.
   - `Index` — `SpatialHash` when agents read each other, `NoIndex` when they don't.
   - `Tally` — a per-chunk reduction merged in chunk order, `()` when there's nothing to count.
   - `Params` — hot params extracted once per tick, so the kernel does no enum matching.

   A model needing a second pass over agents before the step (ants filling deposit lanes)
   overrides `run_deposit_pass`.

3. **`GpuGridModel`** (`henad-core/src/authoring/model/gpu_grid_model.rs`) — a grid stepped by a compute
   shader. Three WGSL sources (step, display, reduce), buffer lengths, seeds and a uniform block.
   All `K` buffers ping-pong together. Each pass binds buffers by label, and the shipped display
   and reduce shaders read only the first.
4. **`GpuAgentModel`** (`henad-core/src/authoring/model/gpu_agent_model.rs`) — a population stepped by
   compute shaders. Unlike a grid, a step is a _list_ of passes, because the two real models
   disagree about almost everything structural: boids rebuilds a neighbour index and runs one pass
   over three ping-ponged lanes, ants runs two passes over seven in-place buffers with a display
   pass and a persistent counter. So a model declares `BUFFERS`, `STEP_PASSES` and an optional
   `DISPLAY`, and each pass points at its shader's generated declarations,
   `crate::binding_decls::bindings::<SHADER>`, brought in by `include_shaders!`. The engine builds a
   second buffer side only when some `BufferSpec` asks for it, so a model that writes in place pays
   nothing for double buffering. `Domain` has exactly three variants because those are the three the
   two models use — do not add speculative ones.
5. **`NetworkModel`** (`henad-core/src/authoring/model/network_model.rs`) is a population of nodes
   joined by edges, on the CPU only. Declare node lanes with `agent_lanes!` as for an agent model,
   then implement `init`, `stats` and whichever passes the model needs. `init` gets a graph holding
   every node and no edges. A tick runs the sequential `run_global_pass` first, the only pass of
   the tick that can change the graph. The parallel `run_node_pass` follows, over every slot,
   retired ones included. The node pass is normally one `lanes.run_pass(..)` call, as in
   `virus_network/step.rs::run`, and Team Assembly has none. `num_agents` (the node count),
   `world_width` and `world_height` are prepended at indices 0, 1 and 2. `Params` are hot params
   as for an agent model, and `Aux` is the model's own state outside the lanes and the graph.
   Nodes come and go through `Nodes::spawn` and `Nodes::retire`. Both keep the lanes and the graph
   the same length. A retired node sits at `NaN`, and a reused slot keeps the old node's lane
   values. `stats` sees `Aux` immutably. A stat that walks the graph (Team Assembly's components)
   is computed in `prepare_view`, cached in `Aux` against `Network::version()` and the node count
   (a spawn leaves the version alone), and counted by `aux_heap_bytes`. `directed` is read every
   tick, and a flip rebuilds the rows. `EDGE_PALETTE` colours each edge by its colour byte, and
   `LAYOUT` tunes the spring layout.

Every trait can declare one-off actions, drawn as buttons in the Parameters panel and named by
`henad-cli --act ID@TICK`. A CPU trait lists them in `ACTIONS` and runs them in `act`, and
`henad_core::actions!` declares the list and numbers each entry by its position. A GPU trait's
`ACTIONS` lists `GpuGridAction` or `GpuAgentAction` passes instead, and the seed passed to build
its uniform block is fresh on every press. An action runs between ticks (`SimCommand::Act` in the
runner) and draws from its own stream (`action::action_seed`). Otherwise a press would draw the
numbers the next tick would have.

`SimState` is the runner interface, not a sixth authoring path. Implement one of the traits above
rather than `SimState` directly.

A GPU port that leaves the order of its writes to the device declares `REPLAYS_EXACTLY = false`,
as `gpu_boids` does, and the kit then skips the checks that compare two runs of one seed. A port
starts from its CPU model's tick 0, seeded from `grid_init_rng(seed)` or `agent_init_rng(seed)`.
The agent ports and the template's `gpu_vote` call the CPU `init`, and the example grid ports
repeat its draws.

Either way, register an example model in `henad-models/src/lib.rs::example_models()` via the
`register_*` generic for its trait, so it's type-erased into a `ModelEntry` and shows up in the UI.
Nothing about an entry should be written by hand. Name, params, stats, actions and
`topology_hint` are all derived from the trait. The registry tests run the testing kit over the
set, as the template's `every_model_conforms` does over a project's `models()`. It is the safety
net that a model's declared params, topology, actions and stat series match what its state
actually does, that its results do not depend on the thread count, and, when a device is
available, that a GPU entry builds on a baseline device and runs a full submission.
