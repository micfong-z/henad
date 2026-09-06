# Cross-engine benchmarks

Henad against MASON, Agents.jl, NetLogo, Mesa and krABMaga, on the same four models.

Each engine gets one directory holding a harness and one implementation per model.
Every implementation is written from the same declaration and checked against Henad before it is timed, so a row in the results table compares engines rather than simulations.

- [`protocol.md`](protocol.md) is the interface every harness implements.
- The declarations are the fixture documents under `crates/henad-models/tests/fixtures/docs/`.
- Results and the write-up land on the [benchmarks page](../docs/benchmarks.md).

## The rule the ports follow

Write what a competent user of that engine would write, using its documented API, and nothing more.
No engine-specific trick its own documentation does not recommend, and no reaching past the API to the internals.
An engine is being measured as its users would experience it.

## Running

```bash
cargo build --release -p henad-cli
uv run --project scripts scripts/validate_ports.py     # every port against Henad
uv run --project scripts scripts/compare_bench.py      # the sweep
uv run --project scripts scripts/plot_compare.py --publish
```

`compare_bench.py --dry-run` prints the matrix and which engines it found, running nothing.
`--smoke` runs one small point per engine and model, which is the quickest check that a change to a harness still works.

A sweep takes hours, so it is built to be interrupted.
Each point is written and flushed as it finishes, and `--resume` picks up from there:

```bash
uv run --project scripts scripts/compare_bench.py --resume
```

With no `--out`, that continues the most recently written sweep and says which one, so an overnight run resumed the next morning does not start a second file.
A resumed sweep also remembers which ladders had already stopped, rather than climbing back into the rungs a slow engine had given up on.
Pointing a fresh sweep at a file that already holds one is refused; pass `--force` to replace it.

Engines that are not installed are skipped with a reason, so a partial machine still produces a partial table.

## What CI covers

The `ports` job compiles krABMaga under both features, imports every Mesa model and instantiates the
Agents.jl project, then smoke-runs one tiny benchmark on each.
MASON and NetLogo need a jar CI does not have, so nothing checks them until someone runs the gates.

No job re-derives a fixture.
The 25 committed fixtures are a snapshot: `cargo test` checks Henad against them, which catches a
change in Henad, and only `validate_ports.py` catches a change in a port.

## Prerequisites

| Engine | Needs | Where the driver looks |
|---|---|---|
| Henad | this repository, built with `--release` | `target/release/henad-cli` |
| Mesa | `uv` | `benchmarks/mesa`, its own locked project |
| NetLogo | NetLogo 7 and a JDK | `$NETLOGO_HOME`, else `/Applications/NetLogo 7.0.4` |
| MASON | `mason.22.jar` and a JDK | `$MASON_JAR`, else `benchmarks/mason/mason.22.jar` |
| Agents.jl | Julia | `$JULIA`, else `julia` on the path, else juliaup's `~/.juliaup/bin/julia` |
| krABMaga | cargo | `benchmarks/krabmaga`, outside the workspace |

The MASON jar is not committed.
`benchmarks/mason/fetch_mason.sh` downloads it and checks its digest.

## Engine notes

Things a reader of a results table has to know, that the table itself cannot say.

- **NetLogo** runs all model code on one job thread, and its world is a patch grid, so a world side
  is rounded to whole patches. Its boids and SIR models come from the reference models the earlier
  consistency work validated. Each file's header records what was changed from that copy: both gain
  a setup procedure, boids loses a global and re-signs its wrap, and SIR's `go` no longer repaints
  every patch, since the copy that does is never timed. Boids still sorts its neighbour set inside
  the timed `go`, worth 21% of its step, and that was left in place rather than tuned away.
- **MASON** gets its synchrony in the two agent models from schedule orderings, since it shuffles
  steppables that share a time and an ordering. The two grid models are two-pass instead. `SimState.doLoop` is never used, because it folds `start()` into the time it
  reports.
- **Agents.jl** steps on one thread. Its own parallelism is `ensemblerun!` across independent
  replicates, which is not what a thread count means here.
- **krABMaga's boids** refuses a world narrower than twice `visual_range`, which its seam-probing
  neighbour search needs. No rung on the ladder reaches that.
- **Mesa turns the cyclic collector off inside its timed window**, where MASON's and NetLogo's
  collectors run inside theirs. Both are disclosed rather than equalised, since either choice
  favours somebody.
- **krABMaga's `parallel` feature does not run agents concurrently.** Its scheduler takes one lock
  around the whole state for each agent's step, so the workers serialise, and the feature also swaps
  the flat field vectors for sharded hash maps. Measured on boids, the parallel build spends less
  than one core of user time and the rest in the kernel contending for that lock. It is reported as
  a separate row because it is a real configuration a user can select, not because it is faster.

## Reading a number

Everything on the benchmarks page is a step counter from a release build, with construction outside the timed window.
Two engines' numbers are comparable only on the same machine in the same sweep; the page says which machine each table came from.
`docs/authoring/performance.md` has the rest of the house rules.
