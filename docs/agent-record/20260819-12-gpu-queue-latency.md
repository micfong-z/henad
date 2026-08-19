---
date: 2026-08-19
title: "Why a running GPU model freezes the app: queue latency, not batch size"
model: claude-opus-5 (Claude Code)
issue: none
status: complete — root cause measured, fix implemented and verified in the running app
baseline_commit: 38d0436
delta_state: uncommitted working tree on `master`
---

# GPU run stutter

> The GPU sim thread submits to the queue egui renders on and never waits for a submission to finish.
> At steady state a dozen or more sim command buffers are always outstanding, and every egui frame is submitted behind them.
> Frame latency is standing queue depth times per-submission GPU time, which is why the batch-size slider cannot fix it and why Step is instant.
> The loop now waits for a batch before encoding the next, sizes batches from that measurement, and splits every batch into submissions small enough to execute.
> Maxed-out `gpu_boids` went from under 1 fps to 17, and 500k agents to 63, with throughput unchanged.

## State before

At `38d0436` on `master`, working tree clean.

The reported symptom: running a GPU model makes the whole app stutter, down to under 1 fps on maxed-out `gpu_boids`.
Stepping the same model with the Step button is instant.
Setting the batch size to 1 does not help.

`gpu/sim_thread.rs`'s module doc already named the suspect and marked it unverified:

> That assumes a continuously busy queue backpressures how fast `submit()` can be issued, which is plausible but has **not** been verified.

## What was done

No source file changed.
Three throwaway integration tests under `crates/henad-models/tests/` reproduced the sim loop's submission pattern against a headless device and were deleted afterwards.
Each built `gpu_boids` through the real registry, so the state under measurement is the one the app runs.

The stand-in for an egui frame is an empty command buffer, submitted and waited on.
It measures only how long the queue makes a newcomer wait, with none of egui's own work mixed in.

Machine: M4 Pro, release build, `gpu_boids` at 200k agents in a 4000x4000 world and at the slider maximum of 1M agents in 10000x10000.

### The loop never waits

`GpuSimLoop::step_batch` encodes `batch_size` steps, calls `queue.submit`, and goes straight round again.
The only blocking call on the running path is `TimestampQuery::resolve_after`, gated to once a second.
`poll_stats_readback(.., false)` reaches `device.poll(PollType::Poll)`, which returns immediately and only when a readback is already in flight.

Nothing bounds how much work can be outstanding.
wgpu's Metal backend creates its queue with `MAX_COMMAND_BUFFERS = 4096` (`wgpu-hal-30.0.0/src/metal/adapter.rs:48`), so `submit` returns without complaint far past any useful depth.

### Frame latency against the running loop

Submitting flat out, sampling the fake frame as it goes:

| model | batch | true GPU cost | sim rate | frame latency |
| --- | --- | --- | --- | --- |
| 200k agents | 64 | 7.8 ms/step | 131 TPS | 2640 ms |
| 200k agents | 1 | 5.3 ms/step | 138 TPS | 56 ms |
| 1M agents | 1 | 39.1 ms/step | 24 TPS | 332 ms |

An idle queue answers the same fake frame in 66 µs.

The shape is `depth x per-submission time`.
Shrinking the batch shrinks the second factor and the depth grows to fill the gap, which is why the slider feels inert.
It does buy something, 2640 ms down to 56 ms at 200k, but never enough: 332 ms at the maximum is 3 fps before egui has drawn anything.

Step is unaffected because `SimCommand::StepOnce` submits one command buffer and `snapshot_now` blocks on the readback.
The queue is empty on either side of it.

### The adaptive controller measures the wrong thing

Running the real controller (`ema_update` / `next_batch_size`, target 8 ms) over the real state, from a cold queue:

```text
iter  1: submitted batch    64 in     4.3 ms -> sample 0.06762 ms/step, next batch 118
iter  2: submitted batch   118 in    56.7 ms -> sample 0.48063 ms/step, next batch 46
iter  6: submitted batch    22 in    24.3 ms -> sample 1.10476 ms/step, next batch 14
iter  7: submitted batch    14 in   141.8 ms -> sample 10.13046 ms/step, next batch 2
iter 12: submitted batch     1 in    10.3 ms -> sample 10.27983 ms/step, next batch 1
frame stall after the adaptive run: 2958 ms
```

The first sample is CPU encode time, 0.068 ms against a true 7.8 ms, off by two orders of magnitude.
The controller only sees something like the truth from iteration 7, once the queue is full enough that `submit` blocks.
By then 326 steps have been committed in 349 ms of wall clock, roughly 2.5 s of GPU work, and the backlog that buys is what the UI sits behind.
It settles around 315 ms and stays there: the loop keeps refilling at exactly the drain rate.

On a cheap model the same error runs the other way and pins the batch at `MAX_BATCH_SIZE`.

### A 4096-step submission is silently dropped

`MAX_BATCH_SIZE` is 4096, and `agent_engine.rs:31` documents 64 as the ceiling a submission must stay under.
`STEPS_PER_SUBMISSION` is only honoured by `run_batched`, which tests call.
The runner uses `self.batch_size`, so the controller can reach 64x the documented limit.

Same total step count, same model, two batch sizes:

```text
4096 steps in batches of 64   -> 28211.6 ms wall, tick 4096, avg speed 5.235
4096 steps in batches of 4096 ->    39.3 ms wall, tick 4096, avg speed 0.000
```

That is the watchdog trap the GPU notes already describe.
No error, no fault in the sink, the tick counter still reports 4096, and the reduce reads back zeros.

### Bounding the queue is what fixes it

Same loop, with a `device.poll(Wait)` on submission N-1 before encoding N, so at most one sim submission is outstanding when a frame arrives:

| model | batch | in flight | sim rate | frame latency |
| --- | --- | --- | --- | --- |
| 200k agents | 1 | unbounded | 138 TPS | 56 ms |
| 200k agents | 1 | one | 137 TPS | 7.1 ms |
| 200k agents | 64 | unbounded | 131 TPS | 2640 ms |
| 200k agents | 64 | one | 132 TPS | 557 ms |
| 1M agents | 1 | unbounded | 24.4 TPS | 332 ms |
| 1M agents | 1 | one | 23.7 TPS | 42 ms |

Throughput is unchanged to within noise, which is the important half.
The GPU is saturated either way; the queue depth was buying latency and nothing else.

Batch 64 at 200k is still 557 ms bounded, because one batch is 500 ms of GPU work and a frame cannot interleave with it.
Latency has a floor of one batch however shallow the queue is, so the batch has to be sized against true GPU time.

At 1M agents a single step is 39 ms, longer than a 60 fps frame, so 24 fps is the ceiling there whatever the runner does.

### Where the submission ceiling actually is

The 4096-step failure above reads like a time-based watchdog and is not.
`gpu_boids` at 5k agents runs 4096 steps in 700 ms when split, and still comes back zero in one submission.
Bisecting, one fresh device per run since a tripped submission takes the device with it:

| model | passes per step | last size that works | first size that fails |
| --- | --- | --- | --- |
| `gpu_boids` | ~5 compute plus 2 copies | 384 | 448 |
| `gpu_game_of_life` | 1 compute | 1024 | 2048 |

Both land near two thousand encoded passes, so the ceiling is the pass count and a model with more passes per step reaches it sooner.
That rules out sizing a submission by time and settles it in steps.

Chunking cost was measured on the cheapest case, `gpu_game_of_life` at 512x512, interleaved best-of-five across chunk sizes 64 to 384.
The spread stayed inside the machine's own drift, so 64 is affordable.

## What was changed

```text
crates/
├── henad-compute/src/gpu/
│   ├── mod.rs                ← MAX_STEPS_PER_SUBMISSION, the one ceiling
│   ├── sim_thread.rs         ← waits for a batch, chunks a batch, measures a batch
│   └── agent_engine.rs       ← private STEPS_PER_SUBMISSION deleted, uses the shared one
├── henad-models/src/registry.rs   ← a_full_submission_executes_every_step
└── henad-cli/src/main.rs     ← its own BATCH = 256 deleted, uses the shared one
```

**One ceiling instead of three.**
`MAX_STEPS_PER_SUBMISSION` is 64, and the runner, `run_batched` and the CLI all chunk on it.
Before, `agent_engine.rs` said 64, the CLI said 256, and the runner said whatever the controller last produced.

**`step_batch` waits.**
`await_previous` blocks on the previous batch's `SubmissionIndex` before the next is encoded.
One batch outstanding is what keeps the CPU encoding while the GPU runs, and it is the whole fix for the freeze.

**The controller measures the batch.**
The sample is one loop period, from the start of encoding a batch to the GPU finishing it.
Nearly all of it is the GPU executing, which is what a frame behind the batch waits for.
`target_ms` is now directly the worst frame latency the sim adds, which is what the tooltip already claimed.

The EMA updates in both modes now that it is a real measurement, so the reset on switching to adaptive is gone.
It threw away a valid number.

**A batch splits across submissions.**
They go out back to back and only the last is waited for, so the split costs a few encoders and no latency.
Only the first submission carries the timestamps, and the reported per-step time divides by that chunk.

## State after

Full `check.sh` green under `HENAD_REQUIRE_GPU=1`, including `trunk build`.

Verified in the running app, `gpu_boids` through the egui MCP server:

| agents | world | GPU time/step | TPS | app FPS |
| --- | --- | --- | --- | --- |
| 1M (slider max) | 10000x10000 | 57.8 ms | 19 | 17 |
| 500k | 10000x10000 | 15.7 ms | 62 | 63 |

Both cases ran under 1 fps before.
The adaptive controller converged to a batch of 1 in both, which is correct: one step already costs more than a frame at these sizes, so a step is the floor and the app sits on it.
Play, Pause and Step all behave, and flocking structure appears on screen as it should.

Against the same measurement harness as the diagnosis, at 200k agents: median frame latency 0 ms and p99 9.6 ms, against 2640 ms before.
TPS moved 131 to 108 at 200k and 24.4 to 22.7 at 1M, both inside this machine's drift.

## Issues found & future directions

- **A fixed batch is still atomic against a frame, by definition.** The slider offers up to 2000, and 2000 steps of maxed `gpu_boids` blocks the UI for 14 s. Adaptive is the default and protects against it, and the number is the user's explicit choice, but nothing in the panel says what it costs.
- **`a_full_submission_executes_every_step` is the guard, and it is a floor not a proof.** It steps every registered GPU model through one full-size submission and fails if every stat reads back zero. Raising the const to 2048 fails it on `gpu_game_of_life`. It cannot catch a model that legitimately reports zero for every series.
- **The empty-command-buffer frame proxy understates latency once the queue is shallow.** Metal can retire a buffer with no resource dependencies out of order, so the median reads 0 ms where a real frame sampling the display texture would not. It separates the two regimes by three orders of magnitude, which is all it was used for, but p99 is the number to read.
- **`gpu_ants` was measured only through the submission-ceiling test.** Its step is two passes over in-place buffers with a persistent counter, so its per-step cost and its margin under the ceiling are still unknown.
- **The timestamp path lost coverage of a split batch.** It stamps the first chunk only. Stamping across chunks needs `encode_steps` to take the opening and closing stamps separately.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

- Identified the error and guided the root cause of the issue.
- Comment and readability improvements to the codebase.
