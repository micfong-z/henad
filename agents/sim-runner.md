# Sim runs off the UI thread

`SimThread` (`henad-compute/src/cpu/sim_thread.rs`) exists so simulation stepping never blocks
rendering. It is a thin handle over a `Driver<Loop>`, and the split underneath it lives in
`henad-compute/src/runner/`. A `SimLoop` does whatever is due now and says when it next wants
calling, as `Pace::Idle`, `Pace::Now` or `Pace::After`. A `Driver` decides how to wait.
`runner/thread.rs` runs the loop on an OS thread of its own, taking `mpsc` commands.
`runner/frame.rs` pumps it inline from `SimThread::update()`, which `henad-app` calls each frame,
and stops once a frame has spent `PUMP_BUDGET_MS`. `wasm32-unknown-unknown` cannot spawn a thread
even with atomics, hence the second driver.

The `#[cfg(target_arch = "wasm32")]` choosing between the two sits in `runner/mod.rs` and nowhere
else, so stepping is written once. `SimThread`'s API (`play`/`pause`/`step_once`/`run_to`/
`set_schedule`/`send`/`update`/`stop`) is the same either way and `henad-app` never learns which
driver it holds. `gpu/sim_thread.rs` is driven the same way.

A loop handles its stop command only between two pumps, and a step that never returns cannot be
interrupted. `Driver::shutdown(cmd, timeout)` sends the stop command and polls
`JoinHandle::is_finished` until the timeout, then joins the thread (`RunnerShutdown::Joined`) or
hands it back unjoined (`RunnerShutdown::Detached(DetachedRunner)`). `SimThread::stop` and
`GpuSimThread::stop` take the timeout, and both drops stop with `DEFAULT_STOP_TIMEOUT` (2 s) and log
a detach. Nothing joins a thread without a bound. The frame driver stops at once and never detaches.
`SimThread::new_in(pool, ..)` (native) builds the first snapshot inside `pool` and runs the loop's
start, each pump and each command through `pool.install`, through `Driver::spawn_in`. A stuck
kernel then holds workers of that pool alone. `SimThread::new` keeps the shape above: commands on
the runner thread, and each pump in `rayon::scope`. `GpuSimThread::new_in` runs its loop in a pool
the same way, and a GPU pump then never waits for a worker of a pool a stuck CPU step holds.
`stop_tests` in `cpu/sim_thread.rs` holds
`a_runner_that_never_returns_is_detached_within_the_timeout`,
`dropping_a_runaway_runner_returns_within_the_timeout`, `a_runner_that_stops_is_joined` and
`a_runner_in_a_pool_steps_on_its_workers`, and `gpu/sim_thread.rs`
`a_gpu_runner_in_a_pool_pumps_on_its_workers`. Keep them.

On native the app never builds or tears down on the UI thread without a bound. It builds on the
`henad-model-build` thread, queues the commands sent meanwhile, stops a runner within `STOP_WAIT`,
and keeps a detached runner or build until it ends. The next build waits for it, and once it has
run past `DEFAULT_STOP_TIMEOUT` the app's CPU work moves to a fresh pool. In a browser the build
runs inline, and a step that never returns freezes the page. `agents/henad-app.md` has the
details.

`SimCommand::SetSchedule` replaces the loop's `Schedule`, fires the entries due at the current tick
unless that tick has fired, and publishes. Both loops keep `fired_through`, the highest tick whose
actions have had their turn, set once a step reaches a tick (on the GPU, once a batch or
`StepOnce` passes it) or a schedule fires it. Only the first schedule sent before the first step
fires the build tick, and no tick fires twice. From then on each entry fires once, after the step
that reaches its tick.
This is `Fire::AfterStep`, the rule of `henad-cli --export-stats` and a sweep's `RunCursor`, and
the snapshot at tick t carries tick t's actions. An empty schedule costs one test per step, and
`Schedule::run_due` allocates nothing when no action is refused. `SimCommand::RunTo(tick)` steps
uncapped toward the tick and never past it, publishes every `runner::RUN_TO_PUBLISH_INTERVAL` on the
way, and at the tick pauses and force-publishes. `Play`, `Pause` and `StepOnce` cancel it. A tick
at or behind the current one pauses at once. Going back is the host's job, and the app rebuilds
first. The GPU loop cuts a batch at the target and a submission at each tick an action is due at,
gives each action a submission of its own (`encode_action` needs one per press), and at the target
takes the blocking `snapshot_now`. On native the stats then equal the state at the target. In a
browser the readback cannot block, and the stats land on a later pump. A loop told to step while a
one-shot snapshot's readback is still out (`late_stats`) waits for it and publishes that tick
again first. Otherwise the build's zeros stay in the history as tick 0's row. Either driver runs a
run-to through the same `pump`, and the frame driver cuts it at `PUMP_BUDGET_MS` like any other.
`a_replay_of_a_planned_run_matches_its_sweep_row` and `a_gpu_schedule_matches_export_stats`
(henad-explore) pin the live loop's rule to a sweep's and to `--export-stats`. Keep them.

A sweep started from the app runs off the UI thread as well, through
`henad_explore::handle::SweepRun`. On native the handle spawns a thread that runs the sweep as
`run_spec` does (`run_in_memory` or `run_into_directory`), lanes or tracks and all, and its
`SweepControl` holds or ends every run between two slices of steps. In a browser it wraps a
`runner::Driver<PumpedSweep>`, and `HenadApp::logic` pumps it each frame through
`ui::sweep::update` and `SweepRun::update`. Either way the host reads `SweepEvent`s from an
`mpsc` channel that loses none (`Planned`, each `RunFinished` in plan order, a search's
`SearchBatchTold` with an `Arc<SearchUpdate>` after each batch's runs, then one `Finished` with the
`SweepRecord`, or `Failed`), and a `SweepProgress` where the latest write wins. The `wake`
callback of `SweepRunOptions` runs after each event, and an idle UI then repaints to collect it.
Dropping the handle aborts the sweep, and on native waits for the thread to write its files.
`SweepRun::stop(timeout)` aborts and waits at most `timeout`, and leaves a thread held in a step
detached. `SweepRunOptions::build_pool` (native) names the pool the probes and a GPU model's run
builds run in (`SweepOptions::in_build_pool`, `Executor::with_build_pool`), and a CPU model's runs
build in their lane pools either way. A pause never changes a result. A run's trajectory depends
on its seed and schedule alone.

The UI thread only ever reads the latest snapshot (`snapshot.rs`) and never touches the live
`SimState` directly. Publishing goes through a `SnapshotSlot`, holding the `fresh` snapshot plus a
`spare` the host hands back for its buffers. Recycling is only an optimisation. Dropping a snapshot
instead means the next publish allocates. A view borrows its palette from the state, and a CPU
snapshot owns a copy, refilled into the recycled `Vec` like the cells and the lanes.

`build_snapshot` calls `SimState::prepare_view` first, which is where a model turns state into
something drawable — ants quantises its `f32` pheromone field into palette indices there. That
runs on publish, not every tick, so anything a view needs but a step doesn't belongs in
`prepare_view` rather than in `step`.

A network model's layout relaxes next, in `SimState::relax_layout`, and stats come last. The layout
runs at least one iteration and keeps going until the budget from `SimCommand::SetLayout` is spent.
The budget is capped at `runner::MAX_VIEW_BUDGET_MS` on wasm, where the view is prepared inside the
frame pump. The layout moves nodes only on a publish that follows a tick, or on every publish while
paused if `while_paused` is set. An action publishes without a tick, and leaves a paused network
where it is unless `while_paused` is set. The layout never runs inside `step()`, so node positions
are not a function of the tick. The edge list is copied into the snapshot only when
`Network::version()` or the edge count changes. A recolour through `Network::update_colors` that
changes nothing leaves the version alone. The GPU runner accepts `SetLayout` and ignores it.
