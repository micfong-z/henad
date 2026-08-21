---
date: 2026-08-21
title: "Six fixes from the runner review: frame pacing, a late readback, an uncapped floor, and CLI ranges"
model: claude-opus-5 (Claude Code)
issue: none
status: complete — all six fixed, each with a regression test, verified in the native app
baseline_commit: b82b406
delta_state: uncommitted working tree on `master`
---

# Runner review fixes

> Four bugs and two inconsistencies came out of a review of the stage 3 runner split, and all six are fixed.
> Two of the four are web-only and both trace to the same thing: the frame driver has no equivalent of `recv_timeout` waking on a command, and `poll_blocking` in a browser does not block.
> The frame driver now compiles natively under `cfg(test)`, which is the only way anything in it can be tested at all.
> The `requestAnimationFrame` limitation from session 13 still holds, so the two web fixes are covered by tests and unverified in a real browser.

## State before

`runner/frame.rs::send` handled a command and left `next_pump_at` where the last `Pace::After` put it.
A model capped at a low tick rate returns `After(~1s)`, so dragging Target TPS up did nothing until that wait expired.
The threaded driver never had this, since `recv_timeout` wakes on the command itself.

`GpuSimState::poll_stats_readback(block = true)` degrades to a non-blocking poll on wasm, where `CounterReadback::poll_blocking` cannot wait on anything.
`snapshot_now` therefore published before its readback landed, and `pump` returned `Pace::Idle` while paused without touching the readback again.
After Build in a browser, population and the stats panel read zero until the first Play or Step, which is the same regression stage 3 fixed on native with `SnapshotSlot::empty`.

`uncapped_batch_for` clamped to at least one *batch*, and a batch is `ticks_per_snapshot` steps.
Ticks/snapshot goes to 1000, so a model at 5 ms a step could run a five-second pump against a 6 ms budget.

`henad-cli`'s `resolve_params` parsed a `--set` value and never checked it against the descriptor's own min and max, and a numeric `Choice` went through without being checked against the option list.
`--set num_agents=4000000000` reached `init` unaltered.

CPU Pause left the last running rate in the toolbar where GPU Pause zeroed it, and CPU StepOnce called `update_tps`, dividing one step by however long the pause before it had lasted.

## What was done

### The frame driver, and testing it at all

`send` resets `next_pump_at` to now.

`runner/mod.rs` builds `mod frame` under `any(target_arch = "wasm32", test)`.
The module only needs `web_time` and `fault::Fault`, both of which exist on native, and the `Driver` alias still resolves per target.
Both web-only bugs lived on this path and nothing could reach it before.

### A readback that lands after the snapshot

`GpuSimState` gained `stats_readback_pending`, forwarded from `CounterReadback::is_pending` and `Reduce::readback_pending`.
The GPU loop's `!running` arm now calls `collect_late_stats` instead of returning `Pace::Idle` outright: it polls a pending readback, republishes once nothing is outstanding, and wakes the host while it waits, since a repaint is the only thing that brings a frame-driven loop back.
Native is unaffected, where `poll_blocking` really blocks and nothing is ever pending by the time the loop idles.

### The uncapped floor

`uncapped_batch_for` became `uncapped_steps_for` and returns steps.
What fits the budget is rounded down to whole snapshot strides once one fits, keeping the tick count a multiple of the stride, and the floor under that is one step.
`MAX_UNCAPPED_BATCH` and `UNCAPPED_BATCH_MS` were renamed to `MAX_UNCAPPED_STEPS` and `UNCAPPED_PUMP_MS` to match the unit.

### CLI ranges

`parse_value` refuses an `F32` or `U32` outside the descriptor's range and a `Choice` index past the end of the options, and `resolve_params` adds `--set <id>` as context.
`henad-cli` had no tests, so it now has an inline module for the override path.

### The two inconsistencies

CPU Pause zeroes `actual_tps`, matching the GPU runner.
CPU StepOnce calls a new `reset_tps_window` in place of `update_tps`, the same shape as the GPU fix in session 04.
Play uses that helper too.

```text
crates/
├── henad-cli/src/main.rs                       # range-checked --set, first tests in the crate
└── henad-compute/src/
    ├── cpu/sim_thread.rs                       # uncapped_steps_for, reset_tps_window, pause zeroes TPS
    ├── gpu/
    │   ├── agent_engine.rs                     # stats_readback_pending
    │   ├── grid_engine.rs                      # stats_readback_pending
    │   ├── sim_thread.rs                       # collect_late_stats, trait method, its test
    │   └── primitives/
    │       ├── readback.rs                     # is_pending
    │       └── reduce.rs                       # readback_pending
    └── runner/
        ├── frame.rs                            # send resets the deadline, plus the module's tests
        └── mod.rs                              # frame builds natively under cfg(test)
```

## State after

`./check.sh` passes end to end with `HENAD_REQUIRE_GPU=1`.

Every fix has a test that fails without it, each confirmed by reverting the fix and watching the assertion it was written for:

- `a_command_makes_the_next_pump_due` — pumps stay at 1 without the reset.
- `a_readback_landing_after_the_initial_snapshot_is_published` — population stays 0.
- `a_stride_too_slow_for_the_budget_is_not_run_whole`, `an_uncapped_pump_runs_whole_snapshot_strides`.
- `a_value_outside_the_descriptor_range_is_refused` and three more in henad-cli.
- `a_pause_and_a_step_after_it_report_no_rate` — pause reported the running rate, and the step after it reported 80 TPS off a stale window.

The native app was driven through the whole path: SIR built, ran at 27 TPS, paused to 0, stepped one tick with the rate staying 0, ran uncapped at Ticks/snapshot 1000, and `gpu_ants` built with population 2000 at tick 0 and ran at 27 kTPS.

## Issues found & future directions

- **The two web fixes are unverified in a browser.** The app loads in the automated browser, reports its 14 workers and gets a `BrowserWebGpu` adapter, and then `requestAnimationFrame` fires zero times in two seconds, so `eframe`'s loop never runs an update. Same limitation session 13 hit. Both fixes are covered by tests that run on native, and neither has been seen working in a real browser.
- **`collect_late_stats` wakes the host every frame while a readback is outstanding.** That is bounded by the map resolving, which `map_async` always does, but a paused browser tab spends real frames on it. Nobody has measured how many frames that actually is.
- **`stats_readback_pending` is a second trait method where one would do.** `poll_stats_readback` already knows whether a value landed and returns nothing. Returning it would not have been enough for the agent engine, which polls two readbacks and would have to say which of them the answer was about, so the pending query stayed separate.
- **The uncapped path no longer keeps tick counts on stride boundaries for a slow model.** A model whose stride does not fit the budget now ends a pump mid-stride. Whether Ticks/snapshot is meant as a hard granularity or as a publish hint has never been written down.
- **A `Choice` parameter has no model using it.** The CLI's index check is covered by a hand-built descriptor in the tests and by nothing in the registry.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

- Identified all 6 issues and their corresponding causes, for the agent to implement the fixes.
- Reviewed all changes.
