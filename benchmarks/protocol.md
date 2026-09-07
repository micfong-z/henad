# The harness contract

Every reference engine in the comparison is driven through one program with the same interface, so `scripts/compare_bench.py` needs no per-engine knowledge beyond how to start it.

Henad is the exception, and knowingly.
`henad-cli` predates this contract and is a tool in its own right, so it takes its model positionally, sizes a run through `--set`, and has no validate mode, being the thing every port is validated against.
`--json` gives it the output half of the contract, and `Henad.command()` in the driver is the twenty lines that bridge the input half.

## Arguments

What every reference harness accepts.

```text
--model {game_of_life,boids,sir,ants}
--grid W H                    grid models, mutually exclusive with --agents
--agents N --world W H        agent models
--steps S                     timed steps per rep
--warmup W                    untimed steps before each rep, on that rep's own state
--reps R                      timed reps
--seed BASE                   rep i seeds its generator with BASE + i
--threads T                   0 leaves the engine's own choice
--set id=value                repeatable, ids are Henad's parameter ids
--validate SCENARIO --out P   write the fixture for SCENARIO to P and exit, timing nothing
```

A harness that cannot honour `--threads` reports what it actually used and carries on.
Parameters an engine does not have are an error, not a silent default: a mismatched parameter set means the two runs are not the same model.

## Output

One JSON object per line on stdout, nothing else.
Progress and warnings go to stderr, where the driver captures them for the `error` column.

```json
{"kind":"info","engine":"mesa","engine_version":"3.5.1","model":"boids","variant":"default","threads":1}
{"kind":"rep","rep":0,"seed":42,"steps":100,"warmup":10,"elapsed_s":1.2345,"population":1000,"heap_bytes":null}
```

`info` comes once, before any rep.
`rep` comes once per timed rep, as soon as that rep finishes, so a run killed by a timeout still reports what it managed.
`heap_bytes` is optional and null where an engine cannot measure it.

A `summary` line may follow the reps.
The driver computes its own statistics from the `rep` lines and uses `summary` only for provenance, so no engine's own arithmetic reaches a published table.

## What is timed

The step loop, and nothing else.
Construction, initial population, warm-up and teardown all sit outside the window, and the timer is the engine's own monotonic clock rather than the wall time of the process.
An engine that compiles or JITs runs one full untimed rep first.

## Validate mode

`--validate` runs a fixed scenario and writes the state it reaches, in the fixture format the matching document under `crates/henad-models/tests/fixtures/docs/` gives.
`scripts/validate_ports.py` compares that against Henad and records a verdict per engine, variant and model.
`compare_bench.py` reads those verdicts and skips any port whose gate did not pass, unless `--allow-unvalidated` says otherwise.

| Scenario | Declaration |
|---|---|
| `glider`, `r-pentomino` | `game_of_life_fixture.md` |
| `boids-8`, `sine-42` | `boids_fixture.md` |
| `ants-lattice` | `ants_fixture.md` |
| `sir-replicates` | `sir_fixture.md` |
