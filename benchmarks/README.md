# Cross-engine benchmarks

Henad against MASON, Agents.jl, NetLogo, Mesa and krABMaga, on the same four models.

Each engine gets one directory holding a harness and one implementation per model.
Every implementation is written from the same declaration and checked against Henad before it is
timed, so a row in the results table compares engines rather than simulations.

- [`protocol.md`](protocol.md) is the interface every harness implements.
- The declarations are the fixture documents under `crates/henad-models/tests/fixtures/docs/`.
- Results and the write-up land on the [benchmarks page](../docs/benchmarks.md).

## The rule the ports follow

Write what a competent user of that engine would write, using its documented API, and nothing more.
No engine-specific trick its own documentation does not recommend, and no reaching past the API to
the internals. An engine is being measured as its users would experience it.

## Running

```bash
cargo build --release -p henad-cli
uv run --project scripts scripts/validate_ports.py     # every port against Henad
uv run --project scripts scripts/compare_bench.py      # the sweep
uv run --project scripts scripts/plot_compare.py --publish
```

`compare_bench.py --dry-run` prints the matrix and which engines it found, running nothing.
`--smoke` runs one small point per engine and model, which is the quickest check that a change to
a harness still works.

A sweep takes hours, so it is built to be interrupted.
Each point is written and flushed as it finishes, and `--resume` picks up from there:

```bash
uv run --project scripts scripts/compare_bench.py --resume
```

With no `--out`, that continues the most recently written sweep and says which one, so an overnight
run resumed the next morning does not start a second file.
A resumed sweep also remembers which ladders had already stopped, rather than climbing back into
the rungs a slow engine had given up on.
Pointing a fresh sweep at a file that already holds one is refused; pass `--force` to replace it.

Engines that are not installed are skipped with a reason, so a partial machine still produces a
partial table.

## Prerequisites

| Engine | Needs | Where the driver looks |
|---|---|---|
| Henad | this repository, built with `--release` | `target/release/henad-cli` |
| Mesa | `uv` | `benchmarks/mesa`, its own locked project |
| NetLogo | NetLogo 7 and a JDK | `$NETLOGO_HOME`, else `/Applications/NetLogo 7.0.4` |
| MASON | `mason.22.jar` and a JDK | `$MASON_JAR`, else `benchmarks/mason/mason.22.jar` |
| Agents.jl | Julia | `julia` on the path, project `benchmarks/agents_jl` |
| krABMaga | cargo | `benchmarks/krabmaga`, outside the workspace |

The MASON jar is not committed.
`benchmarks/mason/fetch_mason.sh` downloads it and checks its digest.

## Reading a number

Everything on the benchmarks page is a step counter from a release build, with construction outside
the timed window.
Two engines' numbers are comparable only on the same machine in the same sweep; the page says which
machine each table came from.
`docs/authoring/performance.md` has the rest of the house rules.
