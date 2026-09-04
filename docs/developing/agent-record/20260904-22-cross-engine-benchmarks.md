---
date: 2026-09-04
title: "Cross-engine benchmarks — the harness, a gate for ants, and the Mesa port"
description: The driver, protocol and validation gates for comparing Henad against five other ABM engines, the scenario that finally makes ants checkable across engines, and the first reference implementation.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: 25
state: harness protocol written and Henad's side of it implemented, driver and gate scripts running end to end, ants gated across engines for the first time, all four models ported to Mesa and passing their gates, benchmarks page written
baseline_commit: 3f9f990
delta_state: uncommitted on `25-cross-abm-comparison`
---

# Cross-engine benchmarks, part 1

> Issue 25 asks for Henad to be measured against MASON, Agents.jl, NetLogo, Mesa and krABMaga.
> This session built the machinery every later engine plugs into: a harness contract, a driver, a validation gate per model, and the plotting and page that turn a sweep into something publishable.
> The one piece of real modelling work was ants, which had no cross-engine check at all and now has one.
> Mesa is then the first engine through it, with all four models ported and gated.
> Four engines are left, one per session.

## State before

`AGENTS.md` opens by saying other frameworks "top out around 100k–1M agents", and nothing in the repository measured it.
Issue 7 had settled that consistency checks stop at NetLogo and left the rest to issue 25, so this is a performance comparison with correctness gated by the fixtures issue 7 built.

Those fixtures covered three models.
Game of Life and boids each had a NetLogo reference and a `matches_every_reference_fixture` test that walks its directory.
SIR had a distributional comparison through `compare_sir.py`, already returning equivalent against NetLogo 7.0.4.
Ants had nothing, and `consistency_ants.rs` said why: the field update and the randomness together made a reference implementation impractical.

`henad-cli` timed the step loop correctly but printed for people, so `bench_matrix.py` read it back with regular expressions.
It had no way to pin the thread count and never printed `heap_bytes`, which `SimState` has offered all along.
`docs/benchmarks.md` was a nine-line stub, left that way deliberately until the numbers had framing.

## What was done

### Ants can be checked across engines after all

The blocker was randomness inside the movement rule.
An ant draws in three places, all in `advect_agent`: once to break a tie between two neighbours holding the same pheromone, once for the momentum branch, once for the random-action branch.
Two engines can agree on every rule and still disagree on every trajectory, and no amount of seed matching fixes that across five languages.

The scenario removes the draws instead of matching them.
Setting `momentum` and `random_action` to zero means neither branch can fire.
Seeding the field from `((7x + 13y) mod 97 + 1) / 98` and `((11x + 5y) mod 89 + 1) / 90` means no two cells in any 3 by 3 neighbourhood hold the same value, so a tie is never reached and the tie-break never draws.
What is left is the rules, and any correct implementation reaches the same state from any generator.

Three tests hold that up. One checks the seeded field really has no ties and no zeros.
One runs the scenario under six different seeds and requires the same answer, which is the property another engine has to be able to reproduce.
One checks the scenario is not vacuous: ants move and both layers change.

The tick count was measured rather than picked. Deposits gradually recreate ties in the evolved field, and the run stops being generator-independent at nine ticks, so the scenario runs five.

Seeding the field needed a hook. `from_cells` and `from_agents` were added for the Game of Life and boids gates in issues 20 and 21; this is the third of the same kind, `from_agents_and_field`, with a `seed_field` on `ScalarField` underneath it.

The tie-break defect inherited from the reference is deliberately outside the gate.
It is reproduced in every port so the ports stay comparable, and disclosed on the page instead.

### The harness contract

`benchmarks/protocol.md` states one interface every engine implements: the same arguments in, one JSON object per line out, an `info` line then a `rep` line per timed rep as it finishes.
Reps stream, so a run killed by a timeout still reports what it managed.
The driver computes every statistic itself from the rep times, so no engine's own arithmetic reaches a published table.

Henad's side is `henad-cli --json`, alongside `--threads` for rayon's global pool and `heap_bytes` in the rep line.
The human report is untouched, so `bench_matrix.py` keeps working.

### Driver, gates, figures

`compare_bench.py` owns the ladder, the timeouts and the arithmetic, and imports the density rule and parameter discovery from `bench_matrix.py` rather than restating them.
Engines it cannot find are skipped with a reason, so a partial machine gives a partial table.
After a timeout it stops climbing that engine's ladder instead of spending fifteen minutes per remaining rung.

`validate_ports.py` runs each port's gate scenarios, drops the fixtures where the consistency tests look, and lets `cargo test` do the comparison.
The gate a port passes is the gate Henad holds itself to, and there is one implementation of it rather than a second one in Python.

`plot_compare.py` writes the throughput curves, the ratio tables normalised to Henad on one thread, a provenance table of what actually ran, and the lines-of-code table.
Everything the page shows is generated, so a number cannot drift from the sweep it came from.

### One budget per configuration

Engines in this comparison differ by orders of magnitude, so a single ladder cannot suit all of
them, and a slow engine should stop rather than be waited for.
Each point gets a thousand seconds across its reps.
Whatever reps finished are kept and the row says how many, since a median over two reps still
places an engine as long as the row admits it was two.
Once a point goes over, larger ones are not attempted.

### Watching a sweep that runs for hours

The sweep draws a spinner, the run in flight with its rep and remaining budget, and a bar, with
finished runs scrolling above carrying their median.
It falls back to one line per run the moment stderr is not a terminal, which is the usual case: a
sweep gets redirected to a log and left alone.

The one thing worth care was width. A live line wider than the terminal wraps, and the cursor-up
that erases the block counts display lines rather than the lines it was handed, so a single wrapped
line corrupts everything below it. Lines are therefore composed and fitted in plain text before any
colour goes on, and the bar shrinks on a narrow terminal rather than pushing the line over.

### Resuming a sweep

A sweep runs for hours and will be interrupted, so each point is written and flushed as it lands
and `--resume` continues from there.

Three things were wrong when that was first tested against a real kill.
Pointing a fresh sweep at an existing file silently truncated it, which is how an overnight run
gets lost; that is now refused unless `--force` says otherwise.
The set of ladders that had already stopped was not rebuilt, so a resume landing just after a point
went over budget would climb into the larger rungs it was meant to abandon, at a thousand seconds
each.
And the default output name carries the date, so a bare `--resume` the next morning started a
second file; it now continues the most recent sweep and says which one.

A row cut short by a hard kill is left out of the completed set so its point runs again.

### The release profile, measured

Henad ships at `opt-level = 2` for the size of the WebAssembly build, where krABMaga will default to 3.
Interleaved on a quiet machine, level 3 runs Game of Life 1.3% faster and boids 4.8% slower.
There is no case for changing the profile, so it is disclosed on the page instead.

```text
benchmarks/                                       new
    README.md                                     how to reproduce, and the user-code rule
    protocol.md                                   the harness contract
    loc_manifest.toml                             what the lines-of-code table counts
    mason/mason.22.jar                            fetched, not committed
    mesa/                                         new, the first reference engine
        pyproject.toml, uv.lock                   its own locked project
        bench.py                                  Mesa's side of the harness contract
        scenarios.py                              gate starting states, from the declarations
        models/game_of_life.py, sir.py            a cell agent each, two-phase
        models/boids.py                           continuous space, Henad's rule
        models/ants.py                            cell agents over two property layers
crates/
    henad-cli/
        Cargo.toml                                + serde_json, rayon
        src/
            main.rs                               + --json, --threads, heap_bytes
            json_report.rs                        new, the protocol lines
    henad-compute/src/cpu/
        agent_engine.rs                           + from_agents_and_field
        field/scalar.rs                           + seed_field
    henad-models/tests/
        consistency_ants.rs                       + the gate scenario and its fixture reader
        fixtures/{game_of_life,boids,ants}/       + five Mesa reference fixtures
        fixtures/docs/
            ants_fixture.md                       new, the fourth declaration
            sir_fixture.md                        flag rename
docs/
    benchmarks.md                                 the stub replaced
    assets/benchmarks/                            generated figures, tables and the sweep CSV
    reference/cli.md                              + the two flags and the JSON stream
    developing/agent-record/
        20260904-22-cross-engine-benchmarks.md    this file
scripts/
    compare_bench.py                              new, the driver
    validate_ports.py                             new, the gates
    plot_compare.py                               new, figures and tables
    progress.py                                   new, the sweep's live display
    compare_sir.py                                --reference, with --netlogo kept as an alias
    README.md                                     the three new scripts
AGENTS.md                                         + the cross-engine benchmarks section
Cargo.toml                                        + workspace exclude, serde_json
.gitignore                                        + *.jar
```

### Mesa, the first engine through

All four models are ported and all four gates pass.

Mesa ships its examples inside the package, so the shapes are its own rather than invented: a
`FixedAgent` per cell on an `OrthogonalMooreGrid` with a two-phase step for Game of Life and SIR, a
`ContinuousSpaceAgent` for boids, `CellAgent` ants over two property layers.
What is inside those shapes is Henad's rule, not Mesa's.
Mesa's own boids is Reynolds's algorithm with a normalised direction and a constant speed, which is
a different simulation; using it would have measured that difference rather than the two engines.

The two-phase step matters for more than tidiness. Mesa's boids example activates agents in a
shuffled order, so each one sees the moves of those before it. Henad reads the whole population
before writing any of it. Following the Game of Life example's `determine_state` then
`assume_state` shape rather than the boids example's `shuffle_do` is what makes the two comparable
at all, and it is ordinary Mesa either way.

Initial conditions are shared rather than reimplemented, extracted from the declarations into
`benchmarks/mesa/scenarios.py`. Both sides have to begin from the same numbers for a gate to mean
anything; it is the rules that are written twice.

Game of Life came out bit-identical to the NetLogo fixture recorded for the earlier consistency
work, on the first run, which is a stronger check than it looks: three independent implementations
of the same rule agreeing exactly after 101 and 500 ticks.

All four gates passed first time. SIR took fifty replicates a side at 256 squared and about an
hour, and returned equivalent on all three summary statistics with the differences well inside
their margins. Its numbers are now in `sir_fixture.md` beside the NetLogo ones.

## State after

`./check.sh` is green and the ants test file holds seven tests where it held three.

Five Mesa fixtures are committed, so `cargo test` now checks Henad against another engine on three
of the four models with nothing installed but cargo.

The driver runs every Henad row end to end, CPU on one thread, CPU on every core, and GPU, across all four models.
Every other engine reports itself as not yet written, which is the correct answer and the shape the next sessions fill in.

`docs/benchmarks.md` carries the method in full: what is timed, the ladder, the thread story, and a gate table saying what each model's check actually proves.
Its result tables and figures hold Henad's own rows, generated from a sweep rather than typed.
Those numbers are provisional and the page says so: the sweep ran on a laptop that was not idle and
was cut short of its last few points, so it shows the shape of the tables rather than anything to
cite.

One number worth keeping in view: at 64 squared, Game of Life on every core is roughly five times *slower* than on one thread.
Four thousand cells is not enough work to pay for a rayon fan-out, and the crossover is somewhere above it.
That is a real property of the engine and the ladder now shows it rather than starting past it.

## Issues found & future directions

- **The lines-of-code column is not yet an apples-to-apples row.** Henad's model files also declare
  the model's parameters, statistics and palette, which in Mesa live in the harness. The page says
  so, but the honest fix is either to count the harness too or to stop counting Henad in that table
  at all. Worth settling before a second engine makes the row look like a trend.
- **`validated.json` replaced rather than merged**, so running one slow gate on its own erased the
  verdicts around it. Fixed, and the general shape is worth watching: three files now accumulate
  state across separate invocations, and each of them got this wrong once.
- **Resume rebuilds state from the CSV, which is the only record.** That works, but it means the
  CSV's columns are now load-bearing for control flow as well as for results. A schema change would
  silently change what a resume skips. Worth a version column, or worth deciding the file is
  append-only.
- **The first budget was per process, which measured nothing.** With one rep per process a
  220-second rep never tripped a 900-second timeout, so boids at a million agents on one thread ran
  for over half an hour before anyone noticed. The budget is now per point across its reps, keeps
  whatever finished, and records how many reps contributed. The general form of the mistake is
  worth remembering: a limit has to be expressed in the same units as the thing being limited.
- **The ants gate stops at eight ticks and nobody is watching.** The scenario runs five, but nothing fails if a future change to the deposit formula moves the divergence point below five. The probe that found nine was thrown away. A test that asserts the scenario is still generator-independent *and* that some longer run is not would pin both ends.
- **`bench_matrix.py` and `compare_bench.py` now overlap.** Both sweep Henad across a scaling axis, and the older one still parses the human report with regular expressions when `--json` would do. Merging them is tempting and probably wrong: the matrix measures warm-up and step-count sensitivity, which the comparison deliberately fixes. Worth a decision rather than drift.
- **The lines-of-code table counts only Henad so far**, and its rule for what counts is crude: non-blank, non-comment, with a NetLogo model reduced to its code section. It will need a harder look once a port exists to compare against, since a generous reading of "comment" moves the number a lot.
- **Nothing yet checks that a port and Henad were given the same parameters.** The driver passes Henad's resolved parameter set to every engine and records it, but a harness that silently ignores a `--set` it does not recognise would produce a plausible row for a different model. The protocol says this is an error; nothing enforces it.
- **The sweep is single-machine.** The published tables come from an M4 Pro, and the existing `results/` history is from a Threadripper with a 4090. The page says numbers from different machines are not comparable, which is right, but the comparison is more interesting run on both and it needs the reference engines installed on the second one.
- **FLAME GPU is out of scope here and probably should not stay out.** It is the one engine facing the same problem Henad's GPU backend faces, and the GPU rows have no external comparator without it. Deliberately deferred to its own issue rather than smuggled into this one.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

<!-- Seeded by the agent: what the human did this session, from the agent's point of view.
     Raw material to reframe, not notes. Delete this block once rewritten.

     - Set the benchmark run-sizing rule: probe small, raise if reasonable, and treat about 1000
       seconds per configuration as the ceiling, reporting the cap instead of waiting. Gave the
       reason as well as the rule, that Mesa can take over 100k seconds where Henad takes a couple,
       so covering every range on every engine is not the goal. This found a real bug: the budget
       had been per process, and with one rep per process a 220-second rep never tripped a
       900-second limit.
     - Asked for a progress display on the sweep, which is what surfaced the width and wrapping
       problem in the live block.
     - Asked whether the sweep was resumable, which is what surfaced three resume bugs, including a
       fresh run silently truncating an existing sweep.
     - Called both sets of numbers provisional while the laptop was under load, and said the real
       ones would come from an idle overnight run.

     - Chose all four models implemented identically in all five engines, over a cheaper split
       where ants would have been stock MASON and krABMaga code with the differences merely
       disclosed. That call is what forced the ants gate to be designed rather than skipped, and it
       removed the vendoring and attribution problem entirely.
     - Asked to be paused before each milestone so each lands as its own commit.
     - Installed Julia 1.12.7 and put mason.22.jar in benchmarks/mason while the plan was being
       reviewed, which took both downloads off the critical path.
     - Set the direction on the earlier plan questions: Henad reported at one thread and at all
       cores rather than all cores only, both machines as publication targets, FLAME GPU deferred
       to its own issue.
-->
