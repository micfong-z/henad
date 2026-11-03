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
`pub(super)` for it, and so are the file's helpers (`build_gpu`, `step`, `press`, `snapshot`,
`stat`, `graph_cells`, `starter_entry`) for the two files below. `src/tests/graph_sir_parity.rs`
holds `gpu_sir_exact.toml` to `gpu_sir` every tick in cells, `rng` words and counts, over a 64-step
batch and presses of Seed outbreak, at three sizes and seeds. It holds the starter `sir.toml` to
`gpu_sir` at tick 0, and every tick and press under the rate gates (infection rate 0 or 1, recovery
rate 0 or 1), which leave no decision to a draw. Past those the starter meets the bands of
`tests/consistency_sir.rs` at its size, rates and seed and one more seed, over the step from tick 0.
`src/tests/graph_repeat.rs` holds `repeat_trials.toml`'s share of hits to `1 - (1 - p)^trials`
within five standard deviations, each device tick to the host evaluator's `run_pass` of its tick
pass cell for cell, and a copy with `max = 140` refused by `Demand::shortfalls` at 4096 by 4096 and
by `build` before the engine allocates. `repeat_trials.toml` keeps `max = 4`, the largest bound
whose submission at 16384 by 16384 fits `SUBMISSION_WORK_BUDGET`. Keep these files.

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
