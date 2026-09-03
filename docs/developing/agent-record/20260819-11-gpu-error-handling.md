---
date: 2026-08-19
title: "Fault handling: wgpu errors and model panics stop the simulation instead of the process"
description: Catching wgpu errors and model panics so they stop the simulation rather than the process.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: "#31"
state: complete — both failure paths verified in the running app, nothing committed
baseline_commit: 50481c0
delta_state: branch `31-gpu-error-handling`, uncommitted working tree
---

# Fault handling

> Every wgpu error in the process used to be fatal, and every panic out of a sim thread used to freeze the viewport with no message.
> Both now become a `Fault`. It stops the simulation, names itself in a modal, and leaves the app usable.
> The device survives, and a failed build is one dismissal away from another attempt.

## State before

At `50481c0` on `master`, working tree clean.

Nothing in the workspace called `push_error_scope`, `on_uncaptured_error` or `set_device_lost_callback`.
`egui-wgpu` 0.36.1 installs no handler either, and it owns the device Henad borrows.
That left `default_error_handler` in `wgpu-30.0.0/src/backend/wgpu_core.rs:692` in charge of every error.
It logs "Handling wgpu errors as fatal by default" and panics.

The consequences differed by thread, and the second was worse than a crash.

Model construction runs on the UI thread, inside `AppState::reset_simulation`.
A wgpu error there killed the app outright.
`gpu/capacity.rs` already covered the three cases that had actually been hit (buffer sizes, texture dimensions, storage-binding counts), but it checks limits rather than free device memory, and it cannot see shader validation, workgroup size or uniform layout.
`limits.rs:14` even logged "the widest ones will fail to build" for an adapter short on storage buffers, a path with no handling behind it.

On a sim thread, the thread died and the UI kept running.
`take_snapshot` does `self.snapshot.lock().ok()?`.
A mutex poisoned by a panic mid-publish then read as "nothing new" forever.
The viewport froze, Play stayed lit, and nothing said why.

The same applied to plain Rust panics from model code, the larger class of the two.
An accidental division by zero in a kernel, or one of the engine's own contract `assert!`s.

## What was done

Three catch points and one sink. No new dependency: `egui::Modal` is already in egui 0.36, and `anyhow` stays where it belongs, in `henad-cli`.

```
crates/
├── henad-compute/src/
│   ├── fault.rs                    ← new, above both backends
│   ├── cpu/sim_thread.rs           ← thread body wrapped, `new` takes a sink
│   └── gpu/
│       ├── fault.rs                ← new, error scopes around a block of work
│       ├── mod.rs                  ← GpuContext takes the sink, installs the handler
│       └── sim_thread.rs           ← thread body wrapped, loop checks the sink
├── henad-models/src/
│   ├── registry.rs                 ← ModelFactory returns Result
│   └── tests/broken.rs             ← new, a model that divides by zero in `init`
├── henad-app/src/
│   ├── lib.rs                      ← hook, sink, per-frame poll
│   ├── state.rs                    ← build_runner, report_fault, `fault` field
│   └── ui/fault.rs                 ← new, the modal
└── henad-cli/src/main.rs           ← `?` on the factory, hook, sink check after a run

AGENTS.md                           ← new `### Sentences` section, rewritten `### UI`
```

### The type

`Fault` carries a `during` and a `FaultKind` of `Device(wgpu::Error)`, `Panic { message, location }` or `Refused(String)`.
It implements `Display` and `Error`, with `source()` returning the wgpu error.
`?` then works straight into `henad-cli`'s `anyhow` with no conversion.

`Refused` was added late, for the two host-side "this cannot run here" cases in `build_runner` (no GPU context, GPU model on the web build).
Both used to be a `log::error!` and a silent early return.

The panic location comes from a hook `install_panic_hook` sets once.
It records `file:line` into a `thread_local!`, then calls the hook that was already there.
Stderr and test output are unchanged.
Thread-local rather than a global mutex.
The hook runs on the panicking thread and `catch_unwind` returns on that same thread, leaving no cross-thread race to reason about.

### Where the catches sit

`fault::catching` wraps a closure and turns a panic into a `Fault`.
`gpu::fault::catching_on` adds wgpu error scopes for all three `ErrorFilter`s around the same closure.
Scopes are thread-local in wgpu 30 (`scopes: HashMap<ThreadId, Vec<ErrorScope>>`).
The sink exists for that reason, rather than scopes everywhere.

1. **Build.** `ModelFactory` returns `Result<ModelState, Fault>`; the four `register_*` functions wrap their factory, the GPU pair with scopes.
   `AppState::build_runner` then wraps the sim thread's construction too, putting `TimestampQuery::new` and the CPU thread's initial `build_snapshot` inside the same catch.
2. **Sim threads.** Both `SimThread::new` and `GpuSimThread::new` wrap `sim_loop.run()` **once at thread start**, outside the loop.
   Nothing enters a per-tick path.
   The GPU loop additionally checks `ctx.faults.is_set()` at the head of each iteration.
   A device error raised by `submit` arrives in the sink rather than unwinding.
3. **The device floor.** `GpuContext::new` installs `on_uncaptured_error`.
   That is the backstop for everything not inside a scope, egui's own render passes included.

Engine internals were not touched.
`GpuGridState::new_seeded`'s `assert!`s stay panics on purpose: they are model-author bugs, and the boundary now reports them as a modal either way.

The CPU thread takes a `FaultSink` parameter while the GPU one reads `ctx.faults`.
Asymmetric, deliberately: the GPU context already carries the sink the device writes into, and giving `GpuSimThread::new` a second one would let the two disagree.

### Four defects found in review

The maintainer read the finished branch and found four, two of them real bugs in the shipped design.

**The panic location was lost on every hot path.**
`LAST_PANIC_LOCATION` was a bare `thread_local!`.
Rayon runs `step_cell` on a worker, so the hook wrote that worker's TLS, and `resume_unwind` landed on the sim thread without running the hook again.
Every `GridModel` and `AgentModel` kernel panic showed its message and no line, which is precisely the case the location was added for.
The two tests that existed both missed it: `game_of_life::init` runs single-threaded on the UI thread, and the `Exploding` test state panics on the sim thread itself.
`fault.rs` now keeps `RECENT_PANIC_SITES` beside the thread-local, keyed by panic message and read newest first.
Keying matters. A single global slot was tried first and the full test run caught it being stolen by a concurrent `catching`, so the fallback can only ever return the right site or none.

**A fault from the outgoing run could abort a successful rebuild.**
`reset_simulation` joined the old thread and built a new one without ever draining `render_ctx.faults`.
A panic recorded on the way out sat in the sink, `logic()` took it the next frame, and `report_fault` offloaded the model that had just built.
Both `reset_simulation` and `offload_simulation` now drain after the join, never before it.

**AGENTS.md described the trap this work closed.**
Its GPU section still said there was no `push_error_scope` around model construction and that workgroup size and uniform layout "surface the fatal way".
Rewritten, with the thread-local scope caveat and the rayon location trap added beside it.

**A factory `Err` was reported as the wrong backend.**
`let Ok(ModelState::Gpu(..)) = create() else { panic!("... not ModelState::Cpu") }` in the two GPU model tests blamed the backend for a build failure.
Split into two steps.

### Writing style

A second pass swept the whole diff against the guidelines the maintainer added to AGENTS.md mid-session.
Three constructions came out everywhere: the cleft ("X is what makes Y", including "which is why"), the trailing "which" clause, and the parallel frame ("outside the loop, not inside it").
"X, so Y" was carrying 24 of the diff's reasons on its own and got thinned to one.

The guidelines themselves were restructured.
Sentence-level rules moved into a new `### Sentences` section above the per-surface ones.
They apply to comments, UI text and markdown alike, and the rule that appeared verbatim in two sections now lives in one.

## State after

Branch `31-gpu-error-handling`, uncommitted, `50481c0` as the base.
`./check.sh` passes, as does the full suite under `HENAD_REQUIRE_GPU=1`.

### Verified in the running app

Two deliberate breaks, each confirmed through the egui MCP server and then reverted.

| Break                                                    | What the modal said                                                                                                |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `1 / 0` in `game_of_life.rs::init`                        | "Game of Life panicked while building the model, at crates/henad-models/src/game_of_life.rs:46" + "attempt to divide by zero" |
| `@workgroup_size(32, 32)` in `gpu_game_of_life/step.wgsl` | the full wgpu validation error, naming `gpu_game_of_life_step_pipeline` and `max_compute_invocations_per_workgroup` |

In both cases the app stayed alive at ~500 FPS with every panel intact.
After dismissing the second, GPU boids built and ran 50k agents on the same device.
Offering "dismiss and try again" rests on exactly that.

The workgroup-size case is one AGENTS.md listed as still surfacing "the fatal way". It no longer does.

### Measured

Interleaved runs of two release `henad-cli` binaries built from the same tree with and without the change, Game of Life, 300 steps x 5 reps, five alternating rounds.

| | mean steps/sec | spread within arm |
| --- | --- | --- |
| baseline | 10 844 | 10 428 – 11 166 |
| with faults | 10 992 | 10 809 – 11 171 |

The 1.4% gap is noise.
It is smaller than the spread inside either arm.
The expected result, with the catch outside every loop.

### New tests

- `fault.rs` — a panic becomes a `Fault`, the message survives, the hook attaches a location, a location survives a panic on a rayon worker, a stale location is not pinned onto the next catch, the sink keeps the first of two.
- `gpu/fault.rs` — an over-`max_buffer_size` allocation comes back as `FaultKind::Device` instead of panicking; the device still works afterwards; a panic inside a scope leaves the scopes balanced.
- `cpu/sim_thread.rs` — a `SimState` whose `step()` divides by zero lands in the sink and wakes the UI.
- `registry.rs` — a broken `GridModel` makes `(entry.create)` return `Err` rather than unwind, and one that panics in `step_cell` still reports the line it panicked on.

The last two print a real panic through the chained hook.
Both are noisy by construction.

## Issues found & future directions

- **`Device::poll` is still fatal inside wgpu.** `wgpu_core.rs:1924` routes non-poll errors to `handle_error_fatal`. That bypasses both error scopes and the uncaptured handler and panics unconditionally. A genuine device loss during `poll_blocking` still takes the sim thread down, but the thread-level catch turns it into a reported fault rather than a silent freeze. The outcome is graceful even where the mechanism is not.
- **Paint callbacks are uncovered.** A panic inside `viewport.rs` or `agent_layer.rs` unwinds into eframe. The handler is device-wide, so device errors there still reach the sink. Panics do not.
- **Wasm catches nothing.** Panics abort on `wasm32-unknown-unknown`. `catching` is inert there, and `SimThread::new`'s sink parameter exists only for signature parity. The web build's behaviour is unchanged.
- **The handler is shared with egui's renderer.** Swallowing an egui error means egui draws on with a broken resource rather than crashing. Better than a panic, but a real trade; the handler logs at `error!` so it is at least visible.
- **A cascade is only as good as its first entry.** `FaultSink::set_once` keeps the first fault and drops the rest. A device error usually produces many, so that is the right call. A second unrelated fault arriving before the UI's next frame is still lost.
- **No device-loss test exists.** Nothing here can trigger a real device loss locally. The `Internal` filter and the lost-device path are covered by construction only.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

- Designed this session.
- Edited comments, UI strings and writing guidelines in AGENTS.md.
- Reviewed all changes and identified 4 issues, including 2 logical bugs and 2 documentation issues. (see above)
- Fixed all issues and verified the fixes with tests.
